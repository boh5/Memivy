# Contributing

Bug reports, suggestions and focused pull requests are welcome.

For a bug, include your Memivy version, macOS version, Mac chip, steps to reproduce,
and what you expected to happen. Use made-up examples instead of private notes.
For a larger change, open an issue first to discuss the problem and approach.
Report security issues through [Security](SECURITY.md).

## Run and check your changes

Follow [Build from source](README.md#build-from-source) to start the app with a
separate development library. Use test data for edits, deletion and restore checks;
keep your everyday library and shared model cache intact.

For code changes, run:

```sh
npm run i18n:check
npm run test:core-assets
python3 scripts/verify_restore.py
```

The test suite builds the examples needed by the restore check and does not require
model credentials. If you change model behavior, also try it with a configured model
and made-up examples. For input, window or packaging changes, test the desktop app.
For documentation-only changes, check links and instructions.

## Submit a pull request

Explain the problem, what changed, and how you checked it. Mention anything you
could not test. Keep the change focused and preserve original notes, edit history
and undo. Do not include private data or credentials.

Use Conventional Commits, for example `fix: preserve the draft when closing a window`.
Write code and documentation in English; translations belong in the i18n files or
Chinese README. Contributions to Memivy's code use the project's MIT license.
