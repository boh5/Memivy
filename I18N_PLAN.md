# Memivy I18N 实施方案

日期：2026-09-12

状态：方案已完成独立 Review 并修订，随后对照四个开源 Tauri 项目的固定源码提交补充实施决策；用户已授权完成实现，工程验证与独立 Astra Review-Fix 已完成；实际证据和原生验收边界见 DEVELOPMENT_PLAN.md。方案审阅与源码研究不代表原生验收。

依据：[PRD.md](PRD.md)、[TECH_STACK.md](TECH_STACK.md)、[DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md) 及本次代码检查。

## 1. 目标与范围

首批完整支持简体中文和 English，采用 **i18next + react-i18next**。默认跟随系统，允许用户在设置中手动选择语言；主窗口、桌面快捷入口和应用自有菜单使用一致的语言。

沿用现有 React、Tauri、Rust 和 MemoryStore 架构，保持白色／黄色品牌、布局与阅读样式。翻译资源随应用打包，离线可用。

本期覆盖正式 macOS 应用。保留既有记忆、原始输入、讨论、版本和草稿的原文；语言切换不翻译用户内容、不重建检索索引。历史隔离样机不是本期交付目标。后续新增语言以补充资源和验证为主；本期不增加翻译服务、语言检测模型、通用设置框架或额外平台适配。

## 2. 当前代码与改造入口

| 当前情况 | 改造入口 |
|---|---|
| React 界面文案散落在页面、模块常量和辅助函数中 | [workspace](src/workspace)、[共享 UI](src/ui.tsx) |
| 日期固定使用 `zh-CN`，错误文本可能直接透传 | [api.ts](src/workspace/api.ts) |
| 主窗口与快捷窗口分别初始化 | [main.tsx](src/main.tsx)、[desktopApi.ts](src/workspace/desktopApi.ts) |
| 原生菜单、导出和备份说明写死中文 | [desktop.rs](src-tauri/src/desktop.rs)、[workspace.rs](src-tauri/src/workspace.rs)、[backup.rs](src-tauri/src/backup.rs) |
| 多数 IPC 返回字符串错误，部分读取接口已有错误码 | [workspace.rs](src-tauri/src/workspace.rs)、[核心错误](crates/memivy-core/src/memory/mod.rs) |
| 整理任务的 `reason` 混合保存系统提示和模型解释 | [organization.rs](crates/memivy-core/src/memory/organization.rs)、[records.rs](crates/memivy-core/src/memory/records.rs)、[OrganizationReceipt.tsx](src/workspace/OrganizationReceipt.tsx) |
| 编辑器随 `label` 变化重建，格式栏还有独立 React root | [MarkdownEditor.tsx](src/workspace/MarkdownEditor.tsx)、[editorExtensions.ts](src/workspace/editorExtensions.ts) |
| 问答有 Agent／基本两条路径，持久化消息还会拼接中文标题 | [discussion.rs](crates/memivy-core/src/memory/discussion.rs) |

以上为文档整理时的代码检查结果；实施时以最新源码为准，不覆盖并行工作的改动。

## 3. 语言选择与同步

设置提供「跟随系统 / 简体中文 / English」。保存的是用户选择，例如 `system`，而不是把当时解析出的语言写成永久选择。

语言位于设置第一项「通用」，使用紧凑的三选一控件与浅黄色选中态；同页按界面语言、启动、快捷入口分组。普通设置入口默认打开通用页，模型配置和语音快捷键入口仍直达对应区域。

