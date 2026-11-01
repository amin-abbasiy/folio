# Security and privacy

## Boundaries

- Folio contains no account, sync, telemetry, updater, web server, or application networking implementation.
- Markdown HTML is shown as inert text. Images are not read or fetched, including local file images. Link clicks copy addresses; no shell/browser command is executed.
- File operations use Rust filesystem APIs, not shell interpolation. User text is bound as SQLite parameters. Search input is literal text, not SQL or a query language.
- Import reads at most 8 MiB plus a sentinel byte, validates UTF-8, and does not execute content.
- Dedicated notebook and backup directories are `0700`; the database, lock, exports, and backups are private files. Folio refuses a symlink notebook directory, database, or lock file. This is not a sandbox against other processes with the same UID, root, hostile parent directories, or a compromised OS.
- An advisory lock prevents two Folio instances from editing the same notebook. This does not stop external SQLite tools that deliberately ignore the lock.
- SQLite foreign keys and durable transactions protect document state. Save errors preserve the current in-memory draft and block navigation/normal window closure. No software can guarantee survival of a forced kill, hardware failure, or input not yet saved.
- Deletion goes to recoverable Trash first. Permanent deletion is limited to trashed notes, with an in-app confirmation. Removing folders never deletes notes.
- Export and backup use create-new files and refuse to overwrite an existing path. SQLite's online-backup API produces consistent backups even in WAL mode.

## Not encryption or secure erasure

Notes, imports, exports, backups, crash memory, and clipboard text are not encrypted by Folio. Use OS encryption and appropriate access controls for sensitive information. Other applications, the OS, root, and anyone with access to your unlocked account may read your data.

Backups contain trashed notes and are retained until manually removed. A permanently deleted note may still exist in backups, SQLite free pages/WAL, storage snapshots, or SSD wear-leveling. Folio does not claim secure erasure.

## Build and installation

The local installer only writes the current user's `~/.local` paths.

Network access during development is used to fetch trusted distribution packages, crates, and vulnerability advisories; this is distinct from the running application's offline behavior. A clean dependency audit and observed offline tests reduce risk, but are not a guarantee of an absence of vulnerabilities.
