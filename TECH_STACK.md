# Memivy 技术栈与架构方案

状态：沿用已验证技术基础；2026-09-06 阶段 3 首页与记忆库已接入正式数据层，实际验证见开发计划
依据：[PRD.md](PRD.md)，技术选型研究日期 2026-09-04，产品方向同步日期 2026-09-06

原阶段 1 于 2026-09-06 验收通过。此后用户另行授权在现有样机中实现悬浮助手和最小记忆讨论闭环，作为阶段 1 第二版；原验收结论保留。模型配置与原生焦点问题修复后，用户于同日确认所给检查清单全部通过，第二版完成用户手工验收。实际测试范围、后续实施顺序与授权状态见 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)。

阶段 1 第二版使用同一 NSPanel 原地调整大小，提供收起图标、输入和回答状态；React 复用一套输入与讨论组件，通过 host 传递话题、输入模式、草稿和所选记忆。`002_interaction.sql` 为隔离样机增加话题、讨论轮次、确认回执和撤销标记，不等同于正式 memory/version 模型。普通会话不进入 FTS；结论只支持用户核对后存为新的不可改写 capture，撤销后退出可检索记忆，保留确认原文和出处。一次提问至多两次模型请求、4 个检索词、8 条原话证据与4轮节选历史；完成状态校验阻止取消后的迟到回答写回。未实现正式 AI 自动整理、续写旧记忆或完整版本编辑。

悬浮拖动实验覆盖整个图标及展开后的标题区；WebView 使用 pointer capture 区分点击与拖动，Rust 读取系统鼠标和窗口的物理坐标来移动原生窗口，不依赖跨 WebView IPC 后的旧鼠标事件。拖动不触发展开，短点击才进入输入状态；实际手感和跨屏行为仍按原生验收记录核对。

阶段 2 的正式接口为 `memivy_core::memory::MemoryStore`，位于 `crates/memivy-core/src/memory/`。默认数据目录为 `~/Library/Application Support/com.memivy.app/`，事实库为 `memivy.db`；通过显式绝对目录进行测试。默认原生入口 `src-tauri/src/workspace.rs` 与 `src/workspace/` 已接入正式接口；`npm run dev:app` 启动正式界面。已验收样机保留在 `prototype.rs` / `Prototype.tsx`，用 `npm run dev:prototype` 启动，继续与显式的 `memivy-mcp-prototype` 使用独立 Phase 1 Store。没有自动迁移样机数据。正式记录、编辑和搜索不调用模型，模型配置/固定文字连接测试单独提供；用户随后要求保留已有可用体验：正式界面现接回问答、从记忆开始讨论、引用、取消/重试及确认保存。2026-09-07 用户授权阶段 4，正式桌面快捷入口已接入；阶段 5 已接入自动 AI 整理；2026-09-07 用户授权阶段 6，默认 `memivy-mcp` 已切换到正式 MemoryStore。

正式讨论使用 `MemoryStore::start_turn` 持久化问题与等待状态，再通过已有 `model::complete` 做检索词提取和回答两次请求。答案请求前冻结至多 8 份版本/原话节选，回答只允许引用这批依据；近期对话取最近 8 条消息、各至多 1000 字，作为讨论上下文。取消和重启恢复都以持久化状态阻止迟到完成。讨论输入及选定依据单独存为草稿，不加入记忆检索；结论必须经文字与目的地确认后调用 `save_conclusion`。正常正式配置缺失时可一次性沿用旧样机已有的本机模型配置，显式测试数据/配置覆盖不会读取用户旧配置。

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

阶段 4 的原生入口集中在 `src-tauri/src/desktop.rs`，复用 `capture_panel.rs` 的非激活 NSPanel；所有 AppKit/菜单/窗口操作在主线程执行。`src/workspace/Desktop.tsx` 与主窗口复用 `CaptureForm`、`Discussion`、来源与结论确认组件。正式话题任务由 host 管理，窗口收起不会停止；命令标签仅允许已知的 main/capture 窗口，模型设置、导出与索引维护保留主窗口边界。