- Rust 统一管理本机语言偏好，使用当前资料库目录下独立的 `ui-preferences.json`，复用现有原子配置读写方式；不为语言新增记忆表，不混入 `models.json` 或仅属于快捷入口的 `desktop.json`。沿用当前整库备份的内容范围，语言偏好不随数据库备份恢复；隔离测试不修改日常资料库的偏好。
- 原生端是已保存语言选择的唯一权威；原生 WebView 不再用 localStorage 或浏览器检测器缓存另一份语言选择。每个 WebView 有自己的 i18next 实例，跨窗口同步的是偏好和实际语言代码；同一 WebView 内的主 React 根与编辑器独立根共享该实例。
- 跟随系统时读取语言偏好列表，按顺序匹配支持的语言；明确处理 `zh-Hans`、`zh-CN`、`zh-SG` 和 `en-*` 等变体。泛中文 `zh` 使用简体中文；繁体中文不声明为已支持，未支持的语言继续尝试下一偏好，最终回退英文。
- macOS 优先复用已有 `objc2-foundation` 接线读取 `NSLocale.preferredLanguages`，不为此单独增加 OS 插件。Tauri OS 插件的 `locale()` 只承诺返回单个 BCP-47 标签，不能替代本方案的偏好列表匹配。语言清单统一使用 `en`、`zh-CN`，共享别名与匹配测试样例；原生前端直接采用 Rust 返回的实际语言，不再自行二次解析。
- 两个窗口在首次显示前完成语言初始化，并更新 HTML 的 `lang` 属性，避免先显示中文再跳到英文。
- 初始化读取偏好失败时，使用内嵌英文资源显示可用界面和重试提示；不无限等待，不把失败后的默认值回写到原配置。菜单更新失败也应保留可用界面并允许重试，不因本地化失败退出应用。
- 手动切换成功持久化后，通知主窗口、快捷窗口和原生菜单更新；写入失败保留原选择并显示本地化错误。
- 先注册语言事件，再读取当前快照；用 revision 拒绝迟到响应，避免旧快照覆盖新选择。新打开或重新加载的窗口也必须重新读取快照，不能只依赖过去的事件。
- 启动及回到前台时重新读取系统偏好；是否能观察到运行中的系统语言变化按实际 macOS 行为验证，不额外承诺系统设置全部即时生效。
- 语言事件只更新呈现，不复用全库刷新，不重新发起问答、整理、模型加载或录音。
- 只读浏览器预览使用独立初始化路径，从浏览器语言偏好解析并允许本地预览切换，不等待不存在的 Rust IPC。

## 4. 翻译资源与格式化

使用稳定的语义 key，例如 `memory.save`，不以中文句子作为业务标识。资源放在仓库根目录的 `locales/`，按 `en/`、`zh-CN/` 分语言，再分 `common.json`、`workspace.json`、`settings.json`、`editor.json`、`errors.json` 和 `native.json`。英文资源是完整的 key 与回退基准，中文保留现有用词；复数分支按各语言规则校验，不要求所有语言拥有相同数量的复数后缀。

首批两种语言全部静态打包并在界面挂载前准备完成，不引入 HTTP backend、翻译下载或按语言懒加载。显式配置 `supportedLngs`、英文 `fallbackLng`、`returnEmptyString: false` 和 `saveMissing: false`；原生端完成语言归一化后只载入该语言资源，避免运行库另走一套地域回退。React 使用预载资源且不因切换语言进入 Suspense，避免编辑区被暂时替换。

前端通过 `useTranslation` 订阅语言变化。模块常量保存 key，在渲染时取译文，避免模块加载时固定中文；状态和缓存优先保存语义值，不保存供下次切换继续使用的已翻译标签。普通 React 根与编辑器独立根显式使用同一 i18n 实例。

Rust 从同一 `locales/` 下编译内嵌 `native.json`，只处理自有菜单和原生说明等少量文本；本期复用 `serde_json`，不再增加独立 Rust 翻译框架、YAML 资源体系或翻译 crate。复数等界面复杂格式交给 i18next，核心业务返回错误码和参数；AI 的本轮语言上下文不读取可变的全局原生语言。Apple 的 `InfoPlist.strings` 保持为专用打包资源，并检查其语言覆盖与语言清单一致。

类型约束优先采用 i18next 官方 `CustomTypeOptions` 与资源类型推导，不重写 `react-i18next` 的函数声明，也不照搬大型项目的类型生成工具。新增只读 `i18n:check` 检查 key、空白译文、插值和原生资源覆盖；检查失败阻止交付，不自动删除疑似未使用 key。动态 key 必须有明确的合法值集合。

| 类型 | 规则 |
|---|---|
| 普通文案 | 翻译完整句子，使用具名插值，避免拼接词语和标点 |
| 数量 | 使用 i18next 复数规则，覆盖 0、1 和多项 |
| 日期、数字与大小 | 统一封装 `Intl` 格式化；日期文字随界面语言，保留原时间戳、本地时区和现有 24 小时制。本期不增加独立地区／时间格式设置 |
| 无障碍文字 | 覆盖 `aria-label`、标题、占位符和图标提示 |
| 缺失翻译 | 运行时回退英文；开发检查发现缺失 key、错误插值和遗漏文案 |
| 品牌、模型名、路径与协议字段 | 保留名称和实际值；不翻译 MCP 工具名、参数名或机器状态码 |

## 5. 错误、回执与持久化文本

