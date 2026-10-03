//! Real-model acceptance evidence using synthetic memories only.
//! Writes full inputs/results for human semantic review; no credentials copied.
use memivy_core::{memory::*, model::ModelConfig, models::ModelSettings};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use uuid::Uuid;
fn id() -> String {
    Uuid::new_v4().to_string()
}
fn read_config(path: &Path) -> ModelConfig {
    let metadata = fs::symlink_metadata(path).expect("private configuration metadata");
    assert!(
        metadata.is_file() && metadata.permissions().mode() & 0o777 == 0o600,
        "configuration must be a private regular 0600 file"
    );
    assert!(metadata.len() <= 256 * 1024, "configuration is too large");
    let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    if value.get("format_version").is_some() {
        assert_eq!(
            value["format_version"], 2,
            "current settings format required"
        );
        // Deserialization is read-only; ModelSettings::read can migrate a file.
        serde_json::from_value::<ModelSettings>(value)
            .unwrap()
            .llm_config()
            .unwrap()
    } else {
        ModelConfig::read(path).expect("current private configuration")
    }
}
fn state_snapshot(s: &MemoryStore) -> Value {
    let db = rusqlite::Connection::open(s.database_path()).unwrap();
    let rows = |sql: &str| -> Vec<Value> {
        let mut statement = db.prepare(sql).unwrap();
        statement
            .query_map([], |r| {
                let raw: String = r.get(0)?;
                Ok(serde_json::from_str(&raw).unwrap())
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    };
    json!({
        "collections": rows("SELECT json_object('id',id,'name',name,'description',description,'revision',revision,'archived',archived) FROM collections ORDER BY id"),
        "memberships": rows("SELECT json_object('collection_id',collection_id,'kind',kind,'record_id',record_id) FROM collection_entries ORDER BY collection_id,kind,record_id"),
        "memories": rows("SELECT json_object('id',id,'state',state,'current_version_id',current_version_id) FROM memories ORDER BY id"),
        "versions": rows("SELECT json_object('id',id,'memory_id',memory_id,'title',title,'body',body) FROM memory_versions ORDER BY id"),
        "receipt_count": db.query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, i64>(0)).unwrap()
    })
}
fn collection_dataset(s: &MemoryStore) -> Value {
    let pine = id();
    let review = id();
    let empty = id();
    s.save_collection(&pine, "松果计划", "离线访谈工具的准备事项", None)
        .unwrap();
    s.save_collection(&review, "本周复盘", "本周需要回顾的事项", None)
        .unwrap();
    s.save_collection(&empty, "稍后再看", "暂时没有成员", None)
        .unwrap();
    let budget = seed(s, "试点预算", "松果工具的试点预算是3600元，尚未上线。");
    let privacy = seed(s, "录音处理", "访谈录音仅在本机处理，禁止上传。");
    let hours = seed(s, "时间安排", "我每周能投入六小时，周六优先。");
    for (collection, memory) in [
        (&pine, &budget.memory_id),
        (&pine, &privacy.memory_id),
        (&review, &budget.memory_id),
        (&review, &hours.memory_id),
    ] {
        s.collect_record(
            collection,
            &RecordKey {
                kind: "memory".into(),
                id: memory.clone(),
            },
            true,
        )
        .unwrap();
    }
    json!({"pine":pine,"review":review,"empty":empty,"budget":budget.memory_id,"privacy":privacy.memory_id,"hours":hours.memory_id})
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
    let before = state_snapshot(s);
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
        "stream_updates":updates,"memory_updates":mutations,"elapsed_ms":started.elapsed().as_millis(),
        "business_state_before":before,"business_state_after":state_snapshot(s)})
}
#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (3..=4).contains(&args.len()),
        "discussion_probe PRIVATE_CONFIG FRESH_OUTPUT_DIR [CASE[,CASE...]]"
    );
    let mut config = read_config(Path::new(&args[1]));
    if let Ok(base_url) = std::env::var("MEMIVY_PROBE_BASE_URL") {
        let url = reqwest::Url::parse(&base_url).expect("valid probe proxy URL");
        assert!(
            url.scheme() == "http"
                && matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
                && url.username().is_empty()
                && url.password().is_none(),
            "probe proxy must be an HTTP loopback URL without credentials"
        );
        config.base_url = base_url;
    }
    let output = std::env::current_dir()
        .unwrap()
        .join(PathBuf::from(&args[2]));
    fs::create_dir(&output).expect("fresh output directory required");
    fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&json!({"model":config.model,"context_token_upper_bound":65536,"output_reserve":8192,"max_model_steps":12,"semantic_review":"pending","synthetic_only":true,"memory_write_contract":"parts_with_source_quotes"})).unwrap()).unwrap();
    for case in [
        "collection_global_list",
        "collection_named_lookup",
        "collection_membership",
        "collection_scoped_lookup",
        "collection_create",
        "collection_initial_membership",
        "collection_lifecycle",
        "collection_no_auto_assignment",
        "collection_scoped_search",
        "collection_recommendation",
        "general_question",
        "multi_query",
        "read_repair",
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
        let library = tempfile::tempdir().expect("isolated synthetic library");
        let s = MemoryStore::open(library.path()).unwrap();

        let topic = id();
        let mut focus = vec![];
        let mut turns = vec![];
        let mut rubric = String::new();
        let mut dataset = Value::Null;
        match case {
            "collection_global_list"
            | "collection_named_lookup"
            | "collection_membership"
            | "collection_scoped_lookup"
            | "collection_create"
            | "collection_initial_membership"
            | "collection_lifecycle"
            | "collection_no_auto_assignment"
            | "collection_scoped_search" => {
                dataset = collection_dataset(&s);
                let pine = dataset["pine"].as_str().unwrap();
                if matches!(
                    case,
                    "collection_scoped_lookup"
                        | "collection_no_auto_assignment"
                        | "collection_scoped_search"
                ) {
                    s.create_scoped_conversation(&topic, "Collection acceptance", Some(pine))
                        .unwrap();
                } else {
                    s.create_conversation(&topic, "Collection acceptance")
                        .unwrap();
                }
                match case {
                    "collection_global_list" => {
                        turns.push(
                            ask(
                                &s,
                                &config,
                                &topic,
                                "我现在有哪些专题？分别有几条记忆？",
                                &[],
                            )
                            .await,
                        );
                        rubric.push_str("List exactly three collections: 松果计划 with two active members, 本周复盘 with two, 稍后再看 with zero. Never present memory titles as collection names. No business mutation.");
                    }
                    "collection_named_lookup" => {
                        turns.push(ask(&s, &config, &topic, "帮我找一下松果计划这个专题，里面都记了什么？再看看稍后再看是不是空的。", &[]).await);
                        rubric.push_str("Resolve collection names to IDs, read two Pine members, summarize and cite budget 3600 and offline/no uploads. Distinguish the existing empty collection from an absent collection. No business mutation.");
                    }
                    "collection_membership" => {
                        focus.push(dataset["budget"].as_str().unwrap().to_owned());
                        turns.push(
                            ask(
                                &s,
                                &config,
                                &topic,
                                "这条试点预算记忆目前在哪几个专题里？",
                                &focus,
                            )
                            .await,
                        );
                        rubric.push_str("Query actual membership and report both 松果计划 and 本周复盘. No business mutation.");
                    }
                    "collection_scoped_lookup" => {
                        turns.push(
                            ask(
                                &s,
                                &config,
                                &topic,
                                "我们现在在哪个专题里？这里的两条记忆分别讲了什么？",
                                &[],
                            )
                            .await,
                        );
                        rubric.push_str("Use current real collection context, report 松果计划, read members and cite 3600 budget and local-only recordings. No business mutation.");
                    }
                    "collection_create" => {
                        focus = vec![
                            dataset["privacy"].as_str().unwrap().to_owned(),
                            dataset["hours"].as_str().unwrap().to_owned(),
                        ];
                        turns.push(ask(&s, &config, &topic, "新建一个叫执行准备的专题，说明写‘开始前要核对的条件’，把我附上的录音处理和时间安排这两条加入。", &focus).await);
                        rubric.push_str("One collection create operation atomically stores name/description and both existing members. Preserve Pine/Review memberships and all memory versions. Return a persistent undoable receipt.");
                    }
                    "collection_initial_membership" => {
                        turns.push(ask(&s, &config, &topic, "记住：松果工具先访谈三位独立开发者，还没有开始。把这条新记忆放进松果计划专题。", &[]).await);
                        rubric.push_str("Resolve Pine ID and revision, then one write_memory operation creates the sourced memory and initial membership atomically. Preserve not-started status, create only one new memory, and do not also use a separate membership write.");
                    }
                    "collection_lifecycle" => {
                        focus.push(dataset["budget"].as_str().unwrap().to_owned());
                        for input in [
                            "把这条试点预算从松果计划移到稍后再看，本周复盘里的归属保留。",
                            "把稍后再看改名为下次讨论，说明改成‘下次讨论的材料’。",
                            "把这条试点预算也加入松果计划。",
                            "把这条试点预算从下次讨论移出，其他专题归属保留。",
                            "撤销刚才从下次讨论移出的操作。",
                        ] {
                            turns.push(ask(&s, &config, &topic, input, &focus).await);
                        }
                        rubric.push_str("Atomic cross-collection move preserves Review; metadata update checks current revision; add/remove/undo preserve memory text and version count. Final budget membership is Pine, Review, and renamed 下次讨论. Each applied change is receipted; undo targets the immediately prior removal.");
                    }
                    "collection_no_auto_assignment" => {
                        turns.push(
                            ask(
                                &s,
                                &config,
                                &topic,
                                "记住一个独立想法：周末想试试做陶艺，目前只是考虑。",
                                &[],
                            )
                            .await,
                        );
                        turns.push(
                            ask(
                                &s,
                                &config,
                                &topic,
                                "你觉得哪几条已有记忆适合放进稍后再看？先只给建议。",
                                &[],
                            )
                            .await,
                        );
                        rubric.push_str("Save the tentative pottery idea without any collection membership despite scoped conversation. A recommendation-only follow-up changes no memberships, memories, or versions.");
                    }
                    "collection_scoped_search" => {
                        turns.push(ask(&s, &config, &topic, "先在松果计划专题里查一下预算，再去整个资料库查我每周能投入多久。只查已有记忆。", &[]).await);
                        rubric.push_str("Use collection_id for Pine budget search, use null/global scope for time search, read evidence and cite 3600 and six hours. No business mutation; scoped conversation must not hide global constraints.");
                    }
                    _ => unreachable!(),
                }
            }
            "collection_recommendation" => {
                let memory = seed(
                    &s,
                    "Shanghai preparation",
                    "For the next Shanghai trip, collecting material beforehand remains optional and undecided.",
                );
                s.create_conversation(&topic, "Independent recommendations")
                    .unwrap();
                let travel = id();
                let cooking = id();
                s.save_collection(&travel, "Travel planning", "Trips and preparation", None)
                    .unwrap();
                s.save_collection(&cooking, "Cooking", "Recipes and groceries", None)
                    .unwrap();
                let current = s.memory(&memory.memory_id).unwrap().current;
                let started = std::time::Instant::now();
                let result = s
                    .recommend_collections(&config, &memory.memory_id, &current.id)
                    .await;
                let key = RecordKey {
                    kind: "memory".into(),
                    id: memory.memory_id.clone(),
                };
                let before = s.record_navigation(&key).unwrap();
                let suggestions = result.as_ref().ok().cloned().unwrap_or_default();
                if let Some(choice) = suggestions.first() {
                    s.accept_collection_recommendation(
                        &memory.memory_id,
                        &current.id,
                        &choice.collection.id,
                        choice.collection.revision,
                    )
                    .unwrap();
                }
                dataset = json!({"suggestions":result.map_err(|e|format!("{e:?}")),"before_confirmation":before,"after_confirmation":s.record_navigation(&key).unwrap(),"elapsed_ms":started.elapsed().as_millis()});
                rubric.push_str("One direct structured model request, no Agent. Suggest the existing travel collection; no membership before explicit confirmation. The memory body and version remain unchanged.");
            }
            "general_question" => {
                seed(
                    &s,
                    "Unrelated personal budget",
                    "My travel budget is 5000 yuan.",
                );
                s.create_conversation(&topic, "General writing").unwrap();
                turns.push(ask(&s, &config, &topic, "请把这句话改得简洁一点：由于今天下雨，因此我们决定取消原定在室外举行的活动。只改写，不保存。", &[]).await);
                rubric.push_str("Respond directly using the supplied sentence. No memory search or mutation is needed. Preserve the cancellation decision without inventing facts.");
            }
            "multi_query" => {
                seed(
                    &s,
                    "Skyline budget",
                    "Skyline project budget is capped at 2000 euros.",
                );
                seed(
                    &s,
                    "Skyline privacy",
                    "Skyline interview recordings must remain offline and cannot be uploaded.",
                );
                s.create_conversation(&topic, "Project constraints")
                    .unwrap();
                turns.push(ask(&s, &config, &topic, "Please find the Skyline budget and recording privacy constraints. Use one multi-query search with a statement-style query for each aspect, then cite the actual saved evidence. Do not save this question.", &[]).await);
                rubric.push_str("The raw search arguments use at least two complementary queries with natural text and concise keywords. Both saved constraints are found and cited. No new memory is created.");
            }
            "read_repair" => {
                seed(
                    &s,
                    "上海准备",
                    "我下次去上海，可能需要提前收集资料，但还没有决定。",
                );
                seed(
                    &s,
                    "上海行程补充",
                    "下次同一趟上海行程，提前收集资料仍是待定选项，不是必须。",
                );
                s.create_conversation(&topic, "上海行程").unwrap();
                let turn = ask(&s, &config, &topic, "查一下关于下次上海行程的记忆，告诉我哪些已定、哪些待定。如果确实重复，可以合并；不要把可能写成确定。", &[]).await;
                let input = turn["input_id"].as_str().unwrap().to_owned();
                turns.push(turn);
                if !s.agent_input_receipts(&input).unwrap().is_empty() {
                    let undo = s.undo_agent_input(&id(), &input).unwrap();
                    let next = id();
                    s.create_conversation(&next, "再次回顾").unwrap();
                    dataset = json!({"undo":undo,"new_conversation":next});
                    turns.push(
                        ask(
                            &s,
                            &config,
                            &next,
                            "回顾一下我下次去上海的准备事项，哪些是确定的？",
                            &[],
                        )
                        .await,
                    );
                }
                rubric.push_str("Read complete evidence before any merge; preserve uncertainty and the same-trip scope. If merged, both records have an atomic receipt and inherited originals. After undo, retrieval in a new conversation reports the reversal and the Agent does not automatically repeat it.");
            }
            "record_and_question" => {
                s.create_conversation(&topic, "随时表达").unwrap();
                for text in [
                    "我有个想法：做一个帮助个人整理访谈的小工具，目前只是考虑。",
                    "我刚才的想法是什么？",
                    "我决定先访谈3位独立开发者，还没有开始。你觉得可以怎么安排？",
                ] {
                    turns.push(ask(&s, &config, &topic, text, &focus).await);
                }
                rubric.push_str("Turn 1 saves only a tentative idea with a brief acknowledgment and follow-up suggestions. Turn 2 recalls without new facts. Turn 3 saves the decision to interview three people as not yet executed and gives two or three suggestions. No mandatory confirmation.");
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
                rubric.push_str("Both focused-material and collection entry points let the Agent retrieve global 10-hour and CNY 5,000 constraints when planning, using its own search calls. Answers must use and cite them without deciding for the user. The later offline/no-upload constraint must be saved and applied.");
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
                rubric.push_str("Save the new idea without assigning collection membership automatically. Update the outside resource plan to eight hours without changing its collection membership. Preserve each original source. Original plan ID: ");
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
                    s.finish_agent_input(&input, &attempt, &[]).unwrap();
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
        let artifact = json!({"case":case,"rubric":rubric,"dataset":dataset,"turns":turns,"memories":memories,"conversation_context":s.agent_conversation_context(dataset["new_conversation"].as_str().unwrap_or(&topic)).unwrap(),"semantic_review":"pending"});
        fs::write(
            output.join(format!("{case}.json")),
            serde_json::to_vec_pretty(&artifact).unwrap(),
        )
        .unwrap();
        println!("{case}: saved synthetic evidence");
    }
}
