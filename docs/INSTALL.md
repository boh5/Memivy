# Install Memivy

You need an Apple Silicon Mac (M-series) running macOS 26 or later.
You do not need developer tools to use the downloaded app.

1. Download the DMG from [the latest release](https://github.com/boh5/memivy/releases/latest).
2. Open the DMG and drag Memivy into Applications.
3. Eject the DMG and open Memivy from Applications.

## When macOS blocks the app

Memivy uses ad-hoc signing and has **not been notarized by Apple**. macOS may
block the first launch because it cannot verify the developer.

After attempting to open Memivy, go to **System Settings → Privacy & Security**
and choose **Open Anyway** for Memivy. Follow the confirmation prompts.
A managed work or school Mac may not allow this exception.
See [Apple's instructions](https://support.apple.com/102445).

## Start using Memivy

You can save and edit notes immediately, without an account or model setup.
To chat with AI, choose OpenAI Compatible, OpenAI Responses, Anthropic, or Google
Gemini in Settings. Enter the service URL, model ID, and API key, then save the
settings. You can optionally test the connection. Your provider may charge for usage.

Voice input and search by meaning are optional. Choose a local model or configure
a remote service in Settings. Local models download separately; keep Memivy open
until the download finishes. Voice input asks for microphone permission. You can
still type if you decline it.

To connect an AI tool that supports MCP, enable external connections in Settings
and copy the generated configuration into that tool. Connections are off by default.
If you move Memivy to another folder, copy the configuration again.

## Upgrade and back up

Memivy does not update automatically.

1. Create a backup in **Settings → Data**.
2. Quit Memivy. If you use MCP, stop its connection in your other AI tools too.
3. Replace Memivy in Applications with the new version.
4. Open it, check your notes and drafts, and reconnect your AI tools.

Keep your backup. An older app may not be able to open data saved by a newer version.
Before restoring a backup, Memivy also saves a copy of your current library.

Backups include your notes and conversations, but not API keys, model settings or
downloaded models. See [Privacy](https://github.com/boh5/memivy/blob/main/docs/PRIVACY.md)
for what is stored and shared.

## Uninstall

Quit Memivy and move it from Applications to Trash. If you connected other AI tools,
disable MCP and remove their Memivy configuration first.

Your notes and settings remain in `~/Library/Application Support/com.memivy.app/`.
Downloaded models remain in `~/Library/Caches/com.memivy.app/models/`.
To remove them too, back up anything you want to keep, stop Memivy and its MCP
connections, then delete those folders. The settings folder may contain API keys;
do not share it publicly.

## Optional: verify the download

Download the matching `.sha256` file into the same folder as the DMG. In Terminal,
open that folder and run:

```sh
shasum -a 256 -c Memivy_*.dmg.sha256
```

The result should end with `OK`. This checks that the downloaded file matches the
published checksum; it does not replace Apple's notarization.
