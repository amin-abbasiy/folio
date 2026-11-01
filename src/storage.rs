use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use fs2::FileExt;
use rusqlite::{Connection, MAIN_DB, OptionalExtension, params};
use thiserror::Error;

pub const MAX_NOTE_BYTES: usize = 8 * 1024 * 1024;
const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("This notebook is already open in another Folio window.")]
    AlreadyOpen,
    #[error("That note no longer exists or is in the trash.")]
    NotFound,
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("File error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Collection {
    #[default]
    All,
    Pinned,
    Folder(i64),
    Trash,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

impl Note {
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "Untitled note"
        } else {
            self.title.trim()
        }
    }

    pub fn preview(&self) -> String {
        self.body
            .lines()
            .map(|line| line.trim_start_matches(['#', '*', '-', '>', ' ', '`']))
            .filter(|line| !line.trim().is_empty())
            .flat_map(|line| line.chars().chain("  ".chars()))
            .take(140)
            .collect()
    }

    pub fn words(&self) -> usize {
        self.body.split_whitespace().count()
    }

    pub fn markdown(&self) -> String {
        format!("# {}\n\n{}\n", self.display_title(), self.body.trim_end())
    }

    pub fn clipboard_text(&self) -> String {
        if self.body.is_empty() {
            self.display_title().into()
        } else {
            format!("{}\n\n{}", self.display_title(), self.body)
        }
    }
}

#[derive(Debug, Default)]
pub struct Counts {
    pub all: usize,
    pub pinned: usize,
    pub trash: usize,
    pub folders: BTreeMap<i64, usize>,
}

pub struct Store {
    conn: Connection,
    directory: PathBuf,
    // The process-wide advisory lock lives as long as the connection.
    _lock: File,
    pub first_run: bool,
}

