# Memivy 技术栈与架构方案

状态：沿用已验证技术基础；2026-09-06 阶段 2 核心数据层已实现，验证记录见开发计划；原生界面仍使用隔离样机
依据：[PRD.md](PRD.md)，技术选型研究日期 2026-09-04，产品方向同步日期 2026-09-06

原阶段 1 于 2026-09-06 验收通过。此后用户另行授权在现有样机中实现悬浮助手和最小记忆讨论闭环，作为阶段 1 第二版；原验收结论保留。模型配置与原生焦点问题修复后，用户于同日确认所给检查清单全部通过，第二版完成用户手工验收。实际测试范围、后续实施顺序与授权状态见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。

阶段 1 第二版使用同一 NSPanel 原地调整大小，提供收起图标、输入和回答状态；React 复用一套输入与讨论组件，通过 host 传递话题、输入模式、草稿和所选记忆。`002_interaction.sql` 为隔离样机增加话题、讨论轮次、确认回执和撤销标记，不等同于正式 memory/version 模型。普通会话不进入 FTS；结论只支持用户核对后存为新的不可改写 capture，撤销后退出可检索记忆，保留确认原文和出处。一次提问至多两次模型请求、4 个检索词、8 条原话证据与4轮节选历史；完成状态校验阻止取消后的迟到回答写回。未实现正式 AI 自动整理、续写旧记忆或完整版本编辑。

悬浮拖动实验覆盖整个图标及展开后的标题区；WebView 使用 pointer capture 区分点击与拖动，Rust 读取系统鼠标和窗口的物理坐标来移动原生窗口，不依赖跨 WebView IPC 后的旧鼠标事件。拖动不触发展开，短点击才进入输入状态；实际手感和跨屏行为仍按原生验收记录核对。

阶段 2 的正式接口为 `memivy_core::memory::MemoryStore`，位于 `crates/memivy-core/src/memory/`。默认数据目录为 `~/Library/Application Support/com.memivy.app/`，事实库为 `memivy.db`；通过显式绝对目录进行测试。原生样机和现有 MCP 仍调用独立的 Phase 1 Store，未迁移样机数据、未切换用户正在试用的界面。正式数据层没有模型调用，后续 UI 与 MCP 接入这套接口。

## 1. 结论

Memivy 的“记一下、问一问、接着想”共用一套本地数据和模型调用基础，继续采用：

| 层 | 选择 |
|---|---|
| macOS 应用壳 | Tauri 2 |
| 界面 | React + TypeScript + Vite |
| 业务核心 | Rust |
| 本地数据 | SQLite + FTS5，由 `rusqlite` 直接管理 |
| 异步任务 | Tokio；任务状态持久化在 SQLite，不引入消息队列 |
| 模型调用 | `reqwest` + `serde`，直接调用一个 OpenAI-compatible 端点 |
| MCP | 官方 Rust SDK `rmcp`，本机使用 stdio |
| 原生补充 | 必要时使用 Swift 补齐 macOS 系统能力 |
| 测试与发布 | Cargo test、Vitest、WebdriverIO、Mac 安装冒烟、GitHub Actions |

当前产品只支持 macOS。Windows、iOS、Android 暂缓，不进入本轮构建、测试和发布；Linux 不支持。

原阶段 1 第一版：`src-tauri/` 为原生 host，`src/` 为复用 Demo 样式的最小 React 界面，`crates/memivy-core/` 为共享原话保存、查询和独立模型协议探测，`crates/memivy-mcp/` 为仅含 `memory_capture` 的 stdio 程序。第一版迁移只有不可改写的原话表与 FTS 索引；未实现正式 memory/version、撤销和 AI 整理。模型探测使用 `reqwest` 的同一 `/chat/completions` 路径，验证完整 JSON schema 结果，拒绝截断、异常状态和超过 64 KB 的响应；模型请求模块没有数据库访问能力。当前实测 SQLite 为 bundled 3.53.2，完整依赖版本以 lockfile 为准。第二版在这一基础上增加上述实验数据与交互，不把试验表视为正式产品数据模型。

