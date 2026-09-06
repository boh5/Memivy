# Memivy 技术栈与架构方案

状态：建议方案
依据：`PRD.md`，研究日期 2026-09-04

## 1. 结论

Memivy 建议采用：

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

阶段 1 实际样机：`src-tauri/` 为原生 host，`src/` 为复用 Demo 样式的最小 React 界面，`crates/memivy-core/` 为共享原话保存、查询和独立模型协议探测，`crates/memivy-mcp/` 为仅含 `memory_capture` 的 stdio 程序。样机迁移只有不可改写的原话表与 FTS 索引；正式 memory/version、撤销和 AI 整理不在本阶段实现。模型探测使用 `reqwest` 的同一 `/chat/completions` 路径，验证完整 JSON schema 结果，拒绝截断、异常状态和超过 64 KB 的响应；探测模块没有数据库访问能力。当前实测 SQLite 为 bundled 3.53.2，完整依赖版本以 lockfile 为准。

2026-09-06 原生验证补充：普通 NSWindow 在访达全屏下未取得可输入焦点，仅增加 fullScreenAuxiliary 仍不足；阶段 1 捕捉窗改由 `src-tauri/src/capture_panel.rs` 配置非激活 NSPanel，保留原 WebView、IPC 与主窗口 Dock 行为。使用 [tauri-nspanel](https://github.com/ahkohd/tauri-nspanel/tree/c9ec2130422200f0863b23dfdad02b133a529b07) 2.1.0，固定提交 `c9ec2130422200f0863b23dfdad02b133a529b07`；它启用 Tauri 的 macos-private-api feature，当前仅作为未签名技术样机验证，不代表已完成分发审核。原生面板操作限定在主线程；输入就绪按 DOM 输入框焦点加 NSPanel key-window 状态判断，非激活面板无需把整个应用设为前台。

技术实现只为 Mac MVP 服务：菜单栏常驻、全局快捷键、本地 SQLite、Memory Agent 和 MCP 都在同一台 Mac 上运行。暂不为未来平台提前增加适配层。

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
2. SQLite、AI 落位、版本历史、导出和 MCP 都适合放在一个可测试的 Rust 核心里；
3. Tauri 原生提供桌面托盘，并有桌面全局快捷键与自启动能力。[系统托盘](https://v2.tauri.app/learn/system-tray/) · [插件能力表](https://v2.tauri.app/plugin/)
4. 记忆页、来源时间线、搜索和 Markdown 编辑更适合复用成熟的 Web UI 与编辑器生态；
5. 不需要 Electron 自带一整套 Chromium，也不必为了原生界面引入 SwiftUI 与 Rust FFI 两套开发面。

Flutter 的主要优势是移动端成熟度和统一渲染，但这些不是当前 Mac MVP 的核心收益。Memivy 所需的菜单栏和系统级快捷键还主要依赖第三方桌面插件；选择 Rust 后则可直接使用 MCP 官方 Rust SDK。[tray_manager](https://pub.dev/packages/tray_manager) · [hotkey_manager](https://pub.dev/documentation/hotkey_manager/latest/) · [rmcp](https://github.com/modelcontextprotocol/rust-sdk)

如果未来决定开发移动端，再根据当时需求重新比较 Tauri 与 Flutter；当前不为这个可能性增加代码复杂度。

## 3. 开源项目给出的证据

| 项目 | 当前技术路线 | 对 Memivy 的启示 |
|---|---|---|
| [AppFlowy](https://github.com/AppFlowy-IO/AppFlowy) | Flutter + Rust，同时覆盖桌面端与移动端 | 证明“跨端 UI + Rust 核心”可行；同时它是重型工作空间，Memivy 不应复制其协作、CRDT 和云服务复杂度 |
| [Screenpipe](https://github.com/screenpipe/screenpipe) | Tauri 桌面应用 + Rust + 本地 SQLite/API | 证明 Tauri/Rust 适合桌面常驻、系统捕捉和本地数据库；Memivy 的采集面更小，不需要它的录屏、OCR 和独立本地 API 架构 |
| [Joplin](https://github.com/laurent22/joplin) | Electron 桌面端 + React Native 移动端 + 共享 TypeScript 库 | 产品覆盖完整，但仓库需要大量桌面/移动 shim；说明分裂应用壳的长期维护成本真实存在，[桌面包](https://github.com/laurent22/joplin/blob/dev/packages/app-desktop/package.json) |
| [Notesnook](https://github.com/streetwriters/notesnook) | Electron 桌面端 + React Native 移动端，monorepo 内分为 desktop/mobile/web | 同样验证了成熟但分裂的多端路线；不适合当前小团队 MVP |
| [Reor](https://github.com/reorproject/reor) | Electron + Markdown + LanceDB + 本地模型 | 语义检索能力强，但桌面限定、模型和向量运行时更重；仓库已于 2026-03-07 归档，不能作为 MVP 基座 |

竞品结论不是“谁用了什么就照抄”，而是：

- AppFlowy 证明 Rust 共享核心值得采用；
- Screenpipe 证明 Tauri 适合本地桌面型 AI 产品；
- Joplin、Notesnook 说明 Electron + React Native 会形成两个应用壳；
- Reor 说明首版同时承担 Electron、本地模型和向量库，会把产品验证变成运行时维护。

## 4. 总体架构

```text
React / TypeScript 界面
        │ Tauri commands + events
        ▼
Tauri Host（macOS 应用进程）
        │
        ▼
memivy-core（共享 Rust 核心）
├── capture        原话落盘、来源、撤销
├── memory         新建/续写/暂不判断、版本历史
├── search         FTS5、过滤、结果裁剪
├── ai             OpenAI-compatible 调用与输出校验
├── export         Markdown 导出与安全备份
└── macos          菜单栏、快捷键、前台应用名
        │
        ▼
SQLite / 本机配置文件

macOS：菜单栏、全局快捷键、前台应用名、MCP
```

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
| 记录、编辑、版本、搜索 | 是 |
| 本地 SQLite 与 AI | 是 |
| 菜单栏常驻 | 是 |
| 系统级全局快捷键 | 是 |
| 获取前台应用名 | 是 |
| 本机 MCP | 是 |

只读取前台应用名称。网页地址和文件路径仍必须由用户主动附带，不因为只支持 macOS 就扩大系统监控范围。

## 6. 数据与搜索

### 6.1 SQLite 的所有权

`memivy-core` 是唯一数据访问层。React 不直接执行 SQL，MCP 也不能绕过核心规则修改表。

建议配置：

- `rusqlite` 使用 `bundled`，把受控 SQLite 版本一起编译，避免各系统自带版本不一致；官方项目也将其推荐给自主管理数据库的应用。[rusqlite](https://github.com/rusqlite/rusqlite)
- 开启 foreign keys、WAL、synchronous=FULL 和合理的 busy timeout；锁定包含 WAL-reset 修复的稳定 SQLite 构建，并检查实际链接版本；
- 使用显式 SQL migration，不引入 ORM；
- 在同一事务保存 `captures` 与初始 pending 状态，提交后才启动 AI 调用；任何模型失败都不能回滚原话；
- UI 和 MCP 并发写入时仍走相同事务规则。

### 6.2 中文搜索必须使用 trigram

PRD 决定 MVP 不使用 embedding，这没有问题，但 FTS5 默认 `unicode61` 对连续中文文本不够好。首版应使用 FTS5 `trigram` tokenizer，让连续三个 Unicode 字符形成索引，从而支持中文和英文子串匹配。SQLite 官方文档明确说明 trigram 面向通用子串检索；少于三个字符的查询需要退回普通 `LIKE` 扫描。[SQLite FTS5 trigram](https://www.sqlite.org/fts5.html#the_trigram_tokenizer)

数据量在个人 MVP 阶段很小，优先采用一个 trigram 索引，不同时维护中英文两套索引。标题、当前记忆、原话、URL 和 AI 检索词可设置不同权重；搜索仍不调用模型。

### 6.3 备份边界

Mac MVP 只有一份本机 `memivy.db`，不提供多设备自动同步。

不能把正在使用的 SQLite 文件直接放进 Dropbox、iCloud Drive 或网络盘当同步方案。SQLite 依赖可靠文件锁和写入顺序，官方明确提示网络文件系统可能造成损坏；复制活动数据库时还必须正确处理 WAL/journal。[SQLite 网络文件系统说明](https://www.sqlite.org/useovernet.html) · [SQLite 安全备份说明](https://www.sqlite.org/howtocorrupt.html#_backup_or_restore_while_a_transaction_is_active)

因此 MVP：

- 提供 Markdown 导出；
- 提供由 SQLite backup API 或 `VACUUM INTO` 生成的一致性备份；
- 只有应用完全退出时，才把直接复制数据库文件描述为安全操作；
- 不做跨设备合并、CRDT、WebDAV 或云盘同步。

若未来确认必须跨设备共享记忆，需要单独设计“逻辑变更同步”，不能同步活动数据库文件。

## 7. Memory Agent

### 7.1 调用方式

- 使用 `reqwest` 直接调用一个 OpenAI-compatible `/v1/chat/completions` 端点；
- 不引入供应商 SDK 和多 Provider 抽象；
- 一次落位调用同时返回：动作、目标 memory ID、当前版本内容、标题、检索词和简短理由；
- 使用 `serde` 结构校验输出；结果不合法即标记失败，不猜测执行；
- 候选只来自本地 FTS、当前项目和少量最近记忆，模型不能扫描全库；
- AI 任务状态直接存在 capture 上，例如 pending/processing/done/failed，不建设队列服务。

Tauri 常驻进程持续消费 pending；应用重启后继续处理未完成任务。

### 7.2 API Key

按已确认的产品决定，不使用 Apple Keychain，也不建设服务端密钥托管。

- Key、Base URL、模型 ID 保存在应用数据目录的独立本机配置文件；
- 内容 SQLite、Markdown 导出和诊断日志都不包含 Key；
- 文件权限收紧为当前 macOS 用户可读写；
- 界面明确说明：这是本机明文配置，只防止误导出，不防同一系统用户下的恶意进程；
- 本地端点允许空 Key。

配置与内容分文件，比把 Key 放进 `memivy.db` 更简单地保证备份和内容导出不带密钥。

## 8. MCP

- 只为 macOS 构建 `memory_capture` 与 `memory_search`；
- 使用官方 `rmcp`，不开 Node/Python 子运行时；
- `memivy-mcp` 是按需启动的 stdio 适配器，不是后台服务；
- 每次调用读取 MCP 总开关，关闭时直接拒绝；
- 直接复用 core 的搜索、落盘、来源和结果裁剪逻辑；
- stdout 只输出协议消息，日志只写 stderr，并统一脱敏 API Key 和记忆正文；
- MVP 不同时实现 HTTP transport。

## 9. 前端与交互实现

- 一个 React 应用，采用适合 Mac 窗口的左右分栏；
- 状态先用 React hooks 与局部 context，不引入 Redux；
- 当前记忆版本先采用纯文本/Markdown 编辑器，不上块编辑器、协同编辑器或富文本 JSON 模型；
- 所有写操作通过类型明确的 Tauri command；长任务通过 event/channel 返回进度；
- macOS 系统能力统一收口在 Rust 侧，界面不直接调用 shell 或系统 API；
- Tauri capability 配置按窗口最小授权，尤其不向捕捉小窗开放无关文件与 shell 权限。[Tauri capabilities](https://v2.tauri.app/es/security/capabilities/)

Tauri 使用系统 WebView。[Tauri 架构](https://v2.tauri.app/concept/architecture/) 因此中文输入法、窗口焦点和长文本编辑必须在目标 macOS 版本上验证，不能只在浏览器里验收。

## 10. 测试与发布

| 层 | 做法 |
|---|---|
| Rust 核心 | 状态转换、事务、迁移、FTS、导出、失败恢复的单元与集成测试 |
| AI 合同 | 固定案例集验证三种动作、结构输出、证据引用和失败不丢原话 |
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
