# Data and privacy

Your library is stored on your Mac. AI features may send text or audio to the
services you configure, as described below.

| Feature | Data flow |
| --- | --- |
| Memories, conversations, sources, versions and drafts | SQLite in the local library |
| AI discussion and cleanup | Input and relevant retrieved context go to the configured model endpoint |
| Local embedding and speech | Inference runs on this Mac; model download contacts Hugging Face |
| Remote embedding | Indexed memory text and search queries go to the configured provider |
| Remote speech | Recorded audio is sent to the configured transcription endpoint |
| MCP | Connected AI tools can save and search your memories; they may send results to their own models |

You choose endpoints and credentials. Their operators have their own retention,
pricing and privacy policies. A localhost endpoint may itself forward requests;
check your provider. Memivy does not include an analytics or advertising SDK.

## Storage

- Library: `~/Library/Application Support/com.memivy.app/`, including `memivy.db`.
- Model credentials: separate local `models.json`, not macOS Keychain. This is not
  an encrypted secret vault. Neither the database nor model configuration provides
  application-level encryption. Protect the user account and device.
- Local model cache: `~/Library/Caches/com.memivy.app/models/`, shared across
  development/test and installed copies. Optional fixed weights total about 1.66 GB
  for embedding and speech together, excluding download staging and indexes.
- Voice recordings: temporarily saved in the library's `voice/` folder while
  transcribing. Unfinished recordings stay there so you can retry; Memivy removes
  them after saving the completed text to your draft or when you discard the recording.
- In-app backups: content database only, excluding separate credentials and weights.
  Temporary voice recordings are also excluded. Backups still contain private
  conversations and source text.
  A manual copy of the whole application-data directory can include credentials.

Deleting a test library does not remove the shared model cache.

## AI changes and deletion

The assistant can automatically maintain useful information you express. Check
what it changed and which notes it used, and undo changes when needed. AI may make mistakes.
Deleting a note moves it to Trash and excludes it from normal recall. You choose
when to empty Trash. Deleting a conversation removes that conversation directly;
it does not delete memories already saved from it or their saved source text.
Backups and copies you made earlier may still contain deleted information.

Never attach your database, `models.json`, recordings or unreviewed logs to a public
issue. Use made-up examples and follow [Security](../SECURITY.md) for vulnerabilities.