2026-09-06 原生验证补充：普通 NSWindow 在访达全屏下未取得可输入焦点，仅增加 fullScreenAuxiliary 仍不足；阶段 1 捕捉窗改由 `src-tauri/src/capture_panel.rs` 配置非激活 NSPanel，保留原 WebView、IPC 与主窗口 Dock 行为。使用 [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel/tree/c9ec2130422200f0863b23dfdad02b133a529b07) 2.1.0，固定提交 `c9ec2130422200f0863b23dfdad02b133a529b07`；它启用 Tauri 的 macos-private-api feature，当前仅作为未签名技术样机验证，不代表已完成分发审核。原生面板操作限定在主线程；输入就绪按 DOM 输入框焦点加 NSPanel key-window 状态判断，非激活面板无需把整个应用设为前台。

技术实现只为 Mac MVP 服务：菜单栏常驻、全局快捷入口、本地 SQLite、Memory Agent、记忆问答与讨论、MCP 都由本机应用与共享核心承载。模型可位于本地或用户配置的远程端点。新产品方向不要求更换应用壳、增加云服务或多 Agent 框架。

## 2. 候选方案与取舍

| 方案 | 优点 | 对当前 PRD 的主要代价 | 结论 |
|---|---|---|---|
| Tauri 2 + React + Rust | Web 编辑体验好；Rust 可统一 SQLite、AI 与 MCP；菜单栏和快捷键能力直接 | macOS WebView、焦点和快捷键仍需真机验证 | 选择 |
| Flutter + Rust | UI 一致、移动端成熟 | Rust FFI 和 Dart 两层；菜单栏与快捷键依赖额外插件 | 暂不选 |
| Electron + React | TypeScript 生态成熟，桌面资料多 | 自带 Chromium；SQLite、AI 和 MCP 容易继续堆在 Node 层 | 不选 |
| SwiftUI + Rust | macOS 原生体验最好 | UI 与 Rust 之间需要维护 FFI；Markdown 编辑和 MCP 开发被拆成两套技术 | 不选 |

### 2.1 为什么主方案是 Tauri，而不是 Flutter

