<p align="center"><img src="design-demo/brand/memivy-logo.svg" alt="Memivy" width="240"></p>

# Memivy, your personal AI memory assistant

**Tell Memivy what's on your mind.** Press a shortcut and talk. Memivy remembers your ideas and keeps your memories organized, without folders or categories to manage.

When you want to look back, just ask. Memivy helps you find your earlier thoughts and pick up where you left off.

[简体中文](README.zh-CN.md) · [Download](https://github.com/boh5/memivy/releases/latest) · [Installation and setup](#get-started)

## Features

- **Start with a shortcut**: Speak or type whenever you have an idea to remember.
- **Pick up where you left off**: Memivy draws on your past memories as you talk, remembering new ideas and changes in your thinking.
- **Find memories in your own words**: Describe what you remember to find related thoughts and records, even when you've forgotten the exact wording.
- **Choose your own model (BYOM)**: Connect your preferred AI model service and keep the same memory library when you switch models.
- **Stored on your Mac**: Your memories and conversations are stored on your Mac. No Memivy account is required.
- **Let other AI assistants use your memories**: Through MCP, other AI assistants can save and search memories in Memivy.

## Get started

You need **an Apple Silicon (M-series) Mac running macOS 26 or later**.

### Install

1. Download the latest DMG from [Releases](https://github.com/boh5/memivy/releases/latest).
2. Open the DMG and drag Memivy into Applications.
3. Eject the disk image, then open Memivy from Applications.

Memivy has not been notarized by Apple. If macOS says it cannot verify the developer or check the app for malicious software, open **System Settings → Privacy & Security**, find the message about Memivy, and click **Open Anyway**. Confirm when prompted. See [Apple's instructions](https://support.apple.com/102445) for the system prompts.

<details>
<summary>Verify the download (optional)</summary>

Download the DMG's matching `.sha256` file and put both files in the same folder. Open Terminal, change to that folder, and run:

```sh
shasum -a 256 -c Memivy_*.dmg.sha256
```

A result ending in `OK` means the file passed verification.

</details>

### Set up your AI assistant

1. Open **Settings → AI & models → AI assistant**.
2. Follow your provider's instructions to choose the API type: **OpenAI Compatible**, **OpenAI Responses**, **Anthropic**, or **Google Gemini**.
3. Enter the **API address**, **Model ID**, and **API Key** from your provider.
4. Click **Test connection**, then **Save settings** once the test passes.

Choose a model that supports tool calling so Memivy can find and organize your memories. Your model provider handles API usage and billing.

### Enable local search and voice input

Return to **Settings → AI & models** and set up both features:

1. Open **Semantic search**, choose **On this Mac**, click **Download and enable**, then **Confirm and start**. Wait for the model download and search preparation to finish.
2. Open **Voice input**, choose **On this Mac**, and click **Download model**. Once the download finishes, click **Enable Voice input**.

Keep Memivy open during the first download. The search model is about 640 MB and the speech model about 1.02 GB. Once downloaded, both features run on your Mac.

### Start a conversation

1. Press the default voice shortcut, **Option+R**, to open the quick window and start recording. Say what's on your mind.
2. Press **Option+R** again to stop recording and wait for transcription to finish.
3. Review the text, then press **Command+Enter** to send it.

Allow microphone access when prompted the first time you record. To type instead, press **Option+M** to open the quick window. **Enter** adds a new line.

To revisit something, press **Option+R** and ask. Describe what you remember; you don't need to find the original conversation first.

## Connect other AI apps

Through MCP, your other agents can save and search memories in Memivy. Send your agent the setup prompt and let it configure the connection for you.

1. Open **Settings → External access** and turn on **Allow other AI apps to save and search memories**.
2. Click **Copy setup prompt** and send it directly to the agent you want to connect. The prompt already includes the Memivy configuration for this Mac.
3. The prompt asks your agent to add the MCP configuration and check the connection. Follow its instructions if it needs a restart or additional permissions.

The other app must support local MCP servers (stdio), and the agent needs permission to edit its configuration. You can also expand **Manual configuration** to set up the connection yourself. If you move Memivy later, copy the prompt again and ask your agent to update the configuration. To stop access, turn off the switch in Memivy's **External access** settings.

## Data and models

Memories and conversations are stored on your Mac. No Memivy account is required. Your library and settings are in `~/Library/Application Support/com.memivy.app/`; downloaded models are in `~/Library/Caches/com.memivy.app/models/`.

Local voice input and semantic search process content on your Mac. Downloading the models connects to Hugging Face. When you chat with a remote AI model, your messages and relevant memories are sent to your chosen provider. If you switch voice input or semantic search to an API service, that provider receives the audio or text it needs to process. Other AI apps connected through MCP may also send search results to their own model providers.

Memivy does not encrypt the library or model configuration. API keys are stored separately in a local `models.json` file, not in macOS Keychain. Keep these files out of shared attachments and bug reports.

## Back up, update, and uninstall

<details>
<summary>Back up and restore</summary>

Open **Settings → Data & storage** and click **Create backup**. Backups include memories, conversations, drafts, original inputs, version history, and Trash. They exclude API keys, model settings, downloaded models, and temporary recordings.

To restore, click **Choose backup**, review its contents, and confirm. Memivy backs up the current library first, then replaces it with the selected backup and restarts. Model settings and the MCP switch keep their current values.

</details>

<details>
<summary>Update</summary>

1. Create a backup in **Settings → Data & storage**.
2. Quit Memivy. If other AI apps are connected, disconnect their Memivy MCP connections too.
3. Download the new DMG and replace Memivy in Applications with the new version.
4. Reopen Memivy, check that your memories and drafts are there, then reconnect your other AI apps.

Updates are currently installed manually. Keep your backup from before the update; an older version may not open a library updated by a newer version.

</details>

<details>
<summary>Uninstall</summary>

Turn off MCP access and remove Memivy's configuration from your other AI apps. Then quit Memivy and move it from Applications to Trash.

Removing the app leaves your library, settings, and downloaded models in place. To remove those too, back up anything you want to keep, then delete the two folders listed above. Memivy and Memivy Dev share the model cache. Backups saved elsewhere must be removed separately.

</details>

## Build from source

The [contributing guide](CONTRIBUTING.md#run-the-app-locally) covers dependencies, startup commands, and testing. The development app uses a separate library and can be installed alongside Memivy.

## Feedback and contributions

[Open an issue](https://github.com/boh5/memivy/issues/new/choose) if something isn't working or you have a suggestion.

Fixes, documentation improvements, and translations are welcome. Read the [contributing guide](CONTRIBUTING.md) before sending a PR.

Please report security issues [privately](SECURITY.md).

## License

[MIT](LICENSE) · Copyright (c) 2026 Huang Bo.
