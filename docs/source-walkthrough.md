# 第一次读 Memivy 源码

这份导读按 2026-09-13 工作区里的源码整理。目标很简单：读完后，你知道每块代码在干什么；想改一个功能时，大致知道去哪里找。它不是功能验收报告，也不要求你先学完 React 和 Rust。

## 1. 先记住这张地图

Memivy 是一个运行在本机的桌面应用。你看到的界面用 React 写，桌面能力由 Tauri 接上，记忆和 AI 的主要业务逻辑用 Rust 写。

```text
你在主窗口或快捷窗口输入
          ↓
src/workspace/             显示界面、保存输入草稿、响应点击
          ↓ 命令调用
src-tauri/src/             接收请求、管理窗口、启动后台工作
          ↓ 调用 Rust 方法
crates/memivy-core/        记忆规则、搜索、对话、版本、撤销
          ├── SQLite      保存本地数据
          ├── 模型接口    请求用户配置的大模型
          └── 本地助手进程  计算向量、把声音转成文字
          ↑
事件通知界面更新
```

这里的“命令调用”可以理解为：界面说“请保存这段文字”，Rust 接到命令后执行，再把结果交回来。代码里叫 `invoke`。后台回答还没结束时，也可以主动通知界面，这叫 event。

另有一个入口 `memivy-mcp`，让其他 Agent 保存或搜索记忆。它也使用同一个 Rust 核心，但不需要经过 React 界面。

## 2. 打开项目，先看哪些目录

| 位置 | 人话解释 | 第一遍怎么读 |
|---|---|---|
| [src/main.tsx](../src/main.tsx) | 界面总入口，决定显示哪个窗口 | 全文很短，直接读完 |
| [src/workspace/](../src/workspace/) | 现在正式应用的界面 | 按下面的功能路线读 |
| [src-tauri/src/](../src-tauri/src/) | 桌面应用的宿主：窗口、命令和后台任务 | 先找具体命令，别从头硬啃 |
| [crates/memivy-core/src/memory/](../crates/memivy-core/src/memory/) | 正式记忆业务的核心 | 最值得花时间理解 |
| [crates/memivy-core/src/model/](../crates/memivy-core/src/model/) | 大模型请求、流式输出、工具调用协议 | 理解 Agent 后再读 |
| [crates/memivy-embedding/](../crates/memivy-embedding/) | 本地计算文本向量的小程序 | 第二遍读 |
| [crates/memivy-speech/](../crates/memivy-speech/) | 本地语音转文字的小程序 | 第二遍读 |
| [crates/memivy-mcp/](../crates/memivy-mcp/) | 给外部 Agent 用的本地入口 | 主流程读完后看 |
| [migrations/memory/](../migrations/memory/) | 数据库建表和后续结构调整 | 遇到数据名再查 |
| [tests/](../tests/) | 界面及前端逻辑测试 | 当作行为示例看 |
| [crates/memivy-core/tests/](../crates/memivy-core/tests/) | Rust 核心测试 | 想确认业务规则时看 |
| [scripts/](../scripts/) | 启动、打包、验证和资源生成脚本 | 用到再读 |
| [src/i18n/](../src/i18n/) | 语言选择、文案和日期等显示格式 | 改中文或英文时看 |
| [design-demo/](../design-demo/) | 早期设计演示与品牌资源 | 不把它当正式程序入口 |

`package.json` 是前端依赖和运行命令清单；根目录的 `Cargo.toml` 是 Rust 子项目清单。`crate` 就是 Rust 项目里的一个包。

正式应用现在只有主窗口和桌面快捷窗口两个界面入口，使用同一个 `memory::MemoryStore` 核心。

也别把技术方案中的历史描述当作当前函数地图：项目经历过几轮演进，阅读时以实际入口和调用关系为准。

## 3. 第一站：应用怎样启动

先读 [src-tauri/src/main.rs](../src-tauri/src/main.rs)。它直接启动 `workspace::run`，没有其他应用分支。

接着在 [src-tauri/src/workspace.rs](../src-tauri/src/workspace.rs) 找 `pub fn run`。这里建立 Tauri 应用，打开正式资料库，恢复中断任务的状态，准备桌面和语音功能，启动整理、向量索引等后台工作，并注册界面可以调用的命令。

再读 [src/main.tsx](../src/main.tsx)：普通窗口显示 `App`；URL 带 `window=capture` 时显示 `Desktop`。

