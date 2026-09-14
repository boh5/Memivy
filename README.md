<p align="center"><img src="design-demo/brand/memivy-logo.svg" alt="Memivy" width="240"></p>

# Memivy

A personal memory app for macOS. Save ideas, ask questions about things you've
saved, and explore them with AI. Your memories and conversations stay on your Mac;
AI features use the model services you choose.

[Simplified Chinese](README.zh-CN.md) · [Download](https://github.com/boh5/memivy/releases/latest) ·
[Installation](docs/INSTALL.md) · [Privacy](docs/PRIVACY.md)

## What you can do

- Write and edit notes with Markdown formatting. Keep your original words and earlier versions.
- Ask AI about your saved memories and continue the conversation. See what AI changes and undo it when needed.
- Capture an idea in the desktop quick window, then continue in the main window.
- Use voice input and optional search by meaning, with local models or a provider you configure.
- Organize memories with pins and collections, and back up your library.
- Connect AI tools that support MCP to save and search memories.

## Get started

Requires **an Apple Silicon Mac (M-series) running macOS 26 or later**.

1. Download the DMG from [Releases](https://github.com/boh5/memivy/releases/latest).
2. Open it and drag Memivy into Applications.
3. Open Memivy from Applications and start saving notes.

Memivy has **not been notarized by Apple**. If macOS blocks the first launch,
follow the [installation guide](docs/INSTALL.md#when-macos-blocks-the-app).

You don't need an account or a model to save and edit notes. To chat with AI,
configure a compatible model service in Settings. Local voice and search models
are optional downloads. If you use a remote provider, the text or audio needed
for that feature is sent to it; see [Privacy](docs/PRIVACY.md).

## Build from source

Use an Apple Silicon Mac with macOS 26+, the macOS 26+ SDK and Xcode command-line
tools, Node **24.12.0**, Rust **1.98.1**, Python 3.11+, and CMake.
Make sure `cargo` and `cmake` are on PATH.

```sh
git clone https://github.com/boh5/memivy.git
cd memivy
npm ci
cargo fetch --locked
npm run dev:app
```

The app uses `~/Library/Application Support/com.memivy.app/` for its data,
including when running from source.
`npm run dev` alone opens a browser preview; use `dev:app` for the desktop app.

To build a DMG, run `npm run build:release`. The DMG and checksum are written to
`target/release/bundle/dmg/`. See [Contributing](CONTRIBUTING.md) for checks and
[Releasing](docs/RELEASING.md) for the release steps.

## License

[MIT](LICENSE) — Copyright (c) 2026 Huang Bo.