仅替换前端字符串不足以完成本地化：当前系统提示可能由 Rust 返回，也可能已作为整理任务原因写入数据库。

- 面向界面的新错误和固定系统状态统一使用稳定 `code` 与必要参数，渲染时翻译。基于现有核心错误枚举和读取错误结构扩展，不通过匹配中文句子反推错误码。
- 明确区分系统状态和模型生成解释。系统状态随界面语言呈现；模型解释、用户文字及历史文本保留生成或保存时的原文。
- 不批量翻译已有 `reason`、历史消息或记忆。若为区分新系统状态需要补充字段，使用现有数据库机制和备份路径，不创建历史内容迁移产品。
- 错误映射保留现有业务含义，例如冲突、不可用、重试和写入未确认，不能全部降为同一条通用提示。
- 未知错误使用本地化的安全兜底文案；不把模型凭据、SQL、原始内容或第三方响应拼入提示。

## 6. AI 与内容语言

界面语言和内容语言分别处理，不增加独立的语言检测模型请求。

| 场景 | 语言规则 |
|---|---|
| 问答 | 优先遵从用户明确的语言要求，其次跟随问题语言；无法判断时参考讨论上下文，再使用界面语言作为兜底 |
| 整理、润色 | 保留原文语言；更新已有记忆时保持目标正文语言，不因界面切换自动翻译 |
| 语音转写 | 保留说话内容的语言，界面切换不改变识别任务和模型绑定 |
| 已保存内容 | 原始输入、记忆、版本、讨论和已确认结论保留原文 |

每轮提交时固定本轮语言上下文，切换界面语言不取消或改变在途回答。系统提示词可继续统一维护，明确输出规则即可。

规则必须同时覆盖 Agent 与基本问答路径，以及程序添加的缺少依据提示、章节标题等派生文本。新持久化消息的派生文字遵守本轮语言上下文，避免英文回答在写入 `messages.text` 时又被加入中文标题；历史消息不重写。

检索始终围绕问题和证据，保留人物、项目等实体原名。检查基本问答中偏中文的检索词提示，补充英文与中英混合回归；不把界面语言用作限制检索内容的条件。

## 7. 编辑器与原生边界

### 编辑器

语言或翻译后的 `label` 变化不得触发编辑器销毁、重新创建或正文重新解析。通过原地更新 ProseMirror 属性和插件呈现更新文案，保留草稿、选区、滚动位置、undo 历史和中文 IME 状态。

必须覆盖三类入口：主界面的 React 文案、独立 `createRoot` 创建的格式工具栏、非 React 的 DOM／表格／链接插件文案。仅移除 `label` 的 effect 依赖不算完成。

### macOS

| 界面 | 切换约定 |
|---|---|
| 应用自有菜单、快捷窗口标题和新打开面板的说明文字 | 使用当前应用语言，原生更新遵守主线程要求 |
| AppKit 的 Open／Save／Cancel 等标准控件 | 遵循 macOS 与 bundle 的语言选择，不承诺随应用内开关立即变化 |
| 麦克风权限说明 | 按语言打包 `InfoPlist.strings`，由系统选择；不承诺已出现的权限弹窗热切换 |

应用内中英文选项不能代替安装包的本地化资源声明。实际 `.app` 包和原生窗口均需验证。

## 8. 实施顺序与验收

| 步骤 | 交付内容 | 完成条件 |
|---|---|---|
| 1. 语言基础 | 依赖、资源、语言设置、Rust 偏好、双窗口与浏览器初始化 | 中英文切换、回退、持久化、失败恢复和迟到响应验证通过 |
| 2. 完整界面 | 全部用户可见文案、错误／状态、编辑器、原生菜单与 bundle 资源 | 两种语言覆盖主要路径，切换保持交互状态，原生边界验证通过 |
| 3. AI 与回归 | 两条问答路径、本轮语言上下文、派生文本和多语言检索规则 | 合成问答、整理、检索及在途切换回归通过；记录实际原生结果 |

验收重点：

