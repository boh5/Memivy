<p align="center"><img src="design-demo/brand/memivy-logo.svg" alt="Memivy" width="240"></p>

# Memivy

macOS 上的个人记忆工具：**记一下、问一问、接着想**。
随手保存想法，向 AI 询问自己记过的内容，继续展开讨论。
记忆和对话保存在你的 Mac 上，AI 功能使用你选择的模型服务。

[English](README.md) · [下载](https://github.com/boh5/memivy/releases/latest) ·
[安装指南（英文）](docs/INSTALL.md) · [隐私说明（英文）](docs/PRIVACY.md)

## 能做什么

- 记录和编辑笔记，支持 Markdown 排版，保留原话和历史版本。
- 向 AI 询问记过的内容、继续讨论。查看 AI 改了什么，不满意可以撤销。
- 在桌面快捷窗口随手记录，再回到主窗口继续。
- 使用语音输入，或按意思查找相关记忆；可选择本地模型或自行配置的服务。
- 用置顶和专题整理记忆，备份资料库。
- 连接支持 MCP 的 AI 工具，让它们保存和搜索记忆。

## 开始使用

需要 **Apple Silicon（M 系列芯片）的 Mac，运行 macOS 26 或更新系统**。

1. 从[下载页](https://github.com/boh5/memivy/releases/latest)下载 DMG。
2. 打开 DMG，将 Memivy 拖入“应用程序”。
3. 从“应用程序”打开 Memivy，开始记录。

Memivy **未经 Apple 公证**。如果首次打开时被 macOS 拦截，先尝试打开一次，
再前往“系统设置 → 隐私与安全性”，找到 Memivy 并选择“仍要打开”。
详见[安装指南（英文）](docs/INSTALL.md#when-macos-blocks-the-app)。

保存和编辑笔记不需要注册账号或配置模型。与 AI 聊天前，需要在设置中配置兼容的模型服务。
本地语音和搜索模型可按需下载。如果使用远程服务，相应功能需要的文字或音频会发送给服务商，
详见[隐私说明（英文）](docs/PRIVACY.md)。

## 从源码运行

需要 Apple Silicon、macOS 26+、macOS 26+ SDK 和 Xcode 命令行工具，
以及 Node **24.12.0**、Rust **1.98.1**、Python 3.11+ 和 CMake。
确保 `cargo` 和 `cmake` 已加入 PATH。

```sh
git clone https://github.com/boh5/memivy.git
cd memivy
npm ci
cargo fetch --locked
MEMIVY_DATA_DIR="$(mktemp -d /tmp/memivy-dev.XXXXXX)" npm run dev:app
```

上述命令会新建独立的开发资料库，不影响日常笔记；下载的模型仍使用 Memivy 共用缓存。
如果之后要继续使用这个开发资料库，请记下临时目录路径。
`npm run dev` 只启动浏览器预览，桌面应用请使用 `dev:app`。

运行 `npm run build:release` 可生成 DMG，安装包和校验文件位于
`target/release/bundle/dmg/`。测试命令见[贡献指南（英文）](CONTRIBUTING.md)，
发布步骤见[发布流程（英文）](docs/RELEASING.md)。

## 许可证

[MIT](LICENSE)，Copyright (c) 2026 Huang Bo。