impl Store {
    pub fn open(directory: &Path) -> Result<Self> {
        private_directory(directory)?;
        let lock_path = directory.join("notebook.lock");
        refuse_symlink(&lock_path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)?;
        lock.try_lock_exclusive().map_err(|err| {
            if err.kind() == std::io::ErrorKind::WouldBlock {
                Error::AlreadyOpen
            } else {
                Error::Io(err)
            }
        })?;

        let path = directory.join("notes.sqlite3");
        refuse_symlink(&path)?;
        let mut conn = Connection::open(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        conn.busy_timeout(Duration::from_secs(3))?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::Invalid(
                "This notebook was created by a newer Folio version. Update Folio before opening it."
                    .into(),
            ));
        }
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL;",
        )?;
        let first_run = version == 0;
        if first_run {
            let tx = conn.transaction()?;
            tx.execute_batch(
                "CREATE TABLE folders (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE CHECK (length(trim(name)) BETWEEN 1 AND 100)
                );
                CREATE TABLE notes (
                    id INTEGER PRIMARY KEY,
                    folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL,
                    title TEXT NOT NULL DEFAULT '',
                    body TEXT NOT NULL DEFAULT '',
                    pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    deleted_at INTEGER
                );
                CREATE INDEX notes_collection ON notes(deleted_at, folder_id, pinned, updated_at DESC);
                CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                PRAGMA user_version = 1;",
            )?;
            tx.commit()?;
        }
        Ok(Self {
            conn,
            directory: directory.to_owned(),
            _lock: lock,
            first_run,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn seed_welcome(&mut self) -> Result<()> {
        if !self.first_run {
            return Ok(());
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO folders (name) VALUES ('Personal'), ('Work'), ('Ideas')",
            [],
        )?;
        let now = Utc::now().timestamp_millis();
        let samples = [
            (
                "Welcome to your quieter workspace",
                "A little room for everything on your mind.\n\nFolio is your own quiet corner: a place to collect ideas, make a plan, or simply think out loud.\n\n## Make yourself at home\n\n- [x] Find a notebook that feels like yours\n- [ ] Capture your first thought\n- [ ] Create a folder for something you love\n\n## Yours. Just yours.\n\nYour notes live on this device. No account, no cloud, no tracking. Folio never makes network requests.\n\n**A few little shortcuts**\n\n- Ctrl+N — start a new note\n- Ctrl+F — find something\n- Ctrl+Enter — switch between writing and reading\n- F1 — see all shortcuts and your storage location\n\n> You don't have to remember everything. That's what a notebook is for.",
                Some(1_i64),
                true,
            ),
            (
                "Little things, big possibilities",
                "Ideas don't need to arrive fully formed.\n\n- A recipe worth making again\n- A place to visit on a rainy afternoon\n- The first line of something new\n- A question you haven't found the answer to\n\nGive a small thought somewhere to grow.",
                Some(3),
                false,
            ),
            (
                "A fresh page for the day",
                "## What matters today?\n\n- [ ] One thing to focus on\n- [ ] One small thing to finish\n- [ ] Something just for you\n\n## A thought to keep\n\nStart wherever you are.",
                Some(1),
                false,
            ),
        ];
        for (index, (title, body, folder, pinned)) in samples.iter().enumerate() {
            tx.execute(
                "INSERT INTO notes (title, body, folder_id, pinned, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![title, body, folder, pinned, now - index as i64 * 60_000],
            )?;
        }
        tx.commit()?;
        self.first_run = false;
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM folders ORDER BY name COLLATE NOCASE")?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Folder {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?)
    }

    pub fn create_folder(&self, name: &str) -> Result<i64> {
        validate_folder_name(name)?;
        self.conn
            .execute("INSERT INTO folders (name) VALUES (?1)", [name.trim()])?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_folder(&self, id: i64, name: &str) -> Result<()> {
        validate_folder_name(name)?;
        self.conn.execute(
            "UPDATE folders SET name = ?1 WHERE id = ?2",
            params![name.trim(), id],
        )?;
        Ok(())
    }

    pub fn delete_folder(&self, id: i64) -> Result<()> {
        // ON DELETE SET NULL keeps every note, including notes in the trash.
        self.conn
            .execute("DELETE FROM folders WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn list(&self, collection: Collection, search: &str) -> Result<Vec<Note>> {
        let (where_clause, folder) = match collection {
            Collection::All => ("deleted_at IS NULL", None),
            Collection::Pinned => ("deleted_at IS NULL AND pinned = 1", None),
            Collection::Folder(id) => ("deleted_at IS NULL AND folder_id = ?1", Some(id)),
            Collection::Trash => ("deleted_at IS NOT NULL", None),
        };
        let sql = format!(
            "SELECT id, folder_id, title, body, pinned, created_at, updated_at, deleted_at FROM notes WHERE {where_clause} ORDER BY pinned DESC, updated_at DESC, id DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = if let Some(id) = folder {
            stmt.query_map([id], read_note)?
        } else {
            stmt.query_map([], read_note)?
        };
        // Literal Unicode case-insensitive matching: quotes, '%' and SQL syntax are ordinary text.
        let terms: Vec<String> = search.split_whitespace().map(str::to_lowercase).collect();
        let mut notes = Vec::new();
        for row in rows {
            let note = row?;
            if terms.is_empty() {
                notes.push(note);
            } else {
                let haystack = format!("{}\n{}", note.title, note.body).to_lowercase();
                if terms.iter().all(|term| haystack.contains(term)) {
                    notes.push(note);
                }
            }
        }
        Ok(notes)
    }

    pub fn get(&self, id: i64) -> Result<Note> {
        self.conn
            .query_row(
                "SELECT id, folder_id, title, body, pinned, created_at, updated_at, deleted_at FROM notes WHERE id = ?1",
                [id],
                read_note,
            )
            .optional()?
            .ok_or(Error::NotFound)
    }

    pub fn counts(&self) -> Result<Counts> {
        let mut counts = Counts::default();
        let mut stmt = self.conn.prepare("SELECT folder_id, pinned, deleted_at IS NOT NULL, count(*) FROM notes GROUP BY folder_id, pinned, deleted_at IS NOT NULL")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let folder: Option<i64> = row.get(0)?;
            let pinned: bool = row.get(1)?;
            let deleted: bool = row.get(2)?;
            let count = row.get::<_, i64>(3)? as usize;
            if deleted {
                counts.trash += count;
            } else {
                counts.all += count;
                if pinned {
                    counts.pinned += count;
                }
                if let Some(folder) = folder {
                    *counts.folders.entry(folder).or_default() += count;
                }
            }
        }
        Ok(counts)
    }

    pub fn create_note(&self, folder_id: Option<i64>, title: &str, body: &str) -> Result<Note> {
        validate_note(title, body)?;
        let now = Utc::now().timestamp_millis();
        self.conn.execute(
            "INSERT INTO notes (folder_id, title, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            params![folder_id, title, body, now],
        )?;
        self.get(self.conn.last_insert_rowid())
    }

    pub fn save(&self, note: &Note) -> Result<i64> {
        validate_note(&note.title, &note.body)?;
        let now = Utc::now().timestamp_millis();
        let changed = self.conn.execute(
            "UPDATE notes SET title = ?1, body = ?2, updated_at = ?3 WHERE id = ?4 AND deleted_at IS NULL",
            params![note.title, note.body, now, note.id],
        )?;
        if changed != 1 {
            return Err(Error::NotFound);
        }
        Ok(now)
    }

    pub fn pin(&self, id: i64, pinned: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET pinned = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![pinned, id],
        )?;
        Ok(())
    }

    pub fn move_note(&self, id: i64, folder: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET folder_id = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![folder, id],
        )?;
        Ok(())
    }

    pub fn trash(&self, id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![Utc::now().timestamp_millis(), id],
        )?;
        Ok(())
    }

    pub fn restore(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE notes SET deleted_at = NULL WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn delete_forever(&self, id: i64) -> Result<()> {
        // Deliberately refuses to delete live notes, even if called incorrectly by the UI.
        self.conn.execute(
            "DELETE FROM notes WHERE id = ?1 AND deleted_at IS NOT NULL",
            [id],
        )?;
        Ok(())
    }

    pub fn empty_trash(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", [])?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
        Ok(())
    }

    pub fn backup(&self, destination: &Path) -> Result<()> {
        // Reserve a new private file. Never replace an existing user file.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(destination)?;
        drop(file);
        if let Err(error) = self.conn.backup(MAIN_DB, destination, None) {
            let _ = fs::remove_file(destination);
            return Err(error.into());
        }
        Ok(())
    }

    pub fn daily_backup(&self) -> Result<Option<PathBuf>> {
        let directory = self.directory.join("backups");
        private_directory(&directory)?;
        let path = directory.join(format!("folio-{}.sqlite3", Utc::now().format("%Y-%m-%d")));
        if path.try_exists()? {
            return Ok(None);
        }
        self.backup(&path)?;
        Ok(Some(path))
    }
}