快捷草稿使用正式 `workspace_drafts` 的 `quick_capture` / `quick_question` 键，来源与原文在同一个 payload；会话草稿仍使用 `discussion:<id>`。Core 比较已读的 request ID 后才写入/清理草稿，跨窗口刷新不能覆盖本地待保存内容；冲突时保留两边文字，用户核对后选择继续版本。捕捉沿用原有幂等 request ID、精确原话和事务规则，不增加事实库或迁移样机数据。

快捷键、固定、显隐、暂停、位置、最近话题及最近捕捉 ID 保存在应用数据目录的 `desktop.json`，不包含原话和模型凭据。来源应用名只在主动唤起时取得；不读取屏幕、选中文本、浏览器 URL 或剪贴板。菜单栏模板图标由已选品牌矢量去底色生成。登录项直接读取并调用 macOS 13 起提供的 `SMAppService.mainAppService`（阶段 6 正式包最低版本声明 26.0，仅面向 Apple Silicon），区分未开启、已开启、待系统允许和应用包不可用；默认不注册，不额外部署后台服务。

登录项状态查询会同步等待系统服务，因此从快捷窗口状态快照中分离：仅设置页打开或重新获得焦点时，通过独立命令在阻塞工作线程查询；登录项变更后使用操作返回的真实状态。窗口打开、收起和快捷键判断只读取所需的窗口状态，不查询登录项。设置页用请求序号拒绝旧状态覆盖新的操作结果，查询期间其余设置仍可使用。

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

阶段 2 的 `record_fts` 是一个随原话和版本写入维护的派生索引；永久删除正文时同步移除索引项，启用 FTS secure-delete。普通检索只返回有效的当前版本与未归属原话，聊天不入索引。阶段 3 的 `library()` 在限量前执行来源/项目/更新时间过滤，按标题命中权重与更新时间排序并分页；短词按字面匹配，SQLite progress handler 将查询限制在约 500ms 执行预算内。来源命中返回原文片段，独立删除的原话不借记忆重新暴露。设置提供事务性索引重建，启动时也会重建缺失的 FTS 表。`003_workspace.sql` 将 schema 升至 3，编辑草稿单独落表，不进入搜索、MCP 或 Markdown；内容提交仍走版本/回执/乐观并发检查。

数据量在个人 MVP 阶段很小，优先采用一个 trigram 索引，不同时维护中英文两套索引。标题、当前记忆、原话、URL 和 AI 检索词可设置不同权重。关键词搜索与 MCP `memory_search` 不调用模型；应用内问答可以调用模型提取检索词，再复用这层检索，见第 7 节。

### 6.4 备份边界

Mac MVP 只有一份本机 `memivy.db`，不提供多设备自动同步。

