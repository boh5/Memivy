# Changelog

## 0.1.6

- Improve the desktop leaf: drag it to move, click to open the quick window, and use its native menu. Restore its saved position when reopening Memivy.
- Check for updates and download them automatically in the background. Choose when to restart and install, or turn automatic updates off in General settings.
- Keep update-setting warnings visible after manual checks and downloads until the setting is saved successfully.
- Apply security patches for DOMPurify 3.4.16 and source-map-js 1.2.2.

This release keeps database schema 2; no new database migration is required.

## 0.1.5

- Discuss topics with the Agent: find, read, create and edit collections, and add, remove or move explicitly selected memories between them.
- Review and undo collection changes alongside memory edits. Repeating an already-saved collection name or description now succeeds safely.
- Let the Agent retrieve relevant memories when needed, with multiple search queries, optional collection scope, and evidence-based memory edits and merges.
- Keep collection membership under your control: choosing a topic focuses the discussion without automatically filing memories or excluding relevant global information.
- Improve the update panel and release packaging, and upgrade the development toolchain to TypeScript 7.
- Upgrade schema 1 libraries directly to schema 2, with a private backup before migration. Preserve memories, original inputs, version history, drafts and existing collection memberships. Returning to an older app requires a compatible backup.
- MCP integration change: `memory_search` now accepts a `queries` array of objects with `text` and `keywords` instead of a single `query`. Clients that cache tool definitions should refresh them.

## 0.1.4

- Version-only release for verifying in-app updates from 0.1.3.
- Update through Settings → App updates; application features and database schema are unchanged.

## 0.1.3

- Check for updates in Settings, download a signed update, and confirm installation and restart.
- Preserve drafts before installation and defer updates while background work, recording, or MCP connections are active.
- Add migration backups and transactional database upgrades for future schema changes; this release keeps schema 1.

Users on 0.1.2 or earlier must install this version manually once to enable future in-app updates. Apple Silicon and macOS 26 or later only; ad-hoc signed and not notarized.

## 0.1.2

First public release.

- Save and edit notes on your Mac, with original text, earlier versions and undo for AI changes.
- Ask AI about your saved memories and continue the conversation.
- Capture ideas in the desktop quick window, use voice input and optionally search by meaning.
- Organize memories with pins and collections; back up and restore your library.
- Connect AI tools that support MCP to save and search memories.
- Chinese and English interfaces.

Apple Silicon and macOS 26 or later only. Ad-hoc signed; not notarized by Apple.
No automatic updater. See [installation and upgrade instructions (Chinese)](https://github.com/boh5/memivy/blob/main/README.zh-CN.md#开始使用).

## 0.1.1

Unpublished draft. Superseded by 0.1.2 after fixing main-application startup.

## 0.1.0

Unpublished release attempt. Superseded after fixing release-package verification.