所以你平时看到的主窗口和桌面快捷入口，是两个界面入口，共用后面的业务核心。

实际启动正式应用的命令是 `npm run dev:app`。`npm run dev` 只有浏览器预览；`api.ts` 为它提供了预览数据，不能用浏览器里的显示结果证明原生存储或录音已经工作。

## 4. 第二站：屏幕上的东西对应哪些文件

先打开 [App.tsx](../src/workspace/App.tsx)，看 import 和最后的 JSX，暂时略过中间的大量 `useEffect`。JSX 就是“这个页面由哪些组件拼起来”的描述。

`App` 管的是全局选择：当前页面、选中哪条记忆、打开哪段讨论、是否显示设置，以及快捷窗口交接过来的内容。具体编辑器和对话不全写在这里。

| 屏幕上的部分 | 主要文件 | 它负责什么 |
|---|---|---|
| 左侧导航 | `WorkspaceSidebar.tsx` | 记忆、讨论、专题等入口 |
| 顶部统一输入 | `WorkspaceQuery.tsx`、`WorkspaceTopBar.tsx` | 输入区展开、收起和草稿预览 |
| 真正的输入框 | `CaptureForm.tsx` | 文字、语音、选取记忆材料、提交 |
| 记忆列表 | `MemoryList.tsx` | 请求列表并显示条目 |
| 单篇记忆 | `MemoryDetail.tsx` | 阅读、编辑、历史和各项操作 |
| 聊天内容 | `Discussion.tsx` | 消息、引用、重试、取消和继续输入 |
| 桌面快捷窗口 | `Desktop.tsx` | 快捷入口的显示状态及主窗口交接 |
| 设置 | `Settings.tsx` 和各个 `*Settings.tsx` | 把设置按用途分成小页面 |

界面公共积木在 `components.tsx`、`IconButton.tsx`、`Select.tsx`、`Toast.tsx`，图标等基础元素在 `src/ui.tsx`。CSS 文件管外观；第一遍先了解行为，不必逐个研究颜色和间距。

读 React 时只需要先认三个词：`useState` 保存当前界面状态；`useEffect` 在条件变化时做事；`useRef` 保留一个值或拿到输入框等页面元素。先看“它存了什么、变化后做什么”，不用一开始钻语法细节。

## 5. 第三站：追踪一次“我发了一句话”

假设你输入：“我准备下个月开始学 Rust，每周拿出三小时。”

### 第一步：输入框先管好草稿

在 [CaptureForm.tsx](../src/workspace/CaptureForm.tsx) 找 `send()`。它先等语音收尾，再把草稿写完，然后交给 `onSubmit`。宿主确认接收后，才清理这次草稿。

这里没有直接调用大模型。输入框只负责把完整的输入交出去。

相关的 [useDraft.ts](../src/workspace/useDraft.ts) 和 [draftQueue.ts](../src/workspace/draftQueue.ts) 负责草稿读写与排队，避免连续输入、切换页面或关窗口时丢掉未完成内容。`request_id` 用来识别同一次提交，重试时不应变成重复保存。

### 第二步：界面向 Rust 发命令

`App.tsx` 的初次提交和 `Discussion.tsx` 的后续提交都会调用 `discussion_submit`。快捷窗口也走这个命令，只额外带上快捷入口信息。

路径是：

```text
CaptureForm.send()
  → App / Discussion / Desktop 的提交处理
  → api.ts 的 call("discussion_submit", ...)
  → resources.ts
  → Tauri invoke
  → workspace.rs 的 discussion_submit
```

[api.ts](../src/workspace/api.ts) 同时放了前端数据类型；[resources.ts](../src/workspace/resources.ts) 管请求缓存、变更通知和刷新。看到 `call("某个名字")` 时，到 Rust 里搜同名函数，通常就能接着往下读。

### 第三步：先存输入，再启动 AI

在 [workspace.rs](../src-tauri/src/workspace.rs) 连着看 `discussion_submit` → `persist_discussion_input` → `launch_discussion`。

前两个函数找到或建立会话，调用核心的 `begin_agent_input` 保存本轮输入和执行状态。之后 `launch_discussion` 才检查模型配置并启动后台任务。

这解释了为什么模型连接失败时，用户刚才说的话仍然有本地记录。**聊天原文已经保存，不等于已经生成了一条长期记忆。** 两者是分开的数据。

