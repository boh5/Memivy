//! Real-model acceptance evidence using synthetic memories only.
//! Writes full inputs/results for human semantic review; no credentials copied.
use memivy_core::{
    memory::*,
    model::{ModelConfig, tools},
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn seed(s: &MemoryStore, title: &str, body: &str) -> CaptureResult {
    let c = s
        .capture(&CaptureRequest {
            request_id: id(),
            text: body.into(),
            origin: Origin::User {
                app: "Synthetic acceptance".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap();
    if title != body {
        s.edit_memory(&EditRequest {
            request_id: id(),
            memory_id: c.memory_id.clone(),
            expected_version: c.version_id.clone(),
            title: title.into(),
            body: body.into(),
        })
        .unwrap();
    }
    c
}
async fn ask(
    s: &MemoryStore,
    config: &ModelConfig,
    topic: &str,
    text: &str,
    focus: &[String],
) -> Value {
    let input = id();
    let attempt = id();
    let e = s
        .begin_agent_input(&input, &attempt, topic, text, focus, None)
        .unwrap();
    let started = std::time::Instant::now();
    let mut updates = 0;
    let mut mutations = 0;
    let result = s
        .run_discussion(config, &input, &attempt, "zh-CN", |changed| {
            updates += 1;
            if changed {
                mutations += 1;
            }
        })
        .await;
    if result.is_err() {
        let _ = s.stop_agent_input(&input, &attempt, "failed", Some("evaluation_failed"));
    }
    let execution = s.agent_execution(&input).unwrap();
    let message = s.turn(&input).unwrap().assistant;
    json!({"input_id":input,"user_message_id":e.user_message_id,"input":text,"focus":focus,
        "result":result.map_err(|e|format!("{e:?}")),"message":message,"execution":execution,
        "stream_updates":updates,"memory_updates":mutations,"elapsed_ms":started.elapsed().as_millis()})
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (3..=4).contains(&args.len()),
        "discussion_probe PRIVATE_CONFIG FRESH_OUTPUT_DIR [CASE[,CASE...]]"
    );
    let config =
        ModelConfig::read(std::path::Path::new(&args[1])).expect("current private configuration");
    let output = std::env::current_dir()
        .unwrap()
        .join(PathBuf::from(&args[2]));
    fs::create_dir(&output).expect("fresh output directory required");
    let capabilities = tools::probe(&config)
        .await
        .expect("actual protocol capability");
    assert!(
        capabilities.supports_agent(),
        "configured model does not support Agent"
    );
    fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&json!({"model":config.model,"capabilities":capabilities,"context_token_upper_bound":65536,"output_reserve":8192,"max_model_steps":12,"semantic_review":"pending","synthetic_only":true,"memory_write_contract":"parts_with_source_quotes"})).unwrap()).unwrap();
    for case in [
        "record_and_question",
        "global_focus",
        "state_changes",
        "pause",
        "topic",
        "history",
        "long_memory",
        "material_changes",
        "versioned_update",
        "compaction",
    ] {
        if args
            .get(3)
            .is_some_and(|filter| !filter.split(',').any(|selected| selected == case))
        {
            continue;
        }
        let s = MemoryStore::open(output.join(case)).unwrap();
        tools::save_capabilities(&output.join(case), &config, &capabilities).unwrap();
        let topic = id();
        let mut focus = vec![];
        let mut turns = vec![];
        let mut rubric = String::new();
        let mut dataset = Value::Null;
        match case {
            "record_and_question" => {
                s.create_conversation(&topic, "随时表达").unwrap();
                for text in [
                    "我有个想法：做一个帮助个人整理访谈的小工具，目前只是考虑。",
                    "我刚才的想法是什么？",
                    "我决定先访谈3位独立开发者，还没有开始。你觉得可以怎么安排？",
                ] {
                    turns.push(ask(&s, &config, &topic, text, &focus).await);
                }
                rubric.push_str("Turn 1 saves only a tentative idea with a brief record_only=true receipt. Turn 2 recalls without new facts. Turn 3 saves the decision to interview three people as not yet executed and gives two or three suggestions. No mandatory confirmation.");
            }
            "global_focus" => {
                let product = seed(
                    &s,
                    "新产品",
                    "想做给独立开发者整理访谈的产品，方案尚未确定。",
                );
                seed(&s, "每周时间", "我每周仅10小时能用于个人项目。");
                seed(&s, "项目预算", "我的新项目总预算只有5000元。");
                focus.push(product.memory_id);
                s.create_conversation(&topic, "可行性").unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "这个新产品可行吗？帮我规划一个能做完的第一步。",
                        &focus,
                    )
                    .await,
                );
                let collection = id();
                s.save_collection(&collection, "新产品", "访谈整理", None)
                    .unwrap();
                s.collect_record(
                    &collection,
                    &RecordKey {
                        kind: "memory".into(),
                        id: focus[0].clone(),
                    },
                    true,
                )
                .unwrap();
                let scoped = id();
                s.create_scoped_conversation(&scoped, "专题可行性", Some(&collection))
                    .unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &scoped,
                        "这个新产品可行吗？帮我规划一个能做完的第一步。",
                        &[],
                    )
                    .await,
                );
                turns.push(
                    ask(
                        &s,
                        &config,
                        &scoped,
                        "补充一个限制：必须离线处理，不能上传访谈录音。按这个限制调整刚才的第一步。",
                        &[],
                    )
                    .await,
                );
                rubric.push_str("Both focused-material and collection entry points include the global 10-hour and CNY 5,000 constraints before the first Agent request. Answers must use and cite them without deciding for the user. The later offline/no-upload constraint must be saved and applied.");
            }
            "state_changes" => {
                s.create_conversation(&topic, "收费决定").unwrap();
                for text in [
                    "我考虑给木桥项目收费，但还没决定。",
                    "现在决定收费，先每年99元。还没有上线，也没有收款。",
                    "朋友小李认为应该月付，这是他的建议，我尚未采纳。",
                    "刚才99元说错了，应是每年79元，其余不变。现在我的决定和执行状态分别是什么？",
                ] {
                    turns.push(ask(&s, &config, &topic, text, &focus).await);
                }
                rubric.push_str("Preserve tentative, decided, and not-yet-launched/paid states. A friend's advice must not become the user's decision. Correct the annual price to CNY 79 in memory and answers while retaining the unexecuted state.");
            }
            "pause" => {
                seed(
                    &s,
                    "已有的产品探索限制",
                    "我每周用于产品探索最多6小时，访谈和尝试新方向都算在内。",
                );
                s.create_conversation(&topic, "暂停记忆").unwrap();
                for text in [
                    "这轮别记：我随口设想把产品改成游戏，帮我想一下可能性。",
                    "我想到先做5次用户访谈，这个想法可以记下。",
                    "接下来先别记，我想结合已有的产品探索限制，试想另一条方向。",
                    "也许完全不做这个产品，结合已有的产品探索限制给我两个思考角度。",
                    "恢复记忆。我决定先保留产品方向，安排5次访谈，但尚未执行。",
                ] {
                    turns.push(ask(&s, &config, &topic, text, &focus).await);
                }
                rubric.push_str("Turn 1 makes no writes; turn 2 saves normally. Turns 3 and 4 remain paused but recall the existing six-hour exploration limit, which turn 4 uses. Turn 5 explicitly resumes saving the real decision; hypothetical abandonment is not a fact.");
            }
            "topic" => {
                let outside = seed(&s, "我的资源计划", "每周个人项目最多10小时，不能额外投入。");
                let collection = id();
                s.save_collection(&collection, "新产品", "访谈整理", None)
                    .unwrap();
                s.create_scoped_conversation(&topic, "New idea", Some(&collection))
                    .unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "新想法：先做访谈原话标注。另有个更正：我每周可投入的时间是8小时，之前记的10小时有误。请记好。",
                        &focus,
                    )
                    .await,
                );
                rubric.push_str("Add the new idea to the current collection. Update the outside resource plan to eight hours without changing its collection membership. Do not capture the current Memory or start a second organizer. Original plan ID: ");
                rubric.push_str(&outside.memory_id);
            }
            "history" => {
                let c = seed(
                    &s,
                    "木桥",
                    "去年我因每周只能拿出2小时而暂停木桥项目，并非缺预算。",
                );
                focus.push(c.memory_id);
                s.create_conversation(&topic, "回顾变化").unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "现在我每周可以投入8小时，决定恢复木桥项目。把这个变化记下来，保留当初为什么暂停的原因。",
                        &focus,
                    )
                    .await,
                );
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "木桥以前什么时候、为什么暂停？现在情况有什么变化？请回读当时的原始记录核对。",
                        &focus,
                    )
                    .await,
                );
                rubric.push_str("The Agent saves the decision to resume at eight hours per week, preserving last year's two-hour constraint rather than inventing a budget issue. It then reads historical/source evidence and answers with the old and current states and openable versions.");
            }
            "long_memory" => {
                let c = seed(
                    &s,
                    "很长的方案",
                    &format!(
                        "项目方案背景：{}最后的硬条件：只支持离线，预算上限4800元，不允许上传录音。",
                        "先研究访谈标注体验和资料整理方式。".repeat(500)
                    ),
                );
                focus.push(c.memory_id);
                s.create_conversation(&topic, "长材料").unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "这份方案有哪些必须遵守的硬条件？请把原文末尾也看完整。",
                        &focus,
                    )
                    .await,
                );
                rubric.push_str("Search and paginated reading must reach the ending and answer offline/CNY 4,800/no uploads accurately. Citations must cover those words and requests must stay within budget.");
            }
            "material_changes" => {
                let first = seed(&s, "访谈工具条件", "访谈工具必须离线处理，不允许上传录音。");
                let second = seed(&s, "试点安排", "试点每周最多安排6小时，预算上限3600元。");
                s.create_conversation(&topic, "增减指定材料").unwrap();
                focus.push(first.memory_id.clone());
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "请复述这次指定材料中的约束并引用原文。",
                        &focus,
                    )
                    .await,
                );
                focus.push(second.memory_id.clone());
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "我又指定了一份材料，请结合现在指定的两份材料复述限制并引用原文。",
                        &focus,
                    )
                    .await,
                );
                focus.remove(0);
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "继续看现在指定的材料：试点时间和预算限制分别是什么？",
                        &focus,
                    )
                    .await,
                );

                let collection = id();
                s.save_collection(&collection, "大型专题", "合成目录预算验收", None)
                    .unwrap();
                let mut directory = vec![];
                for n in 0..360 {
                    let title =
                        format!("{n:03} {}", "用于专题目录预算边界验证的合成材料".repeat(3));
                    let memory = seed(&s, &title, "合成资料：仅周六可安排访谈，其余时间不可安排。");
                    s.collect_record(
                        &collection,
                        &RecordKey {
                            kind: "memory".into(),
                            id: memory.memory_id.clone(),
                        },
                        true,
                    )
                    .unwrap();
                    directory.push(json!({"memory_id":memory.memory_id,"title":title}));
                }
                let full_directory_bytes = serde_json::to_vec(&directory).unwrap().len();
                assert!(
                    full_directory_bytes > 65_536,
                    "directory fixture must exceed the declared context budget"
                );
                let scoped = id();
                s.create_scoped_conversation(&scoped, "目录翻页", Some(&collection))
                    .unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &scoped,
                        "请再翻一页这个专题目录，任选下一页的一条记忆，读取它的正文后告诉我其中安排访谈的时间条件，并引用原文。",
                        &[],
                    )
                    .await,
                );
                dataset = json!({"first_memory_id":first.memory_id,"second_memory_id":second.memory_id,"collection_id":collection,"directory_count":directory.len(),"full_directory_bytes":full_directory_bytes});
                rubric.push_str("Focused materials change A to A+B to B within one conversation and initial requests reflect each change. Removing focus does not erase history. Answers accurately cite offline/no uploads/six hours/CNY 3,600; questions cause no fact writes. The full 360-item catalog exceeds budget and is marked incomplete, never treated as body evidence. The Agent pages forward and reads a body before citing the Saturday-only schedule. Deterministic wire tests check complete request budgets separately.");
            }
            "versioned_update" => {
                let c = seed(
                    &s,
                    "Kappa预算",
                    "Kappa试点预算5000元，只在本机处理录音，尚未上线。",
                );
                focus.push(c.memory_id.clone());
                s.create_conversation(&topic, "预算变化").unwrap();
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "把Kappa试点预算改为3800元，其他条件不变。请引用原记忆说明预算怎么变了，保存后再搜索Kappa预算确认新金额。",
                        &focus,
                    )
                    .await,
                );
                rubric.push_str("Read v1 and write v2 in this turn. The v1 citation remains openable with 5,000; v2 is immediately searchable with 3,800. Retain local processing and the not-launched state without invented facts. Deterministic lifecycle tests cover deleted sources.");
            }
            "compaction" => {
                s.create_conversation(&topic, "持续讨论").unwrap();
                for n in 0..12 {
                    let input = id();
                    let attempt = id();
                    let text = if n == 0 {
                        "固定条件：每周8小时、预算4800元；不上传录音；是否收费尚未决定。"
                            .to_string()
                    } else {
                        format!(
                            "讨论片段{n}：{}这里只讨论思路，尚未执行或决定。",
                            "可以比较访谈标注体验、操作步骤和后续验证问题。".repeat(30)
                        )
                    };
                    s.begin_agent_input(&input, &attempt, &topic, &text, &[], None)
                        .unwrap();
                    s.append_agent_text(
                        &input,
                        &attempt,
                        "这是备选思路，仍需结合最初限制判断，尚未形成新的决定。",
                    )
                    .unwrap();
                    s.finish_agent_input(&input, &attempt, false, &[]).unwrap();
                }
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "接着讨论，请先复述我们最初的数字限制、不能做的事和没有决定的事。",
                        &focus,
                    )
                    .await,
                );
                turns.push(
                    ask(
                        &s,
                        &config,
                        &topic,
                        "更正：预算是3800元，之前4800元有误。其他条件不变。现在的约束是什么？",
                        &focus,
                    )
                    .await,
                );
                rubric.push_str("Real compression must produce summary_through_seq > 0. Preserve eight hours/CNY 4,800/no uploads/undecided pricing; after correction use CNY 3,800 as current. Do not turn a conversation summary into Memory.");
            }
            _ => unreachable!(),
        }
        let memories = s
            .library(&LibraryQuery {
                limit: 100,
                ..Default::default()
            })
            .unwrap()
            .items
            .iter()
            .map(|m| s.library_detail(&m.key).unwrap())
            .collect::<Vec<_>>();
        let artifact = json!({"case":case,"rubric":rubric,"dataset":dataset,"turns":turns,"memories":memories,"conversation_context":s.agent_conversation_context(&topic).unwrap(),"semantic_review":"pending"});
        fs::write(
            output.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
        println!("{case}: saved synthetic evidence");
    }
}
