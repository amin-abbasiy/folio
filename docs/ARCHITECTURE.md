# Implementation decisions

Folio is a single-process Rust desktop app. `eframe`/`egui` provide a native window with an OpenGL renderer, not a browser or web server. The application enables only bundled fonts, OpenGL, X11, and Wayland features; the framework's optional network inspection feature is not enabled.

## Document lifecycle

1. SQLite is the authoritative store. The selected note is cloned into an editable in-memory draft.
2. Editing marks the draft dirty. A 450 ms idle timer saves it; lifecycle operations also flush it immediately.
3. A successful write updates list metadata and clears the dirty state. A failed write keeps the draft, reports the error, and blocks navigation and normal window closure. Export can still rescue the in-memory draft to a different writable path.
4. Replacing a draft resets its text-editor undo/cursor state so one note's undo history cannot leak into another note.

The UI queues actions until after the current frame's text input has been processed. Close and focus-loss handling also run after text input. This avoids saving an older draft during a same-frame switch or normal close. Background logic continues to service the autosave timer when the window is hidden.

## Persistence

SQLite uses WAL, foreign keys, a busy timeout, and `synchronous=FULL`. Single-statement mutations are atomic; schema initialization and welcome content use explicit transactions. The schema is versioned with `user_version`; a newer schema is rejected rather than migrated backward.

A private advisory-lock file prevents multiple Folio instances from opening the same notebook. This is a local-process lock, not coordination for network filesystems or a defense against other programs deliberately editing the database.

Folders reference notes through nullable foreign keys with `ON DELETE SET NULL`; deleting a folder cannot cascade into document deletion. Trash is a timestamp, not a separate copy. Permanent-delete queries are deliberately restricted to trashed notes at the storage layer, independent of UI confirmation.

Search uses literal Unicode-lowercased term matching across title and body, scoped to a collection. All terms must match; SQL metacharacters and quotes have no special meaning. For simplicity, list queries currently load full note bodies; a very large library would benefit from separate summary queries and a search index.

## Import, export, and backup

Imports are bounded UTF-8 text reads from regular files. A leading Markdown H1 becomes the title; otherwise the filename is used. Exports add a title heading and preserve Markdown body content. Export/backup files are created privately with create-new semantics and never overwrite an existing file.

Backups use SQLite's online-backup API rather than copying an active database file. An automatic snapshot is made on the first launch each UTC day. Backups are retained, including historical and trashed content; nothing silently prunes user recovery data.

## Rendering and trust

Markdown is parsed into inert text blocks. Formatting, lists, code, quotes, and interactive tasks are rendered without a web engine. Task changes use verified parser-provided source ranges. HTML never runs, images never load, and links are copied rather than launched. No application code initiates network connections or invokes shells.

The UI stack uses the maintained Skrifa/Fontations font parser, avoiding the unmaintained `ttf-parser` dependency identified during the initial audit. Static Inter fonts are bundled with their licenses; runtime font downloading is absent.

See `SECURITY.md` for the limits: this is not encryption, secure erasure, or an operating-system sandbox.

## Copying and selection

Whole-note copy reads the live draft, not the saved database row, and does not require a successful save. It copies the displayed title, a blank line, and the Markdown body without trimming body whitespace. The global shortcut consumes any duplicate backend Copy event so a partial selection cannot overwrite the whole-note payload.

Reading-view text keeps all paragraph anchors in egui's selection model, even when offscreen; painting remains clipped to the note pane. This avoids the stock label behavior that forgets an offscreen selection endpoint. A note-scoped pointer capture keeps a held selection at the viewport edge, while frame-timed scrolling reveals more content gradually. It applies only to a drag that started on note text, stops on release/focus loss, and resets when changing notes or views. Writing-view text uses the same edge scrolling with the existing native text editor selection.