不能把正在使用的 SQLite 文件直接放进 Dropbox、iCloud Drive 或网络盘当同步方案。SQLite 依赖可靠文件锁和写入顺序，官方明确提示网络文件系统可能造成损坏；复制活动数据库时还必须正确处理 WAL/journal。[SQLite 网络文件系统说明](https://www.sqlite.org/useovernet.html) · [SQLite 安全备份说明](https://www.sqlite.org/howtocorrupt.html#_backup_or_restore_while_a_transaction_is_active)

因此 MVP：

- 日常导出位于单篇记忆页，生成该篇标题与当前正文的一个 Markdown 文件；编辑草稿需先保存；不从设置导出整库、历史或会话；
- 提供由 SQLite backup API 或 `VACUUM INTO` 生成的一致性备份；
- 只有应用及 MCP 等写入进程全部停止时，才把直接复制完整数据库文件描述为安全操作；
- 不做跨设备合并、CRDT、WebDAV 或云盘同步。

阶段 2 使用 SQLite backup API，分步备份并限制总等待时间，完成后校验、同步文件，以不覆盖已有目标的方式发布；恢复只接受新的空目录，不替换运行中的数据库。阶段 2 已验证的数据迁移辅助接口在同一读快照内生成 `captures.md`、`memories.md`、`conversations.md`，包含有效记忆的全部历史和来源，回收站内容不进入 Markdown。`manifest.json` 最后写入，作为导出完成标记；完整数据库备份包含回收站和会话，但不含独立模型配置及 MCP 开关。[SQLite backup API](https://www.sqlite.org/backup.html)

2026-09-06 用户进一步明确导出只针对单篇：正式 UI 移除整库 `library_export` 命令及设置入口，改用记忆页折叠“更多”菜单的 `memory_export`。原生文件保存面板默认以标题命名 `.md` 文件，由 `MemoryStore::export_record_markdown` 核对所见版本并写入该篇标题与当前正文。文件先写临时文件再原子发布，取消不写入，陈旧版本/回收站内容拒绝导出；其余记忆、原话附件、历史、草稿和会话不混入该文件。阶段 2 的迁移辅助接口与数据库备份保留在核心层，不是正式 UI 的日常导出入口。

若未来确认必须跨设备共享记忆，需要单独设计“逻辑变更同步”，不能同步活动数据库文件。

## 7. AI 整理、记忆问答与讨论

### 7.1 捕捉后的记忆整理

- 使用 `reqwest` 直接调用已配置的 OpenAI-compatible 端点；Base URL 包含 API 前缀（如 `/v1`），统一追加 `/chat/completions`；
- 不引入供应商 SDK 和多 Provider 抽象；
- 一次落位调用同时返回：动作、有限候选别名、新建正文或续接的补充/局部修改、标题、检索词和简短理由；
- 按 2026-09-07 已确认规则，续接保留未涉及的正文，由核心基于所用版本应用有限变更并创建新版本，不让模型自动压缩重写整篇；用户明确改变判断时更新当前理解，保留历史，假设与质疑仍标明不确定性；
- 使用 `serde` 结构校验输出；结果不合法即标记失败，不猜测执行；
- 候选只来自本地 FTS、当前项目和少量最近记忆，模型不能扫描全库；候选保留有效来源的项目元数据。原话明确标注项目时，筛选和事务提交都拒绝带有其他项目来源的自动续接；
- 每条原话在同一事务中关联一条 `organization_jobs`（pending/processing/done/failed/deferred/paused），原话内容与任务状态分离；重试生成新的 attempt ID，重启沿用被中断的 ID，不建设队列服务。确认的讨论结论不进入自动整理队列。
- 最多提供 6 条当前版本，每条最多 3000 个字符的节选；续接最多 3 处逐字匹配的局部修改，总修改范围不得超过旧正文一半。核心还检查目标仍是所用版本、原话仍有效且未被其他动作归属；未知目标、空替换、重叠范围及过时结果均拒绝执行。
- 检索词保存在可重建的派生索引中，不伪装成原话。记忆页展示最近一次整理回执，更早回执折叠；可查看前后正文、撤销、改为新记忆和重试。

Tauri 常驻进程持续消费 pending；应用重启后继续处理未完成任务。模型结果落库失败后的终态写入会保留在消费者中，以 750 毫秒到 5 秒的退避间隔重试；数据库锁释放后可恢复到失败重试入口，不重复调用模型。

### 7.2 基于记忆的问答与继续讨论

最小处理链路：

> 用户问题与有限会话上下文 → 生成检索词 → Rust 核心执行 FTS 与过滤 → 读取少量相关版本和原始输入 → 模型回答并引用来源 → 核心校验后呈现。

- 允许用同一个已配置模型提取或改写少量检索词；这是问答的模型调用，不改变关键词搜索无模型的约定；
- 从具体记忆开始讨论时，把该记忆的明确版本作为上下文；继续追问复用必要会话消息，按需检索相关内容；
- 限制候选数量、上下文长度与模型调用次数，不扫描或发送全库，不增加 embedding、远程检索服务或多 Agent 编排；当前限制为 1–4 个独立查询，先合并各查询排名及命中的原话集合再限量，最多 8 个来源、每个 1800 个字符的节选和最近 8 条会话节选；涉及历史变化时补入相关记忆最近 2 个旧版本；
- 原始记忆和会话内容作为待分析资料，不能授权模型执行外部动作或修改数据库；
- 回答以逐段回忆（每段带来源）、新建议和可审核结论三个字段保存。核心校验引用 ID 来自本次提供的证据，并保存该版本的 Unicode 节选位置，点击引用打开当轮实际送入模型的片段；展示记录时间及当前/历史状态，来源删除后如实显示不可用。引用是否支持陈述还需案例评估，不能仅凭 ID 有效认定回答正确；
- 对回忆性陈述、历史变化和新建议作清楚区分；证据不足时说明不足，不能用模型常识补造用户经历；
- 每次回答的状态在会话消息上保存，区分处理中、完成、失败、取消或中断。应用重启后展示中断状态，保留问题供用户继续，不自动重放已取消请求；
- 支持取消、超时和重试；迟到结果不能覆盖已取消或已被新尝试替代的状态，不完整内容不能作为成功回答；
- 长文按独立查询词的加权覆盖选择连续节选，标题中的泛主题词权重较低；保存原始 Unicode 字符偏移，查看引用时返回当轮实际提供的片段。
- 普通回答不触发记忆写入。快捷小窗与主窗口按阶段 4 已确认的同话题交接衔接；目前按完整回答更新，流式展示未纳入本阶段。

### 7.3 确认结论保存

- 使用共享结论弹窗核对文本、标题和去向，可搜索已有记忆，并优先列出本次讨论的关联记忆；融合结果可继续编辑，改动结论或去向会作废旧预览，迟到结果不能覆盖新草稿；
- 明确确认后，经共享核心先创建原始 capture，保留用户确认后的准确文本，标识源自 Memivy 对话；
- 存入已有记忆时默认将确认文本补充到现有正文；用户选择融合时，先生成并展示融合结果，再一次确认最终效果。确认文本与最终版本正文分别处理，不能直接把一段结论当作整篇替换正文；确认后不再调用模型改写最终效果；
- 按用户确认的去向复用新建/续接、版本来源、动作回执和撤销机制，不让模型静默改换目标，也不提供模型直接写库的路径；
- 目标与依据版本在提交时校验；若已删除或变更，不静默写入另一条记忆或覆盖较新版本，保留确认文本、标题、目标以及完整审核融合正文并返回可纠正状态；完整稿不进入搜索，能从原话页恢复核对后另存，永久删除原话时一并清理；
- 同一次确认保存的重试不能创建重复 capture 或重复应用版本；撤销记忆变更保留已确认原始文本，用户主动删除除外。

阶段 5 仍处于开发阶段，按 2026-09-07 用户决定，不建设旧数据兼容、历史记录批量整理或升级迁移交互；正常运行期的版本校验、幂等与中断恢复仍按上述规则实现。

### 7.4 手动正文整理与模型输出策略（2026-09-09）

- 核心 `memory/cleanup.rs` 分离快照准备、只读模型提案和接受保存。快照从同一 SQLite 读事务取得当前版本及可选编辑草稿；模型函数没有 MemoryStore 或写入能力，只接收这一篇的原文、可选候选稿和补充要求。
- 接受使用现有写事务、版本/来源、请求指纹、回执和撤销。CAS 同时核对版本及草稿请求和内容；必要时先将来源草稿留成版本，再保存审核稿，原子消费该草稿。失败整笔回滚，重试原请求返回同一回执。schema 8 为版本增加不可变的 `review_kind` 注记，数据库底层仍沿用 edit 原因，通过现有读取接口呈现 cleanup 来源；无需重建版本表。
- Tauri `cleanup.rs` 只编排主窗口命令和单个可取消任务，先预约请求再发起模型调用，防止取消早于生成命令时重新启动。数据库操作放入阻塞线程；生成及等待不持有数据库事务。前端 `MemoryCleanup.tsx` 隔离预览状态，使用现有 Milkdown；过期响应由组件生命周期丢弃，确认读取同步更新的正文引用，避免快捷键漏掉最后一次输入。
- `cleanupDiff.ts` 按行有限向前匹配（32 行），不建立全文平方大小的矩阵；超过 4000 行合并为完整删/增块，限制 DOM 数量但不裁剪正文。接受稿仍遵守核心 128 KiB 正文边界。
- 模型请求默认省略输出 token 参数。可选 `max_output_tokens` 及 `output_token_parameter` 保存在既有独立私有配置；参数名由用户选 max_tokens 或 max_completion_tokens，不按模型名猜测。普通请求和全文任务分别有响应字节/总时间边界，HTTP 连接池共用；全文放宽响应边界不改变其他业务输入合同。finish_reason=length 单独报截断，残缺结果不进入候选稿。连接探测保持 64 KiB 小响应边界。

### 7.5 API Key 与发送范围

按已确认的产品决定，不使用 Apple Keychain，也不建设服务端密钥托管。

- Key、Base URL、模型 ID 保存在应用数据目录的独立本机配置文件；
- 内容 SQLite、Markdown 导出和诊断日志都不包含 Key；
- 文件权限收紧为当前 macOS 用户可读写；
- 界面明确说明：这是本机明文配置，只防止误导出，不防同一系统用户下的恶意进程；
- 本地端点允许空 Key。

整理、问答与讨论使用同一套配置。问答调用可能发送当前问题、必要会话消息与有限相关记忆；界面说明端点位置和发送范围，不把“数据保存在本地”表述为“使用远程模型时数据不离机”。未配置模型或模型失败时，capture、浏览、编辑与历史仍可用；底层关键词检索和 MCP 无模型，主窗口 AI 检索显示失败原因。

阶段 1 第二版另提供可选“快速回答”：配置 `disable_reasoning: true` 时显式发送 `reasoning_effort: "none"`，否则省略该请求字段，不自动探测或静默回退。[Ollama 兼容接口文档](https://docs.ollama.com/api/openai-compatibility)确认支持此控制。本次本地 Qwen 联调采用该选项，避免额外推理耗尽输出预算；其他端点是否支持由用户连接测试确认。

配置与内容分文件，比把 Key 放进 `memivy.db` 更简单地保证备份和内容导出不带密钥。

## 8. MCP

- 只为 macOS 构建 `memory_capture` 与 `memory_search`；
- 使用官方 `rmcp`，不开 Node/Python 子运行时；
- `memivy-mcp` 是按需启动的 stdio 适配器，不是后台服务；
- 每次数据工具调用在跨进程共享锁内读取 MCP 总开关；开关写入用独占锁与原子文件替换，关闭成功后不再接纳新数据调用；协议发现和本地诊断仍可使用；
- 直接复用 core 的搜索、落盘、来源和结果裁剪逻辑；
- `memory_search` 仍只做无模型的关键词检索，不返回未确认保存的会话内容；应用内新增问答不扩展 MCP 工具集合；
- stdout 只输出协议消息，日志只写 stderr，并统一脱敏 API Key 和记忆正文；
- MVP 不同时实现 HTTP transport。

正式适配器固定 `rmcp = 3.2.0`，声明并测试 2025-11-25 初始化流程和 2026-07-28 按请求携带元数据的发现流程；不以某个客户端的专用协议作为产品边界。输入帧上限 1 MiB，原话上限 128 KiB。保存使用稳定 UUID 与原文、来源指纹实现去重；Agent 名必填，项目与会话 URI 可省略，不从环境推断。返回落盘回执，应用退出时仍可保存并排队，MCP 本身不调用模型。

`memory_search` 要求非空关键词（512 UTF-8 字节、最多 16 个空格分隔的 AND 词），默认 5 条、范围 1–8 条，不提供分页；来源类型、项目、更新时间过滤先于限量。复用 `library_in` 的同一 SQLite 读快照，片段最多约 160 字符、标题最多 200 字符；片段引用实际命中的 capture 或不可变 version ID，独立删除的来源不返回。短词走字面匹配，较长词使用 FTS5 trigram，无模型或向量检索。

开关单独保存在正式数据目录 `mcp.json`（默认关闭），`mcp.lock` 协调 UI 和各 MCP 进程。原生 host 的独立 `data_version` 监听不依赖模型配置，外部落盘会触发既有 `library-refresh`。设置复制同包程序的绝对路径及正式数据目录，检查实际 stdio 握手、应用/服务版本和两个工具名；诊断不读取记忆或触发模型，也不把本地检查描述成外部 Agent 已连接。

后台刷新只更新同一记录的整理任务数据，保留已打开的不可变版本对比和正在执行的操作。MCP 设置操作期间阻止关闭设置，操作后重新读取落盘状态；旧的异步读取不能覆盖新状态，读取失败明确显示状态无法确认。

## 9. 前端与交互实现

- 一个 React 应用：主窗口由 `App` 负责页面与原生衔接，`WorkspaceSidebar` 管理导航，`MemoryList` 管理分页筛选；顶栏 AI 检索与 `CaptureDialog` 复用 `CaptureForm` 和既有草稿协议。正文编辑继续按需加载，不新增状态库或运行时依赖；
- 列表每页最多 40 条，仅在筛选或数据版本变化时读取；选择记忆、切换已有讨论不重新读取列表。请求序号阻止过期响应覆盖新结果，分页锁避免重复加载。顶栏键入不发模型请求，明确提交才启动现有 RAG；
- 记录弹窗关闭只等待自身草稿写入；快捷窗口按 generation 在目标输入草稿就绪后确认衔接。既有草稿 CAS、请求去重、引用和结论审核规则保持不变；
- 状态先用 React hooks 与局部 context，不引入 Redux；
- 2026-09-08 用户指定 Milkdown：当前正文与结论审核使用 `@milkdown/crepe` / `@milkdown/kit` 7.22.1，按需加载正文编辑，仅在选中文字时显示一行浮动格式按钮，不设下拉框或常驻工具栏，不区分正文／源码模式；完整提供用户指定的 Mem 格式栏功能并加 H3，启用 Milkdown 表格组件、任务复选框和下划线标记，支持 Markdown 输入快捷转换；阅读使用 react-markdown 10.1.0 + remark-gfm 4.0.1。共用现有白底、黄色引用线和系统字体，不引入块工作区、协同或富文本 JSON 存储；
- Markdown 字符串仍经过现有草稿、版本、回执与乐观并发接口落到 SQLite；文档变化同步发布，避免编辑器延迟通知遗漏立即保存的末次输入。打开编辑器本身不改写正文；原话保持字面展示。下划线以成对、无属性的 `<u>` 标签保存，通过共享 remark 扩展解析为安全标记；其余 HTML 不执行。待办和表格使用 GFM，图片语法保留为文字占位，不上传附件或加载远程图片；
- 所有写操作通过类型明确的 Tauri command；长任务通过 event/channel 返回进度；
- 记录、关键词搜索和记忆问答使用明确的业务意图；问答状态与捕捉后的整理状态分开，取消回答不撤销已保存的原始输入；
- 首页先使用本地已有记忆与会话，不因近期话题入口增加后台推荐服务、定时总结或用户画像；
- macOS 系统能力统一收口在 Rust 侧，界面不直接调用 shell 或系统 API；
- Tauri capability 配置按窗口最小授权，尤其不向捕捉小窗开放无关文件与 shell 权限。[Tauri capabilities](https://v2.tauri.app/es/security/capabilities/)

Tauri 使用系统 WebView。[Tauri 架构](https://v2.tauri.app/concept/architecture/) 因此中文输入法、窗口焦点和长文本编辑必须在目标 macOS 版本上验证，不能只在浏览器里验收。

本次不修改 `design-demo/`、`DESIGN.md` 或品牌资产。后续先研究 PRD 第 12 节的交互问题，再按授权调整 Demo；新交互经确认后才用于正式 UI 开发。

### 9.1 导航元数据与专题检索

`memory::navigation` 在共享 MemoryStore 内负责置顶、专题与成员关系；schema 6 增加 `record_pins`、`collections`、`collection_entries`、`conversation_collections` 及关系索引，使用原有事务和备份机制。元数据不修改 capture 文本或创建正文版本；专题编辑使用 revision 防止陈旧覆盖，成员增删和置顶设置幂等。原话与整理记忆各保留自己的稳定 record key。永久擦除触发关系清理，回收站状态只影响可见性。

`LibraryQuery` 增加置顶、专题、排除专题和较早排序条件；列表仍分页限量。回顾每批三条，手动换组；不做全库客户端加载或定时模型请求。侧栏置顶与专题各最多 100 项，普通记忆库不受此限制。

专题 RAG 复用既有查询规划与回答模型；SQL 在排序和限量前筛选专题来源，并在绑定证据及提交回答时重新检查成员和专题状态。讨论范围独立持久保存，旧会话、后续追问及快捷窗口不依赖当前 UI 页面推断范围。已移除专题的会话保留历史，但新的问答拒绝扩大范围。专题正文的相关记忆也在限量前限定成员，避免选入专题外来源导致后续讨论失败；快捷窗口的新问题不继承主窗口正在浏览的专题。

专题 AI 推荐由明确按钮启动一次查询规划，发送专题名称／说明和最多三条短节选；本地 FTS 查询排除已有成员，合并去重后返回最多八条真实记录供确认，不增加第二个验证模型、自动归类或外部工具。编辑、推荐和成员管理复用现有 Modal；Milkdown 仍按需加载，没有新增前端依赖。

整理后专题推荐由 `memory::collection_recommendations` 承担：整理成功触发独立异步推荐，host 锁串行化推荐请求，不阻塞下一条原话保存或整理。输入至多 6000 字正文及最多 100 个专题的名称／240 字说明；校验最多三个合法候选。schema 7 的 `collection_feedback` 按不可变回执缓存结果及忽略状态，忽略后重启不再推荐；请求落盘不会覆盖同时发生的忽略。确认加入仍在事务内检查回执、目标版本、专题 revision 和删除状态。读取已缓存建议不依赖模型配置；前端会话缓存合并重复请求，列表按最多 100 个 key 在同一只读事务内读取最新任务和反馈状态，浏览列表不调用模型。没有新依赖或定时任务，备份和恢复包含反馈状态。

`Toast` 统一成功轻提示并限制同时一条，超时自动收起，悬停／聚焦暂停；存储失败仍使用输入处错误。`OrganizationReceipt` 分为标题附近的状态与历史操作两种呈现；`MemoryCollections` 将已有专题和推荐放在正文标题下，理由按需展开。底部提示消失不删除历史回执或成员管理能力。


## 10. 测试与发布

| 层 | 做法 |
|---|---|
| Rust 核心 | 状态转换、事务、迁移、FTS、导出、失败恢复，以及会话与记忆隔离、确认保存去重、引用版本与撤销 |
| AI 合同 | 固定案例验证三种整理动作、问答证据、历史变化与建议区分、继续讨论、无证据、取消及失败不写记忆 |
| React | Node test runner 执行真实 TS/TSX 回调的受控 React 生命周期与 IPC fixture；不将其视为浏览器或原生验收 |
| macOS E2E | 当前用原生 UI 自动化完成关键路径；WebdriverIO 可作为后续测试驱动，[测试文档](https://v2.tauri.app/develop/tests/webdriver/) |
| 构建 | 当前为锁定依赖的本机脚本与 macOS 打包；GitHub Actions 尚未接入。公开发布候选仍需干净 Mac 安装验证 |

2026-09-07 核心测试资产：`npm run test:core-assets` 统一运行离线构建、回归、进程/stdio 故障检查并生成隔离视觉合成库；显式传入 `--models-only --model-config /absolute/private-model.json` 才运行固定整理/问答模型集。固定案例、人工判定要求、失败退出语义和实际验证见 DEVELOPMENT_PLAN.md 第 5 节。沿用已有 Cargo、Node test runner、Python 标准库和原生检查工具，未引入新测试依赖或 CI/运行时服务。

2026-09-07 阶段 6 决策：当前交付 Apple Silicon / macOS 26 的开发测试包。用户没有 Developer ID 证书，因此应用和包内 MCP 只做 ad-hoc 签名，不做公证、不上 App Store；Developer ID 签名和 notarization 是后续分发升级，不将其写成已完成。`npm run build:beta` 构建同版本原生 sidecar、应用、含 Applications 链接与安装说明的 DMG，并输出 SHA-256。构建需要已缓存的锁定依赖及系统磁盘映像设备权限，DMG 不依赖 Finder/AppleScript。没有自动更新器。

打包前校验 Tauri 应用版本、原生 Cargo 包版本与 MCP 包版本一致；MCP 协议版本信息中的服务版本由 Cargo 包版本生成。sidecar 使用 Cargo 返回的实际可执行产物，应用使用 Cargo metadata 的目标目录及显式 `aarch64-apple-darwin` 构建目标，支持自定义 Cargo 输出目录并避免复用旧产物。默认应用输出为 `target/aarch64-apple-darwin/release/bundle/macos/Memivy.app`；DMG 和校验文件固定输出到仓库的 `target/release/bundle/dmg/`。

开发环境用 `npm run build:mcp` 构建 sidecar，`npm run dev:app` 自动执行此步。发布测试包用 `npm run build:beta`；开发用的普通 debug app 不自动带 sidecar，如需完整 debug 包，使用 `npm run tauri -- build --debug --config src-tauri/tauri.beta.conf.json --bundles app`。协议验证运行 `python3 scripts/verify_phase6.py --binary /absolute/path/to/Memivy.app/Contents/MacOS/memivy-mcp`，先构建 `memory_probe`。历史阶段 1 脚本改为显式运行 `memivy-mcp-prototype`，原有协议和数据不变。

安装：将应用移入固定位置后首次启动，再从设置复制 MCP 配置。未公证包可能被 Gatekeeper 拦截；按系统“隐私与安全性”提供的允许打开流程处理，不要求用户关闭系统安全保护。是否能在另一台 Mac 顺利安装需要实际验证。[Tauri 分发](https://v2.tauri.app/distribute/)

手动替换应用前，退出 Memivy 并停止外部 Agent 的 Memivy MCP 子进程；替换后重启、重连。卸载前关闭 MCP、退出应用并移除 Agent 配置，再删除应用包；默认保留 `~/Library/Application Support/com.memivy.app/` 中的记忆、草稿与独立配置。彻底删除数据只能由用户另外明确操作。离线复制整个数据目录前必须停止全部写入进程；其中模型配置含私密信息，不随诊断或安装包分享。

构建或同机开发测试不等于公开发布完成。另一台干净 Mac 的下载、Gatekeeper 首启、真实版本升级和长时间用户试用结果记在 DEVELOPMENT_PLAN.md。

### 2026-09-08 五项优化的架构补充

- `model.rs` 共用有连接池的 HTTP 请求层；普通结构化任务保持 90 秒/64 KiB，全文整理及融合预览使用 180 秒/1 MiB 响应边界（2026-09-09 更新），分别解析 JSON 与单个 Function Calling。内部新建/更新/暂缓参数由 `organization.rs` 转换、校验并提交现有 MemoryStore 事务；不扩展 MCP，不增加 Agent 循环。
- `retrieval.rs` 复用现有 FTS5；问答按来源检索相关历史，相关记忆模块按当前条目聚合。每次查询只开一个连接/读快照，SQL 总执行预算 250ms；每词最多 24 个命中、融合候选最多 32 份，先查 ID 再读正文。短词走受预算限制的字面匹配。问答仍最多 4 词、8 份证据和两次模型请求；本地取证移入阻塞工作线程，不占用模型请求的异步执行线程。
- `related.rs` 先查标题与已有搜索词，不足时再查有限正文词，最多 8 词。按来源 ID 通过索引访问当前记忆，避免每个候选扫描全库；最多返回 3 条。真实命中原话时保留该来源供讨论。React 使用局部状态及 180ms 刷新合并，忽略过期响应，无后台轮询或模型调用。
- `transfer.rs` 负责一致性备份、临时备份校验、固定路径替换与中断恢复；Tauri `backup.rs` 只负责原生选择和重启。现有 `mcp.lock` 抽为小型访问助手，并覆盖 MCP 初始化；正常应用启动先处理恢复，随后才开启数据库业务、整理和监听。复用退出草稿确认，重复确认只能消费一次。安全副本位于当前数据目录的 `recovery/`，不复制模型配置。

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