- 资源检查：缺失 key、空白译文、插值参数、复数分支、固定中文残留和英文回退；用户样例及原始内容不能被误当成漏翻译。中英文自有文案和原生菜单需有完整资源，不能以回退掩盖首批语言的漏翻译。
- 语言解析：偏好列表第一项不支持但后续支持、`en-GB`、`zh-Hans-SG`、`zh-Hant`、大小写／下划线归一化、未知语言和无偏好；原生与浏览器预览使用同一组期望结果验证。
- 保留既有行为测试：原有依赖中文按钮或标签的夹具固定中文语言，继续验证保存、冲突、取消和草稿规则。
- 新增真实语言切换测试：使用真实 i18n 实例及 React 渲染，不能只把 `t()` mock 成原文；覆盖独立工具栏和非 React 插件。
- 状态稳定性：切换后编辑器实例、正文、选区、undo、阅读滚动与表单草稿保留；确认、取消和撤销语义不变。
- 双窗口：启动、隐藏后恢复、窗口重新加载、重复切换及迟到快照；切换不触发记忆查询失效或重复业务请求。另测偏好读取／保存失败和菜单更新失败，确认界面可用、原配置不被默认值覆盖。
- 在途操作：回答生成、整理和录音过程中切换，任务继续按提交时的上下文运行，结果及未提交内容不丢失。
- 原生视觉：中英文分别覆盖常用窗口和最小窗口尺寸，检查长文案、菜单、文件面板及麦克风权限资源；中文输入在目标应用中验证。
- AI：使用合成资料覆盖中文、英文、中英混合与跨语言提问，核对实体检索、引用、生成语言、原文保留和未确认不保存。

实施时运行相关 UI／Rust 回归、`npm run build`、Rust 格式与 clippy 检查，再构建并检查原生应用。既有 [DOM 稳定性测试](tests/markdown-dom.test.mjs)、[编辑器格式测试](tests/editor-formatting.test.mjs)、[资源缓存测试](tests/resources.test.mjs) 和 [原生视觉合同](tests/assets/visual_contract.json) 可复用。真实模型测试仅使用明确配置与合成资料。

实施与验证结果写回 [DEVELOPMENT_PLAN.md](DEVELOPMENT_PLAN.md)，测试证据放在忽略的 `research/`。实际运行结果以 DEVELOPMENT_PLAN.md 的 I18N 记录为准。

## 9. 独立 Review 修订记录

2026-09-12 独立子代理审阅后，保留 i18next 选型，并补齐以下必修问题；修订已由同一审阅者复核，方案层面未发现剩余阻塞项。

1. 编辑器增加独立 React root 和非 React 插件的语言同步，避免重建与正文重新解析。
2. 将持久化系统状态与模型解释分开处理，不能只修改 IPC 错误；保留历史原文。
3. AI 覆盖 Agent／基本问答及程序派生消息，固定本轮语言上下文，保留检索实体原名。
4. 收窄原生热切换承诺，区分自有菜单／说明与系统标准控件／权限弹窗。
5. 补充系统语言偏好列表、快照次序、迟到响应、浏览器初始化和不触发业务刷新的规则。

本记录是方案审阅证据，不是代码 Review 或测试通过记录。

## 10. 大项目源码核对与补充决策

2026-09-12 读取以下项目默认分支的固定提交，检查了前端初始化、语言配置、菜单或类型检查相关源码。来源链接固定到对应提交；结论只表示源码中的实现方式，不表示已安装运行这些应用或验证其所有语言行为。Pot 使用 Tauri 1.8，只借鉴配置和事件分工，不复制旧 API。

