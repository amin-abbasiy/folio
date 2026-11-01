# Changelog

## 0.1.1 — 2026-10-28

- Added a visible **Copy note** button and **Ctrl+Shift+C** to copy the complete title and Markdown text, including unsaved edits. **Ctrl+C** still copies only the highlighted text.
- Fixed continuous scrolling when holding a text selection near or beyond either vertical edge of the note pane, in both writing and reading views.
- Preserved reading-view selection anchors and selected paragraphs that have scrolled offscreen, including Unicode text and code blocks.
- Kept drag scrolling gradual instead of jumping to the end of a long note. Scrolling stops on release or focus loss.
- Added clipboard and long-note selection regression tests. The notebook format and existing notes are unchanged.

## 0.1.0 — 2026-10-26

Initial local-only Rust desktop app with folders, search, pinned notes, Markdown editing/reading, checklists, autosave, trash recovery, import/export, and local backups.