### 第四步：准备模型需要知道的事情

打开核心的 [discussion.rs](../crates/memivy-core/src/memory/discussion.rs)，找 `run_discussion`。

它准备本轮原话、近期消息、较早会话的压缩摘要、相关记忆和当前是否允许维护记忆等信息。第一次让模型回答前，会主动准备记忆上下文；不是每次都等模型自己想起来搜索。

选中的记忆和专题为这一轮提供重点，但当前正式对话仍可全局搜索。不要从旧方案推断它只能读当前专题。

### 第五步：模型说话，也可以请求工具

在 [agent.rs](../crates/memivy-core/src/memory/agent.rs) 找 `run_memory_agent`。循环大致是：

```text
把上下文发给模型
  → 模型返回文字：保存新增文字，通知界面
  → 模型请求工具：检查参数，执行，返回真实结果
  → 把工具结果交给模型继续
  → 回答完成，或因取消、失败、预算限制而停止
```

“工具”就是我们允许模型申请调用的函数，例如搜索记忆、读取正文、写入记忆。模型不能自己拿着数据库随便改。

`discussion.rs` 管本轮对话怎么组织；`agent.rs` 管共享循环、工具定义和读取规则；`model.rs`、`model/tools.rs`、`model/stream.rs` 管向模型服务发送请求和解析回复。产品规则和网络协议分在不同地方。

### 第六步：如果要记住，经过正式保存规则

[agent_mutations.rs](../crates/memivy-core/src/memory/agent_mutations.rs) 的 `apply_agent_memory` 负责实际记忆写入。配合 `agent.rs` 的写入参数、来源检查，以及 `agent_state.rs` 的执行状态检查，它要确认来源、版本和本轮执行仍然有效，再保存变更和回执。

所以这句话是否值得记住、怎么表达，由模型作判断；保存是否合法、是否冲突、有没有来源和撤销记录，由代码把关。看到这些检查，不应理解成代码能保证模型每次都理解正确。

### 第七步：结果回到屏幕

宿主发送 `discussion-updated`，`Discussion.tsx` 读取更新后的消息。记忆发生变化时，也会通知相关列表和详情刷新。执行状态和消息保存在本地，界面不必一直握着整个后台任务。

## 6. 第四站：记忆到底怎么存

先看 [types.rs](../crates/memivy-core/src/memory/types.rs)，了解名字，再看 SQL。直接从建表语句开始容易迷路。

| 名字 | 可以怎么理解 |
|---|---|
| `RawCapture` | 保存下来的原始文字和来源 |
| `Memory` | 一条长期记忆的身份，指向当前版本 |
| `Version` | 某一次保存时的标题、正文等快照 |
| `Receipt` | 一次操作的回执，说明改了什么 |
| `SourceRef` / `Evidence` | 指向具体原话或版本的来源，以及读取出来的证据 |
| `Conversation` / `Message` | 聊天及其中的消息，与长期记忆分开 |
| `WorkspaceDraft` | 还没正式提交的编辑或输入草稿，定义在 `library.rs` |
| `AgentExecution` / `AgentOperation` | 本轮 AI 工作及工具操作的状态，定义在 `agent_state.rs` |

一条记忆可以有多个版本，也可以关联多段来源。聊天消息本身不会因此全部成为普通记忆搜索的结果。Agent 成功写入时，会归档实际用到的用户来源。

[db.rs](../crates/memivy-core/src/memory/db.rs) 负责打开数据库、连接和结构版本；[records.rs](../crates/memivy-core/src/memory/records.rs) 管记录、版本、原话、回执、撤销和回收站；[library.rs](../crates/memivy-core/src/memory/library.rs) 把这些组合成界面需要的列表、详情和草稿接口。

很多文件都有 `impl MemoryStore`，意思是给同一个核心对象增加不同方法。它们不是多个独立存储系统。

SQL 在 [migrations/memory/](../migrations/memory/)：先看 `001_records.sql` 认识基础关系，再按需看 `003_workspace.sql`、`016_agent_sessions.sql`。这些文件记录结构如何累积变化，编号不代表用户操作顺序。

## 7. 第五站：追踪一次手动编辑和撤销

打开 [MemoryDetail.tsx](../src/workspace/MemoryDetail.tsx)，找 `Editor` 里的 `save()`。这条路比 AI 流程短：