fn read_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        title: row.get(2)?,
        body: row.get(3)?,
        pinned: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        deleted_at: row.get(7)?,
    })
}

fn private_directory(path: &Path) -> Result<()> {
    refuse_symlink(path)?;
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn refuse_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Invalid(format!(
            "For your privacy, Folio won't use a symbolic link for {}.",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn validate_folder_name(name: &str) -> Result<()> {
    let length = name.trim().chars().count();
    if length == 0 || length > 100 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Folder names must be 1–100 characters, without line breaks.".into(),
        ));
    }
    Ok(())
}

fn validate_note(title: &str, body: &str) -> Result<()> {
    if title.chars().count() > 200 {
        return Err(Error::Invalid(
            "Please keep note titles under 200 characters.".into(),
        ));
    }
    if body.len() > MAX_NOTE_BYTES {
        return Err(Error::Invalid(
            "This note is too large. The limit is 8 MiB per note.".into(),
        ));
    }
    Ok(())
}

pub fn export_markdown(note: &Note, path: &Path) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    if let Err(error) = file
        .write_all(note.markdown().as_bytes())
        .and_then(|()| file.sync_all())
    {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}

pub fn import_markdown(path: &Path) -> Result<(String, String)> {
    if !fs::metadata(path)?.is_file() {
        return Err(Error::Invalid(
            "Choose a regular Markdown or text file, not a directory or special device.".into(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_NOTE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_NOTE_BYTES {
        return Err(Error::Invalid(
            "This file is too large. Import files up to 8 MiB.".into(),
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Invalid("Import expects a UTF-8 Markdown or text file.".into()))?;
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let (title, body): (String, String) = if let Some(rest) = text.strip_prefix("# ") {
        let (title, body) = rest.split_once('\n').unwrap_or((rest, ""));
        (
            title.trim().chars().take(200).collect(),
            body.trim_start_matches('\n').to_owned(),
        )
    } else {
        (
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .take(200)
                .collect(),
            text,
        )
    };
    validate_note(&title, &body)?;
    Ok((title, body))
}

pub fn safe_filename(title: &str) -> String {
    let name: String = title
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        })
        .scan(0, |bytes, character| {
            *bytes += character.len_utf8();
            (*bytes <= 180).then_some(character)
        })
        .collect();
    let name = name.trim().trim_matches('.');
    if name.is_empty() {
        "Untitled note".into()
    } else {
        name.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn notebook() -> (TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("data")).unwrap();
        (dir, store)
    }

    #[test]
    fn saves_unicode_and_survives_reopening() {
        let (dir, store) = notebook();
        let mut note = store
            .create_note(None, "日記 🌿", "Café — a thought")
            .unwrap();
        note.body.push_str("\nsecond line");
        store.save(&note).unwrap();
        let id = note.id;
        drop(store);
        let reopened = Store::open(&dir.path().join("data")).unwrap();
        let actual = reopened.get(id).unwrap();
        assert_eq!(actual.title, note.title);
        assert_eq!(actual.body, note.body);
        assert!(!reopened.first_run);
    }

    #[test]
    fn search_is_literal_unicode_case_insensitive_and_matches_all_terms() {
        let (_dir, store) = notebook();
        store
            .create_note(None, "CAFÉ", "100% useful _text_ ' OR 1=1 --")
            .unwrap();
        store.create_note(None, "Other", "Not a match").unwrap();
        assert_eq!(store.list(Collection::All, "café useful").unwrap().len(), 1);
        assert_eq!(store.list(Collection::All, "%").unwrap().len(), 1);
        assert_eq!(store.list(Collection::All, "_text_").unwrap().len(), 1);
        assert_eq!(store.list(Collection::All, "' OR 1=1 --").unwrap().len(), 1);
        assert_eq!(
            store.list(Collection::All, "useful missing").unwrap().len(),
            0
        );
    }

    #[test]
    fn collections_counts_move_trash_and_restore() {
        let (_dir, store) = notebook();
        let folder = store.create_folder("Recipes").unwrap();
        let note = store.create_note(Some(folder), "Soup", "Warm").unwrap();
        store.pin(note.id, true).unwrap();
        assert_eq!(store.counts().unwrap().pinned, 1);
        assert_eq!(store.list(Collection::Folder(folder), "").unwrap().len(), 1);
        store.trash(note.id).unwrap();
        assert_eq!(store.counts().unwrap().all, 0);
        assert_eq!(store.counts().unwrap().trash, 1);
        assert_eq!(store.list(Collection::Pinned, "").unwrap().len(), 0);
        assert!(store.save(&note).is_err());
        store.restore(note.id).unwrap();
        store.move_note(note.id, None).unwrap();
        assert_eq!(store.get(note.id).unwrap().folder_id, None);
        assert_eq!(store.counts().unwrap().all, 1);
        assert_eq!(store.counts().unwrap().trash, 0);
    }

    #[test]
    fn deleting_folder_preserves_live_and_trashed_notes() {
        let (_dir, store) = notebook();
        let folder = store.create_folder("Temporary").unwrap();
        let a = store.create_note(Some(folder), "Keep me", "One").unwrap();
        let b = store.create_note(Some(folder), "Me too", "Two").unwrap();
        store.trash(b.id).unwrap();
        store.delete_folder(folder).unwrap();
        assert_eq!(store.get(a.id).unwrap().folder_id, None);
        assert_eq!(store.get(b.id).unwrap().folder_id, None);
        store.restore(b.id).unwrap();
        assert_eq!(store.counts().unwrap().all, 2);
    }

    #[test]
    fn permanent_delete_cannot_delete_live_notes() {
        let (_dir, store) = notebook();
        let note = store.create_note(None, "Keep", "Safe").unwrap();
        store.delete_forever(note.id).unwrap();
        assert!(store.get(note.id).is_ok());
        store.empty_trash().unwrap();
        assert!(store.get(note.id).is_ok());
        store.trash(note.id).unwrap();
        store.delete_forever(note.id).unwrap();
        assert!(store.get(note.id).is_err());
    }

    #[test]
    fn second_instance_is_rejected_and_lock_released_on_exit() {
        let (dir, store) = notebook();
        let path = dir.path().join("data");
        assert!(matches!(Store::open(&path), Err(Error::AlreadyOpen)));
        drop(store);
        assert!(Store::open(&path).is_ok());
    }

    #[test]
    fn private_permissions_and_symlink_protection() {
        let (dir, store) = notebook();
        assert_eq!(
            fs::metadata(store.directory())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.directory().join("notes.sqlite3"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let link = dir.path().join("alias");
        std::os::unix::fs::symlink(store.directory(), &link).unwrap();
        assert!(Store::open(&link).is_err());
    }

    #[test]
    fn folder_names_are_validated_and_unique() {
        let (_dir, store) = notebook();
        assert!(store.create_folder("   ").is_err());
        assert!(store.create_folder("Two\nlines").is_err());
        assert!(store.create_folder(&"a".repeat(101)).is_err());
        let id = store.create_folder("Ideas").unwrap();
        assert!(store.create_folder("ideas").is_err());
        store.rename_folder(id, "Garden").unwrap();
        assert_eq!(store.folders().unwrap()[0].name, "Garden");
    }

    #[test]
    fn export_import_round_trip_and_no_overwrite() {
        let (dir, store) = notebook();
        let note = store
            .create_note(None, "A thought", "## A heading\n\n- [ ] Something\n")
            .unwrap();
        let path = dir.path().join("thought.md");
        export_markdown(&note, &path).unwrap();
        let (title, body) = import_markdown(&path).unwrap();
        assert_eq!(title, note.title);
        assert_eq!(body.trim_end(), note.body.trim_end());
        assert!(export_markdown(&note, &path).is_err());
        assert_eq!(import_markdown(&path).unwrap().0, note.title);
    }

    #[test]
    fn importing_plain_text_crlf_bom_and_invalid_utf8() {
        let (dir, _store) = notebook();
        let path = dir.path().join("Notes.txt");
        fs::write(&path, "\u{feff}# My note\r\n\r\nHello\r\n").unwrap();
        assert_eq!(
            import_markdown(&path).unwrap(),
            ("My note".into(), "Hello\n".into())
        );
        fs::write(&path, "Just some text").unwrap();
        assert_eq!(import_markdown(&path).unwrap().0, "Notes");
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(import_markdown(&path).is_err());
    }

    #[test]
    fn backups_are_consistent_private_and_never_overwrite() {
        let (dir, store) = notebook();
        store.create_note(None, "Remember", "Back this up").unwrap();
        let path = dir.path().join("backup.sqlite3");
        store.backup(&path).unwrap();
        assert!(store.backup(&path).is_err());
        let backup = Connection::open(&path).unwrap();
        assert_eq!(
            backup
                .query_row("SELECT body FROM notes", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "Back this up"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(store.daily_backup().unwrap().is_some());
        assert!(store.daily_backup().unwrap().is_none());
    }

    #[test]
    fn future_schema_is_not_modified() {
        let (dir, store) = notebook();
        store.conn.pragma_update(None, "user_version", 999).unwrap();
        drop(store);
        assert!(Store::open(&dir.path().join("data")).is_err());
    }

    #[test]
    fn welcome_is_seeded_only_once() {
        let (dir, mut store) = notebook();
        store.seed_welcome().unwrap();
        assert_eq!(store.counts().unwrap().all, 3);
        store.seed_welcome().unwrap();
        assert_eq!(store.counts().unwrap().all, 3);
        drop(store);
        let mut reopened = Store::open(&dir.path().join("data")).unwrap();
        reopened.seed_welcome().unwrap();
        assert_eq!(reopened.counts().unwrap().all, 3);
    }

    #[test]
    fn safe_filenames_cannot_escape_destination_directory() {
        assert_eq!(safe_filename("../../secrets"), "secrets");
        assert_eq!(safe_filename("///"), "Untitled note");
        assert_eq!(safe_filename("A lovely idea 🌿"), "A lovely idea 🌿");
        assert!(safe_filename(&"🌿".repeat(100)).len() <= 180);
    }

    #[test]
    fn clipboard_text_preserves_markdown_unicode_and_trailing_whitespace() {
        let (_dir, store) = notebook();
        let mut note = store.create_note(None, "日記", "**café** 🌿  \n").unwrap();
        assert_eq!(note.clipboard_text(), "日記\n\n**café** 🌿  \n");
        note.title.clear();
        note.body.clear();
        assert_eq!(note.clipboard_text(), "Untitled note");
    }

    #[test]
    fn import_and_save_size_limits_are_enforced() {
        let (dir, store) = notebook();
        let oversized = "x".repeat(MAX_NOTE_BYTES + 1);
        assert!(store.create_note(None, "Large", &oversized).is_err());
        let path = dir.path().join("too-large.md");
        fs::write(&path, oversized).unwrap();
        assert!(import_markdown(&path).is_err());
        assert!(import_markdown(dir.path()).is_err());
        assert!(store.create_note(None, &"a".repeat(201), "Body").is_err());
    }
}