Tauri 2 采用“Web 前端 + Rust 应用逻辑 + 必要时原生补充”的结构。[Tauri 2](https://v2.tauri.app/) 这正好对应 Memivy 的技术重心：

1. 主产品不是复杂移动社交应用，而是桌面优先的常驻记忆工具；
2. SQLite、AI 整理、问答与讨论、版本历史、导出和 MCP 都适合放在一个可测试的 Rust 核心里；
3. Tauri 原生提供桌面托盘，并有桌面全局快捷键与自启动能力。[系统托盘](https://v2.tauri.app/learn/system-tray/) · [插件能力表](https://v2.tauri.app/plugin/)
4. 记忆页、来源时间线、搜索和 Markdown 编辑更适合复用成熟的 Web UI 与编辑器生态；
5. 不需要 Electron 自带一整套 Chromium，也不必为了原生界面引入 SwiftUI 与 Rust FFI 两套开发面。

Flutter 的主要优势是移动端成熟度和统一渲染，但这些不是当前 Mac MVP 的核心收益。Memivy 所需的菜单栏和系统级快捷键还主要依赖第三方桌面插件；选择 Rust 后则可直接使用 MCP 官方 Rust SDK。[tray_manager](https://pub.dev/packages/tray_manager) · [hotkey_manager](https://pub.dev/documentation/hotkey_manager/latest/) · [rmcp](https://github.com/modelcontextprotocol/rust-sdk)

如果未来决定开发移动端，再根据当时需求重新比较 Tauri 与 Flutter；当前不为这个可能性增加代码复杂度。

## 3. 技术选型参考（2026-09-04 调研记录）

| 项目 | 调研时的技术路线 | 对 Memivy 的启示 |
|---|---|---|
| [AppFlowy](https://github.com/AppFlowy-IO/AppFlowy) | Flutter + Rust，同时覆盖桌面端与移动端 | 证明“跨端 UI + Rust 核心”可行；同时它是重型工作空间，Memivy 不应复制其协作、CRDT 和云服务复杂度 |
| [Screenpipe](https://github.com/screenpipe/screenpipe) | Tauri 桌面应用 + Rust + 本地 SQLite/API | 证明 Tauri/Rust 适合桌面常驻、系统捕捉和本地数据库；Memivy 的采集面更小，不需要它的录屏、OCR 和独立本地 API 架构 |
| [Joplin](https://github.com/laurent22/joplin) | Electron 桌面端 + React Native 移动端 + 共享 TypeScript 库 | 产品覆盖完整，但仓库需要大量桌面/移动 shim；说明分裂应用壳的长期维护成本真实存在，[桌面包](https://github.com/laurent22/joplin/blob/dev/packages/app-desktop/package.json) |
| [Notesnook](https://github.com/streetwriters/notesnook) | Electron 桌面端 + React Native 移动端，monorepo 内分为 desktop/mobile/web | 同样验证了成熟但分裂的多端路线；不适合当前小团队 MVP |
| [Reor](https://github.com/reorproject/reor) | Electron + Markdown + LanceDB + 本地模型 | 语义检索能力强，但桌面限定、模型和向量运行时更重；仓库已于 2026-03-07 归档，不能作为 MVP 基座 |

成熟项目的技术与产品做法可以直接借鉴，按 Memivy 当前范围判断采用成本。上述选型参考支持：

- AppFlowy 证明 Rust 共享核心值得采用；
- Screenpipe 证明 Tauri 适合本地桌面型 AI 产品；
- Joplin、Notesnook 说明 Electron + React Native 会形成两个应用壳；
- Reor 说明首版同时承担 Electron、本地模型和向量库，会把产品验证变成运行时维护。

## 4. 总体架构

```text
React / TypeScript 界面（快捷入口、首页、讨论、记忆库）
        │ Tauri commands + events
        ▼
Tauri Host（macOS 应用进程）
        │
        ▼
memivy-core（共享 Rust 核心）
├── capture        原话落盘、来源、撤销
├── memory         新建/续写/暂不判断、版本历史
├── search         无模型的 FTS5 检索、过滤、结果裁剪
├── conversation   记忆问答、会话上下文、引用、确认保存
├── ai             OpenAI-compatible 调用与输出校验
├── export         Markdown 导出与安全备份
└── macos          菜单栏、快捷键、前台应用名
        │
        ▼
SQLite / 本机配置文件

macOS：菜单栏、全局快捷键、前台应用名、MCP
```

这是职责划分，不要求每个职责单独拆 crate、服务或框架。`conversation` 复用 `search`、`ai` 和 `memory`，不建立第二套记忆写入规则。UI 与 MCP 仍通过同一核心访问数据；MCP 不暴露应用内会话或新增问答工具。

### 4.1 进程安排

- 不建设 Memivy 云服务；
- 不再拆一个常驻 daemon；Tauri 主进程本身就是菜单栏常驻进程；
- MCP 使用同仓库生成的轻量 `memivy-mcp` stdio 程序，由 Agent 需要时启动，不常驻、不监听端口、不拥有另一份数据；
- UI、MCP 和后台处理都调用同一个 `memivy-core`，不能各写一套记忆规则。

2026-09-05 已确认：关闭主窗口只隐藏窗口，菜单栏进程继续运行；彻底退出应用后，已开启的 MCP 仍可保存原话与 pending 状态，明确返回等待应用运行后整理。主进程下次启动再消费 pending；实际 AI 消费器归阶段 5，阶段 1 仅验证独立进程写入与待处理状态。

MCP 官方 Rust SDK 将 stdio 定义为本地 MCP Server 作为子进程启动的标准方式，并同时支持 Streamable HTTP。[rmcp transport](https://github.com/modelcontextprotocol/rust-sdk/blob/main/README.md#transports) Memivy MVP 只实现 stdio，避免本地端口、Origin 校验、鉴权令牌和客户端 HTTP 兼容性问题。

## 5. macOS 能力边界

| 能力 | MVP |
|---|---:|
| 记录、编辑、版本、关键词搜索 | 是 |
| 记忆问答、继续讨论、确认结论保存 | 是，后续阶段实现 |
| 本地 SQLite 与 AI | 是 |
| 菜单栏常驻 | 是 |
| 系统级全局快捷键 | 是 |
| 获取前台应用名 | 是 |
| 本机 MCP | 是 |

只读取前台应用名称。网页地址和文件路径仍必须由用户主动附带，不因为只支持 macOS 就扩大系统监控范围。

## 6. 数据与检索

### 6.1 SQLite 的所有权

`memivy-core` 是唯一数据访问层。React 不直接执行 SQL，MCP 也不能绕过核心规则修改表。

建议配置：

- `rusqlite` 使用 `bundled`，把受控 SQLite 版本一起编译，避免各系统自带版本不一致；官方项目也将其推荐给自主管理数据库的应用。[rusqlite](https://github.com/rusqlite/rusqlite)
- 开启 foreign keys、WAL、synchronous=FULL 和合理的 busy timeout；锁定包含 WAL-reset 修复的稳定 SQLite 构建，并检查实际链接版本；
- 使用显式 SQL migration，不引入 ORM；
- 在同一事务保存 `captures` 与初始 pending 状态，提交后才启动 AI 调用；任何模型失败都不能回滚原话；
- UI 和 MCP 并发写入时仍走相同事务规则。

### 6.2 记忆与会话分层

- 正式数据层以 `captures`、`memories`、`memory_versions` 和版本来源保存长期记忆；阶段 1 隔离数据库不直接升级为用户正式数据；
- `conversations`、`turns`、`messages` 与 `message_citations` 保存会话、尝试 ID、消息角色、顺序、状态及引用；会话消息支持游标分页。打开数据库不打断其他活跃进程的回答，重启恢复由确认旧进程已退出的应用 host 显式执行；
- 普通问题、AI 回答和讨论草稿只属于会话，不自动创建 capture，也不进入记忆 FTS 或 MCP 搜索；
- 用户确认保存的结论创建独立 capture，保留确认文本、会话/消息来源及生成与确认身份，再复用记忆版本与回执机制；
- 回答引用稳定的 capture 或 memory-version ID，不仅引用会变化的当前记忆 ID；来源被用户删除时保留不可用状态，不恢复已删除正文；
- 删除会话不连带删除已确认保存的 capture 和记忆。会话在本地保留供继续讨论，导出时与长期记忆区分；
- 不为了问答引入关系图、完整用户画像或额外记忆数据库。

正式迁移位于 `migrations/memory/`，通过 application ID 与 schema version 拒绝样机、无关或较新数据库；初始化前在同一读快照检查表和版本信息，取得写锁后再次核对，避免并发首次打开误报格式错误。写事务使用 `BEGIN IMMEDIATE`，保留 WAL / FULL / 750 ms busy timeout。原话与版本正文不允许覆盖，用户主动永久删除时才擦除正文并留下来源不可用的 ID。

`memory_versions` 保存完整快照，`version_captures` 保存原话关联；编辑、恢复和撤销均追加版本。`receipts` 与 `receipt_changes` 和版本同事务提交，纠正归属记录两边的变更，撤销校验所有涉及的当前版本。撤销纠错的回执指向恢复后的归属与新版本，可继续纠正；再次纠正只处理当前归属，不重做之前已撤销的目标变更。请求以 UUID 和 SHA-256 内容摘要去重；相同文本的不同主动保存仍分别保留。SHA-256 只用于请求一致性校验，不是内容加密。

永久删除原话时，在同一事务擦除关联的 `undone` 隐藏记忆及其 FTS 正文，保留其他有效或回收站中的记忆。也可通过回执中的记忆 ID 显式清空已撤销的版本，此操作保留仍有效的原话；普通撤销本身不擦除原话。新生成的备份不会携带这些已擦除的隐藏正文，已有备份仍由用户自行管理。

`conclusion_intents` 保存用户确认的名称和去向；目标变更或删除时，确认原文和 intent 仍提交，回执为 `needs_review`，由用户纠正。来源角色与会话 ID 保存在独立 capture 中，删除会话不级联删除已保存结论。引用表只保存稳定来源 ID，不缓存一份会在原文删除后重新出现的引用正文。

### 6.3 中文关键词搜索使用 trigram

PRD 决定 MVP 不使用 embedding，但 FTS5 默认 `unicode61` 对连续中文文本不够好。使用 FTS5 `trigram` tokenizer，让连续三个 Unicode 字符形成索引，支持中文和英文子串匹配。少于三个字符的查询使用 `instr` 做字面匹配，避免把 `%`、`_` 当作通配符；结果数量有上限，短词扫描的工作量不等同于结果上限。[SQLite FTS5 trigram](https://www.sqlite.org/fts5.html#the_trigram_tokenizer)

阶段 2 的 `record_fts` 是一个随原话和版本写入维护的派生索引；永久删除正文时同步移除索引项，启用 FTS secure-delete。普通检索只返回有效的当前版本与未归属原话，聊天不入索引。当前按更新时间排序；标题权重、来源/时间/项目过滤和索引重建入口留在阶段 3。

数据量在个人 MVP 阶段很小，优先采用一个 trigram 索引，不同时维护中英文两套索引。标题、当前记忆、原话、URL 和 AI 检索词可设置不同权重。关键词搜索与 MCP `memory_search` 不调用模型；应用内问答可以调用模型提取检索词，再复用这层检索，见第 7 节。

### 6.4 备份边界

Mac MVP 只有一份本机 `memivy.db`，不提供多设备自动同步。

不能把正在使用的 SQLite 文件直接放进 Dropbox、iCloud Drive 或网络盘当同步方案。SQLite 依赖可靠文件锁和写入顺序，官方明确提示网络文件系统可能造成损坏；复制活动数据库时还必须正确处理 WAL/journal。[SQLite 网络文件系统说明](https://www.sqlite.org/useovernet.html) · [SQLite 安全备份说明](https://www.sqlite.org/howtocorrupt.html#_backup_or_restore_while_a_transaction_is_active)

因此 MVP：

- 提供记忆、原始输入、历史版本与来源的 Markdown 导出，会话单独标识导出；
- 提供由 SQLite backup API 或 `VACUUM INTO` 生成的一致性备份；
- 只有应用及 MCP 等写入进程全部停止时，才把直接复制完整数据库文件描述为安全操作；
- 不做跨设备合并、CRDT、WebDAV 或云盘同步。

阶段 2 使用 SQLite backup API，分步备份并限制总等待时间，完成后校验、同步文件，以不覆盖已有目标的方式发布；恢复只接受新的空目录，不替换运行中的数据库。Markdown 导出在同一读快照内生成 `captures.md`、`memories.md`、`conversations.md`，包含有效记忆的全部历史和来源，回收站内容不进入 Markdown。`manifest.json` 最后写入，作为导出完成标记；完整数据库备份包含回收站和会话，但不含独立模型配置及 MCP 开关。[SQLite backup API](https://www.sqlite.org/backup.html)

若未来确认必须跨设备共享记忆，需要单独设计“逻辑变更同步”，不能同步活动数据库文件。

## 7. AI 整理、记忆问答与讨论

### 7.1 捕捉后的记忆整理

- 使用 `reqwest` 直接调用已配置的 OpenAI-compatible 端点；Base URL 包含 API 前缀（如 `/v1`），统一追加 `/chat/completions`；
- 不引入供应商 SDK 和多 Provider 抽象；
- 一次落位调用同时返回：动作、目标 memory ID、当前版本内容、标题、检索词和简短理由；
- 使用 `serde` 结构校验输出；结果不合法即标记失败，不猜测执行；
- 候选只来自本地 FTS、当前项目和少量最近记忆，模型不能扫描全库；
- 整理任务状态直接存在 capture 上，例如 pending/processing/done/failed，不建设队列服务。

Tauri 常驻进程持续消费 pending；应用重启后继续处理未完成任务。

### 7.2 基于记忆的问答与继续讨论

最小处理链路：

> 用户问题与有限会话上下文 → 生成检索词 → Rust 核心执行 FTS 与过滤 → 读取少量相关版本和原始输入 → 模型回答并引用来源 → 核心校验后呈现。

- 允许用同一个已配置模型提取或改写少量检索词；这是问答的模型调用，不改变关键词搜索无模型的约定；
- 从具体记忆开始讨论时，把该记忆的明确版本作为上下文；继续追问复用必要会话消息，按需检索相关内容；
- 限制候选数量、上下文长度与模型调用次数，不扫描或发送全库，不增加 embedding、远程检索服务或多 Agent 编排；具体限制在实现时结合案例验证；
- 原始记忆和会话内容作为待分析资料，不能授权模型执行外部动作或修改数据库；
- 核心校验引用 ID 来自本次提供的证据，引用可打开对应版本或原始输入。引用是否支持陈述还需案例评估，不能仅凭 ID 有效认定回答正确；
- 对回忆性陈述、历史变化和新建议作清楚区分；证据不足时说明不足，不能用模型常识补造用户经历；
- 每次回答的状态在会话消息上保存，区分处理中、完成、失败、取消或中断。应用重启后展示中断状态，保留问题供用户继续，不自动重放已取消请求；
- 支持取消、超时和重试；迟到结果不能覆盖已取消或已被新尝试替代的状态，不完整内容不能作为成功回答；
- 普通回答不触发记忆写入。是否流式展示、快捷小窗与主窗口如何衔接，待交互研究及 Demo 确认后再定，不预先增加相关依赖。

### 7.3 确认结论保存

- 将拟保存文本、来源与保存去向提供给用户核对，具体确认界面待研究；
- 明确确认后，经共享核心先创建原始 capture，保留用户确认后的准确文本，标识源自 Memivy 对话；
- 按用户确认的去向复用新建/续接、版本来源、动作回执和撤销机制，不让模型静默改换目标，也不提供模型直接写库的路径；
- 目标与依据版本在提交时校验；若已删除或变更，不静默写入另一条记忆或覆盖较新版本，保留确认文本并返回可纠正状态；
- 同一次确认保存的重试不能创建重复 capture 或重复应用版本；撤销记忆变更保留已确认原始文本，用户主动删除除外。

### 7.4 API Key 与发送范围

按已确认的产品决定，不使用 Apple Keychain，也不建设服务端密钥托管。

- Key、Base URL、模型 ID 保存在应用数据目录的独立本机配置文件；
- 内容 SQLite、Markdown 导出和诊断日志都不包含 Key；
- 文件权限收紧为当前 macOS 用户可读写；
- 界面明确说明：这是本机明文配置，只防止误导出，不防同一系统用户下的恶意进程；
- 本地端点允许空 Key。

整理、问答与讨论使用同一套配置。问答调用可能发送当前问题、必要会话消息与有限相关记忆；界面说明端点位置和发送范围，不把“数据保存在本地”表述为“使用远程模型时数据不离机”。未配置模型或模型失败时，capture、编辑、历史与关键词搜索仍可用。

阶段 1 第二版另提供可选“快速回答”：配置 `disable_reasoning: true` 时显式发送 `reasoning_effort: "none"`，否则省略该请求字段，不自动探测或静默回退。[Ollama 兼容接口文档](https://docs.ollama.com/api/openai-compatibility)确认支持此控制。本次本地 Qwen 联调采用该选项，避免额外推理耗尽输出预算；其他端点是否支持由用户连接测试确认。

配置与内容分文件，比把 Key 放进 `memivy.db` 更简单地保证备份和内容导出不带密钥。

## 8. MCP

- 只为 macOS 构建 `memory_capture` 与 `memory_search`；
- 使用官方 `rmcp`，不开 Node/Python 子运行时；
- `memivy-mcp` 是按需启动的 stdio 适配器，不是后台服务；
- 每次调用读取 MCP 总开关，关闭时直接拒绝；
- 直接复用 core 的搜索、落盘、来源和结果裁剪逻辑；
- `memory_search` 仍只做无模型的关键词检索，不返回未确认保存的会话内容；应用内新增问答不扩展 MCP 工具集合；
- stdout 只输出协议消息，日志只写 stderr，并统一脱敏 API Key 和记忆正文；
- MVP 不同时实现 HTTP transport。

## 9. 前端与交互实现

- 一个 React 应用，承载快捷记录/提问、近期话题首页、讨论和记忆库；具体布局、入口切换及窗口衔接待研究，不把旧 Demo 的左右分栏定为新方向的固定要求；
- 状态先用 React hooks 与局部 context，不引入 Redux；
- 当前记忆版本先采用纯文本/Markdown 编辑器，不上块编辑器、协同编辑器或富文本 JSON 模型；
- 所有写操作通过类型明确的 Tauri command；长任务通过 event/channel 返回进度；
- 记录、关键词搜索和记忆问答使用明确的业务意图；问答状态与捕捉后的整理状态分开，取消回答不撤销已保存的原始输入；
- 首页先使用本地已有记忆与会话，不因近期话题入口增加后台推荐服务、定时总结或用户画像；
- macOS 系统能力统一收口在 Rust 侧，界面不直接调用 shell 或系统 API；
- Tauri capability 配置按窗口最小授权，尤其不向捕捉小窗开放无关文件与 shell 权限。[Tauri capabilities](https://v2.tauri.app/es/security/capabilities/)

Tauri 使用系统 WebView。[Tauri 架构](https://v2.tauri.app/concept/architecture/) 因此中文输入法、窗口焦点和长文本编辑必须在目标 macOS 版本上验证，不能只在浏览器里验收。

本次不修改 `design-demo/`、`DESIGN.md` 或品牌资产。后续先研究 PRD 第 12 节的交互问题，再按授权调整 Demo；新交互经确认后才用于正式 UI 开发。

## 10. 测试与发布

| 层 | 做法 |
|---|---|
| Rust 核心 | 状态转换、事务、迁移、FTS、导出、失败恢复，以及会话与记忆隔离、确认保存去重、引用版本与撤销 |
| AI 合同 | 固定案例验证三种整理动作、问答证据、历史变化与建议区分、继续讨论、无证据、取消及失败不写记忆 |
| React | Vitest + Testing Library，Tauri IPC mock |
| macOS E2E | WebdriverIO 与关键路径人工冒烟，[测试文档](https://v2.tauri.app/develop/tests/webdriver/) |
| 构建 | GitHub Actions 做 Rust、前端测试和 macOS 构建；发布候选必须在干净 Mac 安装验证 |

发布只包含签名、notarization 和 DMG。GitHub Actions 构建成功不等于发布完成，必须验证下载、安装、首次启动、升级和卸载后的数据保留。[Tauri 分发](https://v2.tauri.app/distribute/)

## 11. 明确不引入

- Electron、React Native、Flutter 三套壳并存；
- Web 后端、账号系统、托管模型代理；
- 独立常驻 daemon、本地 HTTP API；
- ORM、Redis、任务队列、微服务；
- embedding、向量数据库、图数据库；
- CRDT、实时协作、跨设备同步；
- 块编辑器、插件系统、主题市场；
- 首版自动更新系统。

## 12. 暂缓的平台

Windows、iOS、Android 不进入当前架构验收和开发排期，也不为其预留平台适配层。未来若重新启动其中任一平台，先单独确认捕捉入口、MCP、同步和发布需求，再评估是否继续复用当前技术路线。Linux 不支持。