```text
MarkdownEditor 编辑内容
  → useDraft 保存草稿
  → library_edit 命令
  → 核心 library.rs / records.rs
  → 检查当前版本，保存新版本和回执
  → 界面更新
```

重点看 `expected_version`：它表示“我是在这个版本上开始改的”。如果你编辑时 AI 或另一个窗口已经更新了记忆，就不能悄悄覆盖。这类比较后再写入的检查，有时叫 CAS；记成“先核对，再保存”就够了。

撤销也要检查后续变化。它不能为了撤销一次旧操作，把你后来写的新内容一起盖掉。普通记录操作看 `records.rs`；按一轮 Agent 操作整体撤销看 `agent_mutations.rs`。界面的回执和变化历史在 `OrganizationReceipt.tsx`、`MemoryChanges.tsx`。

编辑器相关文件可以成组读：

- `MarkdownEditor.tsx`：接入 Milkdown 编辑器，连接文档和编辑状态。
- `FormattingToolbar.tsx`、`editorFormatting.ts`、`editorExtensions.ts`：选中文字后的格式操作与编辑器扩展。
- `Markdown.tsx`：阅读时显示 Markdown；`remarkUnderline.ts` 处理下划线扩展。
- `MemoryCleanup.tsx`、`cleanupDiff.ts`，以及宿主和核心各自的 `cleanup.rs`：单篇整理、预览差异、接受修改。

“手动编辑”和“让 AI 整理”最终都需要守住版本与草稿，但前者不需要请求模型。

## 8. 第六站：搜索、向量和自动整理

这几块容易被统称为 AI，其实工作不同。

| 模块 | 它具体做什么 |
|---|---|
| `memory/search.rs` | 正式统一搜索入口，先看 `SearchRequest` 和 `MemoryStore::search` |
| `memory/retrieval.rs` | 搜索使用的底层匹配和排序辅助逻辑 |
| `memory/embedding.rs` | 记忆分块的索引任务及向量检索接入 |
| core 的 `embedding/` | 分块、模型缓存和本地编码进程通信 |
| `memivy-embedding/src/main.rs` | 实际加载本地模型，把文字变成向量 |
| `memory/related.rs` | 找单篇下面的相关记忆 |
| `memory/organization.rs` | 捕获后的后台整理任务及恢复、重试等流程 |
| `memory/collection_recommendations.rs` | 专题候选推荐 |
| `memory/navigation.rs` | 专题、成员关系、置顶等本地操作 |

关键词搜索找文字匹配；向量让系统也能比较意思接近的内容。向量索引是搜索的辅助数据，正文和版本仍由 SQLite 管。

现在正式聊天的记忆维护从 `discussion.rs` 进入。`organization.rs` 仍用于捕获后的后台整理；不要只因为文件名叫“整理”，就把所有聊天写入都归到这里。宿主的 `start_organizer` 可以帮你确认这条后台路线。

普通搜索围绕当前记忆；原话和旧版本用于来源、历史及按 ID 读取证据。想研究搜索的具体范围，从 `SearchRequest` 和调用处传入的参数读起。

## 9. 第七站：桌面入口、语音和模型设置

桌面能力主要看 [desktop.rs](../src-tauri/src/desktop.rs)：菜单栏、全局快捷键、快捷窗口状态和主窗口交接。前端搭配 `Desktop.tsx`、`desktopApi.ts`。更底层的 macOS 面板行为在 `capture_panel.rs`，退出收尾在 `desktop/termination.rs`。

语音可以沿着这条线读：

```text
VoiceInput.tsx / useVoice.ts / voiceSession.ts
  → src-tauri/src/voice/mod.rs 管录音会话
  → audio.rs 采集和处理音频
  → engine.rs 管本地转写进程
  → memivy-speech 把音频转成文字
  → 文字回到同一份输入草稿
  → 用户提交，进入前面的 discussion_submit
```

这条线描述本地语音路径。模型设置也区分本地或连接服务的来源，不能把整个设置系统理解成只支持一个写死的本地模型。

停止录音需要等剩余转写收尾，转出来的文字仍走共享输入和草稿流程。语音不是另一套记忆保存系统。

模型设置跨三处：前端 `ModelOverview.tsx`、`ModelCapability.tsx` 等显示配置；宿主 `models.rs` 接收配置和测试命令；core 的 `models.rs`、`model.rs` 管配置结构、连接和调用。core 的 `speech.rs`、`embedding/cache.rs` 管本地模型资源；权重使用统一系统缓存，与资料库里的正文分开。