| 项目与源码快照 | 实际做法 | Memivy 采用的部分 |
|---|---|---|
| Clash Verge Rev，Tauri 2.11.5，`f624ffc382a0` | React 使用 i18next；Rust 有单独的 rust-i18n 资源；配置语言变化会更新原生语言和托盘；按区块加载前端资源，并有 key 类型生成 | 保留前端／原生分工、语言变化更新菜单和资源检查；Memivy 首批全量内嵌资源，使用官方类型推导，原生资源沿用同一 JSON 目录。[前端](https://github.com/clash-verge-rev/clash-verge-rev/blob/f624ffc382a03fea21d39e7130000bce96678189/src/services/i18n.ts)、[原生](https://github.com/clash-verge-rev/clash-verge-rev/blob/f624ffc382a03fea21d39e7130000bce96678189/crates/clash-verge-i18n/src/lib.rs)、[配置联动](https://github.com/clash-verge-rev/clash-verge-rev/blob/f624ffc382a03fea21d39e7130000bce96678189/src-tauri/src/feat/config.rs) |
| Pot，Tauri 1.8，`594d32ede96a` | React 使用 i18next 和本地资源；共享配置通过事件同步窗口，切换语言另行调用 Rust 更新托盘 | 采用“持久化配置 + 跨窗口事件 + 原生菜单更新”；结合 Memivy 既有快照序号处理迟到事件，不复制每种语言各写一套菜单的实现。[资源](https://github.com/pot-app/pot-desktop/blob/594d32ede96acd106b0256deaa8bb440ffcdff40/src/i18n/index.jsx)、[同步](https://github.com/pot-app/pot-desktop/blob/594d32ede96acd106b0256deaa8bb440ffcdff40/src/hooks/useConfig.jsx)、[菜单调用](https://github.com/pot-app/pot-desktop/blob/594d32ede96acd106b0256deaa8bb440ffcdff40/src/window/Config/pages/General/index.jsx) |
| NextAI Translator，原 OpenAI Translator，Tauri 2.8.5，`a9681a4ab059` | React 使用 i18next、本地 JSON 和英文回退；浏览器检测器关闭独立缓存，桌面窗口按设置更新语言；所查托盘仍使用固定英文 | 采用本地资源和设置驱动；原生菜单需独立完成，不能把前端有 i18next 当成整包本地化。其浏览器扩展共用的 detector／HTTP backend 不引入 Memivy。[初始化](https://github.com/nextai-translator/nextai-translator/blob/a9681a4ab0599bef7013e29fca701f250d7738a2/src/common/i18n.js)、[窗口](https://github.com/nextai-translator/nextai-translator/blob/a9681a4ab0599bef7013e29fca701f250d7738a2/src/tauri/components/Window.tsx)、[托盘](https://github.com/nextai-translator/nextai-translator/blob/a9681a4ab0599bef7013e29fca701f250d7738a2/src-tauri/src/tray.rs) |
| Hoppscotch，Tauri 2，`ac145e7f7581` | 共用前端使用 vue-i18n，偏好优先、浏览器语言其次、英文回退；其他语言按需加载。桌面端由 Tauri 承载 | 说明采用与前端框架匹配的成熟库可行；Memivy 沿用 React 的 i18next。其 Web／桌面共用的配置和懒加载机制不直接替代 Memivy 原生偏好。[语言模块](https://github.com/hoppscotch/hoppscotch/blob/ac145e7f758151b41fd46d3e5f513886ce9068ba/packages/hoppscotch-common/src/modules/i18n.ts)、[桌面依赖](https://github.com/hoppscotch/hoppscotch/blob/ac145e7f758151b41fd46d3e5f513886ce9068ba/packages/hoppscotch-desktop/src-tauri/Cargo.toml) |

本轮补定的技术决策已并入第 3、4、8 节：每个 WebView 一个实例、Rust 偏好为唯一权威、原生与前端统一语言清单、两种语言全量内嵌、简单原生资源共用 JSON，以及完整资源检查和初始化失败处理。Rust 管理偏好是基于 Memivy 双窗口、原生菜单及现有配置机制的选择，不将任一上游实现宣称为 Tauri 唯一标准。

实施前无需再扩大框架选型。实施中需完成两项针对性验证：macOS 的系统／单应用语言设置实际如何反映到偏好列表和系统窗口；Milkdown 表格、链接及独立工具栏能否在不重建编辑器的情况下更新文案。验证结果写入开发计划，不能用上游项目存在类似功能代替本机证据。

## 11. 官方参考

- [react-i18next：useTranslation](https://react.i18next.com/latest/usetranslation-hook) 与 [共享 i18next 实例](https://react.i18next.com/latest/i18next-instance)：React 文案更新和独立 root 的接入依据。
- [i18next：TypeScript](https://www.i18next.com/overview/typescript)、[复数](https://www.i18next.com/translation-function/plurals) 与 [回退](https://www.i18next.com/principles/fallback)：资源约束和格式规则依据。
- [Apple：preferredLanguages](https://developer.apple.com/documentation/foundation/nslocale/preferredlanguages)：按用户语言偏好顺序匹配。
- [Tauri：Info.plist localization](https://v2.tauri.app/distribute/macos-application-bundle/#infoplist-localization)：通过 `.lproj/InfoPlist.strings` 打包权限说明。
- [Tauri：Process Model](https://v2.tauri.app/concept/process-model/)：Core 与 WebView 的运行边界；窗口之间通过消息同步状态。
- [Tauri：OS locale](https://v2.tauri.app/reference/javascript/os/#locale)：接口返回单个语言标签，无法直接表达完整偏好列表。
- [i18next：配置选项](https://www.i18next.com/overview/configuration-options)：支持语言、回退、空译文和缺失 key 行为。