## 10. 第八站：备份、MCP 和其他收尾模块

| 功能 | 从哪里进入 | 往下看哪里 |
|---|---|---|
| 备份恢复 | `BackupSettings.tsx` | 宿主 `backup.rs` → core `memory/transfer.rs` |
| 单篇 Markdown 导出 | 记忆详情的操作菜单 | 宿主 `memory_export` → `transfer.rs` |
| MCP 开关和配置 | `McpSettings.tsx` | 宿主 `mcp.rs` |
| 外部 Agent 真正调用 MCP | `crates/memivy-mcp/src/main.rs` | core `memory/mcp.rs` 和 `MemoryStore` |
| 跨窗口数据刷新 | `resources.ts` | 宿主 `library_changes` → core `memory/changes.rs` |
| 中文、英文与错误显示 | `src/i18n/` | 宿主 `i18n.rs`、`errors.rs` |
| 配置文件路径保护 | 宿主 `storage.rs` | 看路径校验规则；它不是主数据库模块 |

备份恢复需要协调正在运行的应用，所以宿主管暂停、窗口与重启相关事情，核心管备份数据与恢复流程。

MCP 对外只有 `memory_capture` 和 `memory_search`，使用 stdio，也就是调用方通过进程的标准输入输出通信。它没有把整个聊天 Agent 暴露成第三个工具。应用自己对大模型开放的内部工具，与对外 MCP 工具，是两组不同接口。

## 11. 最后读测试：看具体例子，比读抽象解释更快

不必一开始把全套测试都运行起来。先挑一个测试，读准备了什么数据、执行什么操作、最后要求什么结果。

| 想确认的事情 | 可以先读 |
|---|---|
| 保存、版本、撤销、回收站 | `crates/memivy-core/tests/memory_data.rs` |
| 列表和草稿 | `crates/memivy-core/tests/memory_library.rs` |
| 当前记忆与归档的关系 | `crates/memivy-core/tests/current_memory.rs` |
| Agent 状态与操作 | `crates/memivy-core/tests/agent_execution.rs`、`agent_workflows.rs` |
| 手动保存和 Agent 历史 | `crates/memivy-core/tests/agent_manual_history.rs` |
| 模型流式输出 | `crates/memivy-core/tests/model_stream.rs` |
| 草稿排队、统一输入 | `tests/draft-queue.test.mjs`、`tests/unified-input.test.mjs` |
| Markdown 与格式操作 | `tests/editor-formatting.test.mjs`、`tests/markdown-render.test.mjs` |
| MCP 边界 | `crates/memivy-core/tests/memory_mcp.rs` |

有些 Rust 文件底部还有 `#[cfg(test)]` 测试模块。大文件后半段可能已经是测试，不必全部当作生产主流程读。前端测试中也有源码结构检查；它们通过不等于真实窗口、输入法或麦克风已经验证。

当前前端测试命令在 `package.json` 中是 `npm run test:ui`，使用 Node 测试运行器。Rust 核心可用 `cargo test -p memivy-core --offline`；`--offline` 要求本机已有依赖。原生行为还需要实际打开应用检查。

## 12. 真正开始读时，就按这六次来

1. **看懂应用的骨架。** 读两个 `main`、`workspace::run`、`App`。能指出主窗口、快捷窗口和核心各在哪里，就够了。
2. **跟一条输入走到底。** 读 `CaptureForm.send`、`discussion_submit`、`begin_agent_input`、`run_discussion`。重点回答“原话在哪一步保存”。
3. **看懂记忆与聊天的区别。** 读 `types.rs`，再看 `records.rs`、`conversations.rs`。画出一条记忆、两个版本和一段来源的关系。
4. **看懂 AI 怎样做事。** 读 `run_memory_agent`、工具定义、`apply_agent_memory`。重点回答“模型提出写入后，代码检查什么”。
5. **看懂自己的内容怎样被保护。** 跟一次编辑、版本冲突、撤销、取消和重试。遇到不明白的分支，找对应测试。
6. **按兴趣补周边。** 搜索与向量、语音、桌面窗口、备份和 MCP，每次挑一条线。

读一个函数时，只问四件事：**谁调用它？收到什么？改了什么？结果交给谁？** 先把这四件事串起来，再回来理解细节，会轻松很多。
