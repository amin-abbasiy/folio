use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Frame, Id, Key, Layout, Margin, Modifiers, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, pos2, vec2,
};

use crate::editor::{self, Format};
use crate::icons::{self, Icon};
use crate::markdown::{self, Document};
use crate::selection::{self, SelectionScroll};
use crate::storage::{self, Collection, Counts, Folder, Note, Store};
use crate::theme::{self, Palette, Theme};

const AUTOSAVE_DELAY: Duration = Duration::from_millis(450);

enum Action {
    Save,
    CopyNote,
    Select(i64),
    Switch(Collection),
    NewNote,
    Pin(i64, bool),
    Trash(i64),
    Restore(i64),
    Move(i64, Option<i64>),
    NewFolder,
    RenameFolder(Folder),
    DeleteFolder(Folder),
    DeleteForever(i64),
    EmptyTrash,
    Export,
    Import,
    Backup,
    Help,
    ToggleTheme,
    TogglePreview,
    Format(Format),
}

#[derive(Clone, Copy)]
enum FileOperation {
    Export,
    Import,
    Backup,
}

impl FileOperation {
    fn title(self) -> &'static str {
        match self {
            Self::Export => "Export as Markdown",
            Self::Import => "Import a note",
            Self::Backup => "Back up your notebook",
        }
    }

    fn button(self) -> &'static str {
        match self {
            Self::Export => "Export note",
            Self::Import => "Import note",
            Self::Backup => "Create backup",
        }
    }
}

enum Dialog {
    Folder {
        id: Option<i64>,
        name: String,
        error: String,
        focus: bool,
    },
    DeleteFolder {
        id: i64,
        name: String,
    },
    DeleteNote(i64),
    EmptyTrash,
    File {
        operation: FileOperation,
        path: String,
        directory: PathBuf,
        error: String,
        focus: bool,
    },
    Help,
}

struct Notice {
    text: String,
    error: bool,
}

pub struct Folio {
    store: Store,
    folders: Vec<Folder>,
    notes: Vec<Note>,
    counts: Counts,
    collection: Collection,
    draft: Option<Note>,
    search: String,
    theme: Theme,
    sidebar_visible: bool,
    preview: bool,
    document: Document,
    preview_stale: bool,
    dirty: bool,
    edited_at: Instant,
    last_save_attempt: Option<Instant>,
    save_failed: bool,
    selection: Range<usize>,
    selection_scroll: SelectionScroll,
    focus_title: bool,
    reset_editor: bool,
    dialog: Option<Dialog>,
    notice: Option<Notice>,
    actions: Vec<Action>,
}

impl Folio {
    pub fn new(ctx: &egui::Context, store: Store, backup_warning: Option<String>) -> Self {
        theme::install_fonts(ctx);
        let theme = Theme::from_setting(store.setting("theme").ok().flatten().as_deref());
        theme.apply(ctx);
        let last_id = store
            .setting("last_note")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i64>().ok());
        let preview = store.setting("preview").ok().flatten().as_deref() != Some("false");
        let mut app = Self {
            store,
            folders: Vec::new(),
            notes: Vec::new(),
            counts: Counts::default(),
            collection: Collection::All,
            draft: None,
            search: String::new(),
            theme,
            sidebar_visible: true,
            preview,
            document: Document::default(),
            preview_stale: true,
            dirty: false,
            edited_at: Instant::now(),
            last_save_attempt: None,
            save_failed: false,
            selection: 0..0,
            selection_scroll: SelectionScroll::default(),
            focus_title: false,
            reset_editor: true,
            dialog: None,
            notice: backup_warning.map(|message| Notice {
                text: format!("Automatic backup could not be created: {message}"),
                error: true,
            }),
            actions: Vec::new(),
        };
        app.reload(ctx, true);
        if let Some(id) = last_id.filter(|id| app.notes.iter().any(|note| note.id == *id)) {
            app.select(id, ctx);
        }
        app
    }

    fn report(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Some(Notice {
            text: text.into(),
            error,
        });
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.edited_at = Instant::now();
        self.preview_stale = true;
    }

    fn save_now(&mut self) -> bool {
        if !self.dirty {
            return true;
        }
        self.last_save_attempt = Some(Instant::now());
        let Some(note) = &mut self.draft else {
            return true;
        };
        match self.store.save(note) {
            Ok(updated_at) => {
                note.updated_at = updated_at;
                if let Some(row) = self.notes.iter_mut().find(|row| row.id == note.id) {
                    *row = note.clone();
                }
                self.dirty = false;
                self.save_failed = false;
                self.notes.sort_by(|a, b| {
                    b.pinned
                        .cmp(&a.pinned)
                        .then(b.updated_at.cmp(&a.updated_at))
                        .then(b.id.cmp(&a.id))
                });
                if self.notice.as_ref().is_some_and(|notice| {
                    notice.error && notice.text.starts_with("Could not save.")
                }) {
                    self.notice = None;
                }
                true
            }
            Err(error) => {
                self.save_failed = true;
                self.report(
                    format!("Could not save. Your edits are still here. {error}"),
                    true,
                );
                false
            }
        }
    }

    fn reload(&mut self, ctx: &egui::Context, reconcile_selection: bool) {
        let result = (|| {
            Ok::<_, storage::Error>((
                self.store.folders()?,
                self.store.list(self.collection, &self.search)?,
                self.store.counts()?,
            ))
        })();
        match result {
            Ok((folders, notes, counts)) => {
                self.folders = folders;
                self.notes = notes;
                self.counts = counts;
            }
            Err(error) => {
                self.report(error.to_string(), true);
                return;
            }
        }
        if reconcile_selection
            && !self.dirty
            && !self
                .draft
                .as_ref()
                .is_some_and(|draft| self.notes.iter().any(|note| note.id == draft.id))
        {
            self.draft = self.notes.first().cloned();
            self.note_changed(ctx);
        }
    }

    fn note_changed(&mut self, ctx: &egui::Context) {
        self.dirty = false;
        self.save_failed = false;
        self.preview_stale = true;
        self.selection = 0..0;
        self.selection_scroll = SelectionScroll::default();
        ctx.plugin::<egui::text_selection::LabelSelectionState>()
            .lock()
            .clear_selection();
        self.reset_editor = true;
        if let Some(note) = &self.draft
            && let Err(error) = self.store.set_setting("last_note", &note.id.to_string())
        {
            self.report(error.to_string(), true);
        }
        ctx.request_repaint();
    }

    fn select(&mut self, id: i64, ctx: &egui::Context) {
        if self.draft.as_ref().is_some_and(|note| note.id == id) || !self.save_now() {
            return;
        }
        match self.store.get(id) {
            Ok(note) => {
                self.draft = Some(note);
                self.note_changed(ctx);
            }
            Err(error) => self.report(error.to_string(), true),
        }
    }

    fn collection_title(&self) -> String {
        match self.collection {
            Collection::All => "All notes".into(),
            Collection::Pinned => "Pinned notes".into(),
            Collection::Trash => "Trash".into(),
            Collection::Folder(id) => self
                .folders
                .iter()
                .find(|folder| folder.id == id)
                .map_or_else(|| "Folder".into(), |folder| folder.name.clone()),
        }
    }

    fn folder_name(&self, id: Option<i64>) -> String {
        id.and_then(|id| self.folders.iter().find(|folder| folder.id == id))
            .map_or_else(|| "Unfiled notes".into(), |folder| folder.name.clone())
    }

    fn header(&mut self, root: &mut Ui, p: Palette) {
        egui::Panel::top("app-header")
            .exact_size(72.0)
            .frame(
                Frame::new()
                    .fill(p.paper)
                    .inner_margin(Margin::symmetric(22, 14))
                    .stroke(Stroke::new(1.0, p.line)),
            )
            .show(root, |ui| {
                ui.horizontal_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                    icons::logo(ui.painter(), rect);
                    ui.add_space(3.0);
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(RichText::new("Folio").font(theme::bold(22.0)).color(p.text));
                        ui.label(
                            RichText::new("A little room to think.")
                                .size(11.0)
                                .color(p.muted),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let new = egui::Button::new(
                            RichText::new("+   New note")
                                .font(theme::bold(13.0))
                                .color(Color32::from_rgb(66, 48, 13)),
                        )
                        .fill(p.yellow)
                        .stroke(Stroke::NONE)
                        .min_size(vec2(121.0, 35.0));
                        if ui
                            .add(new)
                            .on_hover_text("Start a fresh page · Ctrl+N")
                            .clicked()
                        {
                            self.actions.push(Action::NewNote);
                        }
                        ui.add_space(8.0);
                        if icons::button(
                            ui,
                            Icon::Help,
                            p.muted,
                            "Help, shortcuts & local files · F1",
                        )
                        .clicked()
                        {
                            self.actions.push(Action::Help);
                        }
                        let icon = if self.theme == Theme::Light {
                            Icon::Moon
                        } else {
                            Icon::Sun
                        };
                        if icons::button(ui, icon, p.muted, "Switch light / dark appearance")
                            .clicked()
                        {
                            self.actions.push(Action::ToggleTheme);
                        }
                        if icons::button(ui, Icon::Sidebar, p.muted, "Show or hide folders")
                            .clicked()
                        {
                            self.sidebar_visible = !self.sidebar_visible;
                        }
                    });
                });
            });
    }

    fn status(&mut self, root: &mut Ui, p: Palette) {
        egui::Panel::bottom("app-status")
            .exact_size(32.0)
            .frame(
                Frame::new()
                    .fill(p.paper)
                    .inner_margin(Margin::symmetric(20, 5))
                    .stroke(Stroke::new(1.0, p.line)),
            )
            .show(root, |ui| {
                ui.horizontal_centered(|ui| {
                    if self.notice.is_some() {
                        let mut dismiss = false;
                        if ui
                            .small_button("×")
                            .on_hover_text("Dismiss message")
                            .clicked()
                        {
                            dismiss = true;
                        }
                        if let Some(notice) = &self.notice {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&notice.text)
                                        .size(11.0)
                                        .color(if notice.error { p.red } else { p.green }),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&notice.text);
                        }
                        if dismiss {
                            self.notice = None;
                        }
                    } else {
                        let (color, text) = if self.save_failed {
                            (p.red, "Unsaved — check storage")
                        } else if self.dirty {
                            (p.accent, "Saving…")
                        } else {
                            (p.green, "Saved on this device")
                        };
                        let (rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                        ui.painter().circle_filled(rect.center(), 2.5, color);
                        ui.label(RichText::new(text).size(11.0).color(p.muted));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(note) = &self.draft {
                                ui.label(
                                    RichText::new(format!(
                                        "{} words  ·  {} characters",
                                        note.words(),
                                        note.body.chars().count()
                                    ))
                                    .size(11.0)
                                    .color(p.muted),
                                );
                            } else {
                                ui.label(
                                    RichText::new("No account. No cloud. Just your notes.")
                                        .size(11.0)
                                        .color(p.muted),
                                );
                            }
                        });
                    }
                });
            });
    }

    fn sidebar(&mut self, root: &mut Ui, p: Palette) {
        egui::Panel::left("folders-panel")
            .exact_size(210.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(Margin::symmetric(13, 20)),
            )
            .show(root, |ui| {
                section_label(ui, "LIBRARY", p);
                ui.add_space(8.0);
                for (collection, label, icon, count) in [
                    (Collection::All, "All notes", Icon::Notes, self.counts.all),
                    (Collection::Pinned, "Pinned", Icon::Pin, self.counts.pinned),
                    (Collection::Trash, "Trash", Icon::Trash, self.counts.trash),
                ] {
                    if navigation_row(
                        ui,
                        label,
                        icon,
                        Some(count),
                        collection == self.collection,
                        p,
                    )
                    .clicked()
                    {
                        self.actions.push(Action::Switch(collection));
                    }
                }
                ui.add_space(25.0);
                ui.horizontal(|ui| {
                    section_label(ui, "FOLDERS", p);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .small_button("+")
                            .on_hover_text("New folder · Ctrl+Shift+N")
                            .clicked()
                        {
                            self.actions.push(Action::NewFolder);
                        }
                    });
                });
                ui.add_space(6.0);
                let folder_height = (ui.available_height() - 151.0).max(60.0);
                ScrollArea::vertical()
                    .id_salt("folders-scroll")
                    .max_height(folder_height)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for folder in &self.folders {
                            let response = navigation_row(
                                ui,
                                &folder.name,
                                Icon::Folder,
                                Some(self.counts.folders.get(&folder.id).copied().unwrap_or(0)),
                                self.collection == Collection::Folder(folder.id),
                                p,
                            );
                            if response.clicked() {
                                self.actions
                                    .push(Action::Switch(Collection::Folder(folder.id)));
                            }
                            response.context_menu(|ui| {
                                if ui.button("Rename folder…").clicked() {
                                    self.actions.push(Action::RenameFolder(folder.clone()));
                                    ui.close();
                                }
                                if ui.button("Remove folder…").clicked() {
                                    self.actions.push(Action::DeleteFolder(folder.clone()));
                                    ui.close();
                                }
                            });
                        }
                    });
                ui.add_space(5.0);
                if navigation_row(ui, "New folder", Icon::Plus, None, false, p)
                    .on_hover_text("Make a little space for something")
                    .clicked()
                {
                    self.actions.push(Action::NewFolder);
                }
                ui.add_space((ui.available_height() - 104.0).max(0.0));
                ui.separator();
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, p.green);
                    ui.label(
                        RichText::new("LOCAL & PRIVATE")
                            .font(theme::bold(9.0))
                            .color(p.muted),
                    );
                });
                ui.label(
                    RichText::new("Your notes stay right here.")
                        .size(11.0)
                        .color(p.muted),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Back up notebook…")
                                .size(11.0)
                                .color(p.accent),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.actions.push(Action::Backup);
                }
            });
    }

    fn note_list(&mut self, root: &mut Ui, ctx: &egui::Context, p: Palette) {
        egui::Panel::left("notes-panel")
            .default_size(296.0)
            .size_range(240.0..=380.0)
            .resizable(true)
            .frame(
                Frame::new()
                    .fill(p.list)
                    .inner_margin(Margin::symmetric(15, 20))
                    .stroke(Stroke::new(1.0, p.line)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    if ctx.content_rect().width() < 980.0 {
                        let title = self.collection_title();
                        let short_title: String = title.chars().take(23).collect();
                        let title = if title.chars().count() > 23 {
                            format!("{short_title}…")
                        } else {
                            short_title
                        };
                        let menu = ui.menu_button(
                            RichText::new(format!("{title}    ")).font(theme::bold(19.0)),
                            |ui| {
                                for (collection, name) in [
                                    (Collection::All, "All notes"),
                                    (Collection::Pinned, "Pinned"),
                                    (Collection::Trash, "Trash"),
                                ] {
                                    if ui
                                        .selectable_label(self.collection == collection, name)
                                        .clicked()
                                    {
                                        self.actions.push(Action::Switch(collection));
                                        ui.close();
                                    }
                                }
                                ui.separator();
                                for folder in &self.folders {
                                    if ui
                                        .selectable_label(
                                            self.collection == Collection::Folder(folder.id),
                                            &folder.name,
                                        )
                                        .clicked()
                                    {
                                        self.actions
                                            .push(Action::Switch(Collection::Folder(folder.id)));
                                        ui.close();
                                    }
                                }
                                if ui.button("New folder…").clicked() {
                                    self.actions.push(Action::NewFolder);
                                    ui.close();
                                }
                            },
                        );
                        icons::draw(
                            ui.painter(),
                            egui::Rect::from_center_size(
                                pos2(
                                    menu.response.rect.right() - 13.0,
                                    menu.response.rect.center().y,
                                ),
                                vec2(12.0, 12.0),
                            ),
                            Icon::ChevronDown,
                            p.muted,
                        );
                    } else {
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.collection_title()).font(theme::bold(21.0)),
                            )
                            .truncate(),
                        );
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.notes.len().to_string())
                                .size(12.0)
                                .color(p.muted),
                        );
                    });
                });
                ui.add_space(10.0);
                let mut changed = false;
                Frame::new()
                    .fill(p.sidebar)
                    .corner_radius(7)
                    .inner_margin(Margin::symmetric(9, 5))
                    .stroke(Stroke::new(1.0, p.line))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 7.0;
                            icons::small(ui, Icon::Search, p.muted);
                            let clear_width = if self.search.is_empty() { 0.0 } else { 22.0 };
                            let response = ui.add(
                                TextEdit::singleline(&mut self.search)
                                    .id(Id::new("note-search"))
                                    .hint_text("Search notes")
                                    .font(FontId::proportional(13.0))
                                    .frame(Frame::NONE)
                                    .desired_width((ui.available_width() - clear_width).max(50.0)),
                            );
                            changed = response.changed();
                            if !self.search.is_empty() && ui.small_button("×").clicked() {
                                self.search.clear();
                                changed = true;
                            }
                        });
                    });
                if changed && self.save_now() {
                    self.reload(ctx, true);
                }
                ui.add_space(16.0);
                if self.collection == Collection::Trash {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Kept until you empty the trash.")
                                .size(10.0)
                                .color(p.muted),
                        );
                    });
                    if self.counts.trash > 0 && ui.small_button("Empty trash…").clicked() {
                        self.actions.push(Action::EmptyTrash);
                    }
                    ui.add_space(8.0);
                }
                if self.notes.is_empty() {
                    ui.add_space(45.0);
                    ui.vertical_centered(|ui| {
                        icons::small(
                            ui,
                            if self.search.is_empty() {
                                Icon::Notes
                            } else {
                                Icon::Search
                            },
                            p.faint,
                        );
                        ui.add_space(9.0);
                        ui.label(
                            RichText::new(if !self.search.is_empty() {
                                "No matching notes"
                            } else if self.collection == Collection::Trash {
                                "A clean slate"
                            } else {
                                "Room for a new thought"
                            })
                            .font(theme::bold(13.0))
                            .color(p.muted),
                        );
                        ui.add_space(3.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(if !self.search.is_empty() {
                                    "Try a different word or another folder."
                                } else if self.collection == Collection::Trash {
                                    "Deleted notes will appear here."
                                } else {
                                    "Create your first note with Ctrl+N."
                                })
                                .size(11.0)
                                .color(p.muted),
                            )
                            .wrap(),
                        );
                    });
                } else {
                    ScrollArea::vertical()
                        .id_salt("notes-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut was_pinned = None;
                            for note in &self.notes {
                                if self.collection != Collection::Trash
                                    && was_pinned != Some(note.pinned)
                                {
                                    if was_pinned.is_some() {
                                        ui.add_space(15.0);
                                    }
                                    section_label(
                                        ui,
                                        if note.pinned { "PINNED" } else { "YOUR NOTES" },
                                        p,
                                    );
                                    ui.add_space(7.0);
                                    was_pinned = Some(note.pinned);
                                }
                                let selected =
                                    self.draft.as_ref().is_some_and(|draft| draft.id == note.id);
                                let response = note_card(ui, note, selected, p);
                                if response.clicked() {
                                    self.actions.push(Action::Select(note.id));
                                }
                                response.context_menu(|ui| {
                                    if self.collection == Collection::Trash {
                                        if ui.button("Restore note").clicked() {
                                            self.actions.push(Action::Restore(note.id));
                                            ui.close();
                                        }
                                        if ui.button("Delete permanently…").clicked() {
                                            self.actions.push(Action::DeleteForever(note.id));
                                            ui.close();
                                        }
                                    } else {
                                        if ui
                                            .button(if note.pinned {
                                                "Unpin note"
                                            } else {
                                                "Pin note"
                                            })
                                            .clicked()
                                        {
                                            self.actions.push(Action::Pin(note.id, !note.pinned));
                                            ui.close();
                                        }
                                        if ui.button("Move to trash").clicked() {
                                            self.actions.push(Action::Trash(note.id));
                                            ui.close();
                                        }
                                    }
                                });
                                ui.add_space(5.0);
                            }
                        });
                }
            });
    }

    fn editor_toolbar(&mut self, ui: &mut Ui, p: Palette) {
        let Some(note) = &self.draft else {
            return;
        };
        let (id, pinned, deleted, folder) = (
            note.id,
            note.pinned,
            note.deleted_at.is_some(),
            note.folder_id,
        );
        ui.horizontal(|ui| {
            icons::small(ui, Icon::Folder, p.faint);
            let breadcrumb_size = vec2((ui.available_width() - 380.0).max(30.0), 22.0);
            ui.allocate_ui_with_layout(
                breadcrumb_size,
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_size(breadcrumb_size);
                    ui.add(
                        egui::Label::new(
                            RichText::new(self.folder_name(folder))
                                .size(11.0)
                                .color(p.muted),
                        )
                        .truncate(),
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let more = icons::button(ui, Icon::More, p.muted, "More note actions");
                egui::Popup::menu(&more).show(|ui| {
                    if ui.button("Copy whole note").clicked() {
                        self.actions.push(Action::CopyNote);
                        ui.close();
                    }
                    if ui.button("Export as Markdown…").clicked() {
                        self.actions.push(Action::Export);
                        ui.close();
                    }
                    if !deleted {
                        ui.menu_button("Move to folder", |ui| {
                            if ui
                                .selectable_label(folder.is_none(), "Unfiled notes")
                                .clicked()
                            {
                                self.actions.push(Action::Move(id, None));
                                ui.close();
                            }
                            for target in &self.folders {
                                if ui
                                    .selectable_label(folder == Some(target.id), &target.name)
                                    .clicked()
                                {
                                    self.actions.push(Action::Move(id, Some(target.id)));
                                    ui.close();
                                }
                            }
                        });
                        ui.separator();
                        if ui.button("Move to trash").clicked() {
                            self.actions.push(Action::Trash(id));
                            ui.close();
                        }
                    } else if ui.button("Delete permanently…").clicked() {
                        self.actions.push(Action::DeleteForever(id));
                        ui.close();
                    }
                });
                if deleted {
                    if icons::button(ui, Icon::Restore, p.accent, "Restore this note").clicked() {
                        self.actions.push(Action::Restore(id));
                    }
                } else {
                    if icons::button(
                        ui,
                        Icon::Trash,
                        p.muted,
                        "Move to trash — you can restore it later",
                    )
                    .clicked()
                    {
                        self.actions.push(Action::Trash(id));
                    }
                    if icons::button(
                        ui,
                        Icon::Pin,
                        if pinned { p.accent } else { p.muted },
                        if pinned { "Unpin note" } else { "Pin note" },
                    )
                    .clicked()
                    {
                        self.actions.push(Action::Pin(id, !pinned));
                    }
                }
                if ui.add(egui::Button::new(RichText::new("Copy note").size(12.0)).min_size(vec2(90.0, 29.0)))
                    .on_hover_text("Copy the entire title and note text, including unsaved edits · Ctrl+Shift+C")
                    .clicked()
                {
                    self.actions.push(Action::CopyNote);
                }
                ui.add_space(9.0);
                let (icon, text) = if self.preview {
                    (Icon::Edit, "Write")
                } else {
                    (Icon::Eye, "Read")
                };
                let response = ui
                    .add_enabled(
                        !deleted,
                        egui::Button::new(RichText::new(format!("     {text}")).size(12.0))
                            .fill(p.sidebar)
                            .min_size(vec2(85.0, 29.0)),
                    )
                    .on_hover_text("Switch writing / reading view · Ctrl+Enter");
                if response.clicked() {
                    self.actions.push(Action::TogglePreview);
                }
                icons::draw(
                    ui.painter(),
                    egui::Rect::from_min_size(
                        pos2(response.rect.left() + 9.0, response.rect.center().y - 8.0),
                        vec2(16.0, 16.0),
                    ),
                    icon,
                    p.muted,
                );
            });
        });
    }

    fn editor_panel(&mut self, root: &mut Ui, ctx: &egui::Context, p: Palette) {
        egui::CentralPanel::default().frame(Frame::new().fill(p.paper).inner_margin(Margin::symmetric(30, 15))).show(root, |ui| {
            if self.draft.is_none() {
                ui.add_space((ui.available_height() * 0.28).max(0.0));
                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(64.0, 64.0), Sense::hover());
                    icons::logo(ui.painter(), rect);
                    ui.add_space(21.0);
                    ui.label(RichText::new("Make room for a thought.").font(theme::bold(25.0)).color(p.text));
                    ui.add_space(7.0);
                    ui.label(RichText::new("A fresh page is a good place to start.").size(14.0).color(p.muted));
                    ui.add_space(21.0);
                    if ui.add(egui::Button::new("+  Create a note").fill(p.selected).min_size(vec2(154.0, 37.0))).clicked() { self.actions.push(Action::NewNote); }
                });
                return;
            }
            self.editor_toolbar(ui, p);
            ui.add_space(9.0);
            ui.separator();
            ui.add_space(12.0);

            let note = self.draft.as_mut().expect("checked draft");
            let deleted = note.deleted_at.is_some();
            if self.reset_editor {
                egui::text_edit::TextEditState::default().store(ctx, Id::new("note-body"));
                self.reset_editor = false;
            }
            if deleted {
                Frame::new().fill(p.selected).corner_radius(6).inner_margin(10).show(ui, |ui| {
                    ui.label(RichText::new("This note is in the trash. Restore it to keep writing.").size(12.0).color(p.accent));
                });
                ui.add_space(10.0);
            }
            let date = format_datetime(note.updated_at);
            ui.horizontal(|ui| {
                ui.label(RichText::new(date).size(10.0).color(p.muted));
                if note.pinned {
                    ui.label(RichText::new("·  PINNED").font(theme::bold(9.0)).color(p.accent));
                }
            });
            ui.add_space(8.0);
            let mut edited = false;
            let mut toggled = None;
            ScrollArea::vertical().id_salt(("editor-scroll", note.id, self.preview)).auto_shrink([false, false]).show(ui, |ui| {
                let viewport = ui.clip_rect();
                ui.set_max_width(ui.available_width().min(850.0));
                if self.preview || deleted {
                    selection::label(ui, RichText::new(note.display_title()).font(theme::bold(29.0)).color(p.text), p.text, Sense::hover(), &mut self.selection_scroll);
                } else {
                    let response = ui.add(TextEdit::singleline(&mut note.title).id(Id::new(("note-title", note.id))).hint_text("Untitled note").char_limit(200).font(theme::bold(29.0)).frame(Frame::NONE).desired_width(f32::INFINITY));
                    if self.focus_title { response.request_focus(); self.focus_title = false; }
                    edited |= response.changed();
                }
                ui.add_space(18.0);
                if self.preview || deleted {
                    if self.preview_stale {
                        self.document = Document::parse(&note.body);
                        self.preview_stale = false;
                    }
                    if note.body.is_empty() {
                        ui.label(RichText::new("An unwritten thought. Switch to Write to begin.").size(15.0).color(p.faint));
                    } else {
                        toggled = self.document.show(ui, p, !deleted, &mut self.selection_scroll);
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 3.0;
                        for (label, tooltip, format) in [
                            ("B", "Bold · Ctrl+B", Format::Bold),
                            ("I", "Italic · Ctrl+I", Format::Italic),
                            ("H", "Heading", Format::Heading),
                            ("•", "Bulleted list", Format::Bullet),
                            ("☑", "Checklist · Ctrl+Shift+L", Format::Checklist),
                            (">", "Quote", Format::Quote),
                            ("</>", "Inline code", Format::Code),
                        ] {
                            if ui.add(egui::Button::new(RichText::new(label).font(theme::bold(13.0)).color(p.muted)).frame(false).min_size(vec2(29.0, 27.0))).on_hover_text(tooltip).clicked() { self.actions.push(Action::Format(format)); }
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("MARKDOWN").size(9.0).color(p.faint)).on_hover_text("Write in Markdown, then use Read to see formatting. Links and images are never fetched.");
                        });
                    });
                    ui.add_space(10.0);
                    let output = TextEdit::multiline(&mut note.body).id(Id::new("note-body")).hint_text("Let your thoughts settle here…").font(FontId::proportional(16.0)).frame(Frame::NONE).desired_width(f32::INFINITY).desired_rows(25).show(ui);
                    edited |= output.response.changed();
                    self.selection_scroll.observe(&output.response);
                    if let Some(range) = output.cursor_range { let range = range.as_sorted_char_range(); self.selection = range.start.0..range.end.0; }
                }
                ui.add_space(50.0);
                self.selection_scroll.finish(ui, viewport);
            });
            if let Some((range, checked)) = toggled {
                edited |= markdown::toggle_task(&mut note.body, range, checked);
            }
            if edited { self.mark_dirty(); }
        });
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let copy_whole_note = ctx.input_mut(|input| {
            let shortcut = input.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::C);
            // Winit normalizes Ctrl+Shift+C to Copy instead of a C key press.
            // If the entire press/release was batched, its modifiers survive on
            // the C release even though the frame's final modifiers are empty.
            let shifted_copy = input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
                && ((input.modifiers.ctrl && input.modifiers.shift)
                    || input.events.iter().any(|event| {
                        matches!(event,
                        egui::Event::Key { key: Key::C, pressed: false, modifiers, .. }
                            if modifiers.ctrl && modifiers.shift)
                    }));
            if shortcut || shifted_copy {
                input
                    .events
                    .retain(|event| !matches!(event, egui::Event::Copy));
                true
            } else {
                false
            }
        });
        if copy_whole_note {
            self.actions.push(Action::CopyNote);
        }
        let shortcuts = [
            (
                Modifiers::CTRL | Modifiers::SHIFT,
                Key::N,
                Action::NewFolder,
            ),
            (Modifiers::CTRL | Modifiers::SHIFT, Key::E, Action::Export),
            (Modifiers::CTRL, Key::N, Action::NewNote),
            (Modifiers::CTRL, Key::O, Action::Import),
            (Modifiers::CTRL | Modifiers::SHIFT, Key::B, Action::Backup),
            (Modifiers::CTRL, Key::Enter, Action::TogglePreview),
            (Modifiers::NONE, Key::F1, Action::Help),
        ];
        for (modifiers, key, action) in shortcuts {
            if ctx.input_mut(|input| input.consume_key(modifiers, key)) {
                self.actions.push(action);
            }
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::CTRL, Key::F)) {
            ctx.memory_mut(|memory| memory.request_focus(Id::new("note-search")));
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::CTRL, Key::S)) {
            self.actions.push(Action::Save);
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::CTRL | Modifiers::SHIFT, Key::P))
            && let Some(note) = self.draft.as_ref().filter(|note| note.deleted_at.is_none())
        {
            self.actions.push(Action::Pin(note.id, !note.pinned));
        }
        if ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
            && !self.search.is_empty()
        {
            self.search.clear();
            if self.save_now() {
                self.reload(ctx, true);
            }
        }
        if !self.preview && ctx.memory(|memory| memory.has_focus(Id::new("note-body"))) {
            for (modifiers, key, format) in [
                (Modifiers::CTRL, Key::B, Format::Bold),
                (Modifiers::CTRL, Key::I, Format::Italic),
                (
                    Modifiers::CTRL | Modifiers::SHIFT,
                    Key::L,
                    Format::Checklist,
                ),
            ] {
                if ctx.input_mut(|input| input.consume_key(modifiers, key)) {
                    self.actions.push(Action::Format(format));
                }
            }
        }
    }

    fn act(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::CopyNote => {
                if let Some(note) = &self.draft {
                    ctx.copy_text(note.clipboard_text());
                    self.report("Copied whole note: title and Markdown text.", false);
                }
            }
            Action::Save => {
                self.save_now();
            }
            Action::Select(id) => self.select(id, ctx),
            Action::Switch(collection) => {
                if !self.save_now() {
                    return;
                }
                self.collection = collection;
                self.search.clear();
                self.reload(ctx, true);
            }
            Action::NewNote => {
                if !self.save_now() {
                    return;
                }
                self.search.clear();
                let folder = match self.collection {
                    Collection::Folder(id) => Some(id),
                    _ => None,
                };
                if self.collection == Collection::Trash {
                    self.collection = Collection::All;
                }
                let result = self.store.create_note(folder, "", "");
                match result {
                    Ok(mut note) => {
                        if self.collection == Collection::Pinned {
                            if let Err(error) = self.store.pin(note.id, true) {
                                self.report(error.to_string(), true);
                            } else {
                                note.pinned = true;
                            }
                        }
                        self.draft = Some(note);
                        self.preview = false;
                        self.focus_title = true;
                        self.note_changed(ctx);
                        self.reload(ctx, false);
                    }
                    Err(error) => self.report(error.to_string(), true),
                }
            }
            Action::Pin(id, pinned) => {
                if !self.save_now() {
                    return;
                }
                match self.store.pin(id, pinned) {
                    Ok(()) => {
                        if let Some(note) = self.draft.as_mut().filter(|note| note.id == id) {
                            note.pinned = pinned;
                        }
                        self.reload(ctx, true);
                    }
                    Err(error) => self.report(error.to_string(), true),
                }
            }
            Action::Move(id, folder) => {
                if !self.save_now() {
                    return;
                }
                match self.store.move_note(id, folder) {
                    Ok(()) => {
                        if let Some(note) = self.draft.as_mut().filter(|note| note.id == id) {
                            note.folder_id = folder;
                        }
                        self.reload(ctx, true);
                        self.report("Note moved to its new home.", false);
                    }
                    Err(error) => self.report(error.to_string(), true),
                }
            }
            Action::Trash(id) | Action::Restore(id) => {
                if !self.save_now() {
                    return;
                }
                let restoring = matches!(action, Action::Restore(_));
                let result = if restoring {
                    self.store.restore(id)
                } else {
                    self.store.trash(id)
                };
                match result {
                    Ok(()) => {
                        self.reload(ctx, true);
                        self.report(
                            if restoring {
                                "Note restored. Welcome back."
                            } else {
                                "Moved to Trash. You can restore it any time."
                            },
                            false,
                        );
                    }
                    Err(error) => self.report(error.to_string(), true),
                }
            }
            Action::NewFolder => {
                self.dialog = Some(Dialog::Folder {
                    id: None,
                    name: String::new(),
                    error: String::new(),
                    focus: true,
                })
            }
            Action::RenameFolder(folder) => {
                self.dialog = Some(Dialog::Folder {
                    id: Some(folder.id),
                    name: folder.name,
                    error: String::new(),
                    focus: true,
                })
            }
            Action::DeleteFolder(folder) => {
                self.dialog = Some(Dialog::DeleteFolder {
                    id: folder.id,
                    name: folder.name,
                })
            }
            Action::DeleteForever(id) => self.dialog = Some(Dialog::DeleteNote(id)),
            Action::EmptyTrash => self.dialog = Some(Dialog::EmptyTrash),
            Action::Export => {
                if self.draft.is_some() {
                    self.open_file_dialog(FileOperation::Export);
                }
            }
            Action::Import => self.open_file_dialog(FileOperation::Import),
            Action::Backup => {
                if self.save_now() {
                    self.open_file_dialog(FileOperation::Backup);
                }
            }
            Action::Help => self.dialog = Some(Dialog::Help),
            Action::ToggleTheme => {
                self.theme = self.theme.toggle();
                self.theme.apply(ctx);
                if let Err(error) = self.store.set_setting("theme", self.theme.setting()) {
                    self.report(error.to_string(), true);
                }
            }
            Action::TogglePreview => {
                if self
                    .draft
                    .as_ref()
                    .is_some_and(|note| note.deleted_at.is_some())
                {
                    return;
                }
                self.preview = !self.preview;
                self.selection_scroll = SelectionScroll::default();
                ctx.plugin::<egui::text_selection::LabelSelectionState>()
                    .lock()
                    .clear_selection();
                if let Err(error) = self
                    .store
                    .set_setting("preview", if self.preview { "true" } else { "false" })
                {
                    self.report(error.to_string(), true);
                }
            }
            Action::Format(format) => {
                if self.preview {
                    return;
                }
                if let Some(note) = &mut self.draft {
                    if note.deleted_at.is_some() {
                        return;
                    }
                    let range = editor::apply(&mut note.body, self.selection.clone(), format);
                    let mut state = egui::text_edit::TextEditState::load(ctx, Id::new("note-body"))
                        .unwrap_or_default();
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(range.start),
                            egui::text::CCursor::new(range.end),
                        )));
                    state.store(ctx, Id::new("note-body"));
                    self.selection = range;
                    self.mark_dirty();
                    ctx.memory_mut(|memory| memory.request_focus(Id::new("note-body")));
                }
            }
        }
        ctx.request_repaint();
    }

    fn open_file_dialog(&mut self, operation: FileOperation) {
        let home = directories::BaseDirs::new().map_or_else(
            || self.store.directory().to_owned(),
            |dirs| dirs.home_dir().to_owned(),
        );
        let documents = home.join("Documents");
        let directory = match operation {
            FileOperation::Backup => self.store.directory().join("backups"),
            _ if documents.is_dir() => documents,
            _ => home,
        };
        let filename = match operation {
            FileOperation::Export => format!(
                "{}.md",
                storage::safe_filename(
                    self.draft
                        .as_ref()
                        .map_or("Untitled note", Note::display_title)
                )
            ),
            FileOperation::Import => String::new(),
            FileOperation::Backup => {
                format!("folio-{}.sqlite3", Local::now().format("%Y-%m-%d-%H%M%S"))
            }
        };
        let path = directory.join(filename).to_string_lossy().into_owned();
        self.dialog = Some(Dialog::File {
            operation,
            path,
            directory,
            error: String::new(),
            focus: true,
        });
    }

    fn show_dialog(&mut self, ctx: &egui::Context, p: Palette) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut keep = true;
        let response = egui::Modal::new(Id::new("folio-dialog")).frame(Frame::new().fill(p.paper).stroke(Stroke::new(1.0, p.line)).corner_radius(13).inner_margin(26)).show(ctx, |ui| {
            ui.set_width(465.0_f32.min(ctx.content_rect().width() - 90.0));
            match &mut dialog {
                Dialog::Folder { id, name, error, focus } => {
                    dialog_heading(ui, if id.is_some() { "A new name" } else { "Make a little space" }, "Give this folder a name that feels right.", p);
                    let input = ui.add(TextEdit::singleline(name).id(Id::new("folder-name")).hint_text("Folder name").char_limit(100).desired_width(f32::INFINITY));
                    if *focus { input.request_focus(); *focus = false; }
                    inline_error(ui, error, p);
                    let submit = dialog_buttons(ui, if id.is_some() { "Save name" } else { "Create folder" }, &mut keep, p) || ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                    if submit {
                        if !self.save_now() { *error = "Save your current edits before changing folders.".into(); return; }
                        let result = if let Some(id) = id { self.store.rename_folder(*id, name).map(|()| *id) } else { self.store.create_folder(name) };
                        match result {
                            Ok(id) => {
                                if self.save_now() { self.collection = Collection::Folder(id); self.search.clear(); self.reload(ctx, true); keep = false; }
                            }
                            Err(err) => *error = friendly_error(err),
                        }
                    }
                }
                Dialog::DeleteFolder { id, name } => {
                    dialog_heading(ui, &format!("Remove “{name}”?"), "Your notes won't be deleted. They will remain in All notes, without a folder.", p);
                    if dialog_buttons(ui, "Remove folder", &mut keep, p) && self.save_now() {
                        match self.store.delete_folder(*id) {
                            Ok(()) => {
                                if self.collection == Collection::Folder(*id) { self.collection = Collection::All; }
                                if let Some(note) = &mut self.draft && note.folder_id == Some(*id) { note.folder_id = None; }
                                self.reload(ctx, true); keep = false;
                            }
                            Err(error) => self.report(error.to_string(), true),
                        }
                    }
                }
                Dialog::DeleteNote(id) => {
                    dialog_heading(ui, "Let this note go?", "This permanently removes the note from this notebook. You cannot undo it. Existing backup files are not changed.", p);
                    if dialog_buttons(ui, "Delete permanently", &mut keep, p) {
                        match self.store.delete_forever(*id) {
                            Ok(()) => { self.reload(ctx, true); keep = false; }
                            Err(error) => self.report(error.to_string(), true),
                        }
                    }
                }
                Dialog::EmptyTrash => {
                    dialog_heading(ui, "Empty the trash?", "All notes in the trash will be permanently removed from this notebook. This cannot be undone. Existing backups are not changed.", p);
                    if dialog_buttons(ui, "Empty trash", &mut keep, p) {
                        match self.store.empty_trash() {
                            Ok(()) => { self.reload(ctx, true); keep = false; }
                            Err(error) => self.report(error.to_string(), true),
                        }
                    }
                }
                Dialog::File { operation, path, directory, error, focus } => {
                    dialog_heading(ui, operation.title(), match operation {
                        FileOperation::Export => "A portable Markdown file, ready to keep anywhere locally.",
                        FileOperation::Import => "Choose a UTF-8 .md or .txt file, up to 8 MiB.",
                        FileOperation::Backup => "A complete SQLite snapshot: notes, folders, trash, and preferences.",
                    }, p);
                    ui.horizontal(|ui| {
                        if ui.small_button("↑ Parent").clicked() && let Some(parent) = directory.parent() {
                            *directory = parent.to_owned();
                            relocate_file_path(path, directory, *operation);
                        }
                        ui.add(egui::Label::new(RichText::new(directory.to_string_lossy()).size(11.0).color(p.muted)).truncate());
                    });
                    Frame::new().fill(p.list).stroke(Stroke::new(1.0, p.line)).corner_radius(6).inner_margin(8).show(ui, |ui| {
                        ScrollArea::vertical().id_salt("file-browser").max_height(170.0).min_scrolled_height(110.0).auto_shrink([false, false]).show(ui, |ui| {
                            let mut entries: Vec<_> = match fs::read_dir(&*directory) {
                                Ok(entries) => entries.filter_map(Result::ok).filter(|entry| !entry.file_name().to_string_lossy().starts_with('.')).take(500).collect(),
                                Err(err) => { ui.label(RichText::new(err.to_string()).size(11.0).color(p.red)); Vec::new() }
                            };
                            entries.sort_by_key(|entry| (!entry.path().is_dir(), entry.file_name().to_string_lossy().to_lowercase()));
                            for entry in entries {
                                let entry_path = entry.path();
                                let is_directory = entry_path.is_dir();
                                if !is_directory && matches!(operation, FileOperation::Import) && !entry_path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt") || extension.eq_ignore_ascii_case("markdown")) { continue; }
                                ui.horizontal(|ui| {
                                    icons::small(ui, if is_directory { Icon::Folder } else { Icon::Notes }, p.muted);
                                    if ui.selectable_label(Path::new(path) == entry_path, entry.file_name().to_string_lossy()).clicked() {
                                        if is_directory { *directory = entry_path; relocate_file_path(path, directory, *operation); }
                                        else { *path = entry_path.to_string_lossy().into_owned(); }
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(7.0);
                    ui.label(RichText::new("FULL FILE PATH").size(9.0).color(p.muted));
                    let input = ui.add(TextEdit::singleline(path).id(Id::new("file-path")).desired_width(f32::INFINITY));
                    if *focus { input.request_focus(); *focus = false; }
                    ui.label(RichText::new("Local files only. Existing files are never overwritten.").size(10.0).color(p.muted));
                    inline_error(ui, error, p);
                    if dialog_buttons(ui, operation.button(), &mut keep, p)
                        || ctx.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter))
                    {
                        let destination = expand_home(path);
                        if !destination.is_absolute() { *error = "Please enter an absolute file path (or ~/…).".into(); }
                        else {
                            let result = match operation {
                                FileOperation::Export => self.draft.as_ref().ok_or(storage::Error::NotFound).and_then(|note| storage::export_markdown(note, &destination)),
                                FileOperation::Backup => if self.save_now() { self.store.backup(&destination) } else { Err(storage::Error::Invalid("Save your edits before backing up the notebook.".into())) },
                                FileOperation::Import => {
                                    if !self.save_now() { Err(storage::Error::Invalid("Save your current edits before importing.".into())) }
                                    else {
                                        storage::import_markdown(&destination).and_then(|(title, body)| {
                                            let folder = match self.collection { Collection::Folder(id) => Some(id), _ => None };
                                            let note = self.store.create_note(folder, &title, &body)?;
                                            self.collection = folder.map_or(Collection::All, Collection::Folder);
                                            self.search.clear();
                                            self.draft = Some(note);
                                            self.note_changed(ctx);
                                            self.reload(ctx, false);
                                            Ok(())
                                        })
                                    }
                                }
                            };
                            match result {
                                Ok(()) => { self.report(format!("{}: {}", match operation { FileOperation::Import => "Imported", FileOperation::Export => "Exported", FileOperation::Backup => "Backup created" }, destination.display()), false); keep = false; }
                                Err(err) => *error = friendly_error(err),
                            }
                        }
                    }
                }
                Dialog::Help => {
                    dialog_heading(ui, "A quiet place for your thoughts.", &format!("Folio {} · Native Rust · Made for Linux", env!("CARGO_PKG_VERSION")), p);
                    ScrollArea::vertical().max_height((ctx.content_rect().height() - 260.0).clamp(160.0, 490.0)).show(ui, |ui| {
                        ui.label(RichText::new("YOUR KEYBOARD, A LITTLE MORE USEFUL").font(theme::bold(10.0)).color(p.muted));
                        ui.add_space(8.0);
                        egui::Grid::new("shortcuts").num_columns(2).spacing(vec2(24.0, 9.0)).show(ui, |ui| {
                            for (key, description) in [
                                ("Ctrl + N", "New note"), ("Ctrl + Shift + N", "New folder"), ("Ctrl + F", "Search this collection"), ("Ctrl + S", "Save now (also automatic)"),
                                ("Ctrl + Shift + C", "Copy whole note (title + Markdown)"),
                                ("Ctrl + O", "Import Markdown / text"), ("Ctrl + Shift + B", "Back up notebook"),
                                ("Ctrl + Enter", "Write / read view"), ("Ctrl + B / I", "Bold / italic in the editor"), ("Ctrl + Shift + L", "Checklist in the editor"),
                                ("Ctrl + Shift + E", "Export current note"), ("Ctrl + Shift + P", "Pin / unpin current note"), ("Ctrl + Z / Shift + Z", "Undo / redo while editing"), ("Escape", "Close a dialog / clear search"),
                                ("Right-click a folder", "Rename or remove it"), ("Right-click a note", "Pin, trash, or restore it"),
                            ] { ui.label(RichText::new(key).monospace().size(11.0).color(p.accent)); ui.label(RichText::new(description).size(11.0)); ui.end_row(); }
                        });
                        ui.add_space(20.0);
                        ui.separator();
                        ui.add_space(12.0);
                        ui.label(RichText::new("YOURS. JUST YOURS.").font(theme::bold(10.0)).color(p.muted));
                        ui.add_space(6.0);
                        ui.label(RichText::new("No account, cloud sync, telemetry, or application network requests. Links are copied, not opened. Remote images and HTML are never executed or loaded.").size(12.0));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Notes and backups are not encrypted. Use disk encryption for sensitive information. A private local backup is made on the first launch each day; backups are kept until you remove them.").size(11.0).color(p.muted));
                        ui.add_space(12.0);
                        ui.label(RichText::new("NOTEBOOK LOCATION").font(theme::bold(10.0)).color(p.muted));
                        ui.add(egui::Label::new(RichText::new(self.store.directory().to_string_lossy()).monospace().size(11.0)).wrap());
                        if ui.small_button("Copy location").clicked() { ctx.copy_text(self.store.directory().to_string_lossy().into_owned()); }
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Import a note…").clicked() { self.actions.push(Action::Import); keep = false; }
                            if ui.button("Back up notebook…").clicked() { self.actions.push(Action::Backup); keep = false; }
                        });
                        ui.add_space(10.0);
                        ui.collapsing("Font license", |ui| { ui.label(RichText::new(include_str!("../assets/INTER-LICENSE.txt")).size(10.0)); });
                    });
                    ui.add_space(14.0);
                    if ui.button("Back to your notes").clicked() { keep = false; }
                }
            }
        });
        if keep && !response.should_close() {
            self.dialog = Some(dialog);
        }
    }
}

impl eframe::App for Folio {
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if self.dialog.is_none() {
            self.selection_scroll.capture_pointer(ctx, input);
        }
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.dirty {
            let retry_ready = !self.save_failed
                || self
                    .last_save_attempt
                    .is_none_or(|attempt| attempt.elapsed() > Duration::from_secs(3));
            if self.edited_at.elapsed() >= AUTOSAVE_DELAY && retry_ready {
                self.save_now();
            }
            ctx.request_repaint_after(if self.save_failed {
                Duration::from_secs(3)
            } else {
                AUTOSAVE_DELAY.saturating_sub(self.edited_at.elapsed())
            });
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        let p = self.theme.palette();
        self.header(ui, p);
        self.status(ui, p);
        if self.sidebar_visible && ctx.content_rect().width() >= 980.0 {
            self.sidebar(ui, p);
        }
        self.note_list(ui, &ctx, p);
        self.editor_panel(ui, &ctx, p);
        self.show_dialog(&ctx, p);
        for action in std::mem::take(&mut self.actions) {
            self.act(action, &ctx);
        }
        // Handle close/focus events after text input, so even edits in the last frame are saved.
        if ctx.input(|input| input.viewport().close_requested()) && !self.save_now() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        if !ctx.input(|input| input.focused) && self.dirty && !self.save_failed {
            self.save_now();
        }
        ctx.request_repaint_after(if self.dirty {
            AUTOSAVE_DELAY
        } else {
            Duration::from_secs(30)
        });
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.theme.palette().paper.to_normalized_gamma_f32()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.save_now() {
            eprintln!("Folio: unsaved edits remain after an unexpected exit.");
        }
    }
}

fn navigation_row(
    ui: &mut Ui,
    label: &str,
    icon: Icon,
    count: Option<usize>,
    selected: bool,
    p: Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, 6, if selected { p.selected } else { p.hover });
    }
    let color = if selected { p.accent } else { p.muted };
    icons::draw(
        ui.painter(),
        egui::Rect::from_center_size(pos2(rect.left() + 19.0, rect.center().y), vec2(17.0, 17.0)),
        icon,
        color,
    );
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.into(),
        if selected {
            theme::bold(13.0)
        } else {
            FontId::proportional(13.0)
        },
        if selected { p.text } else { p.muted },
    );
    job.wrap.max_width = (rect.width() - 70.0).max(20.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(rect.left() + 37.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text,
    );
    if let Some(count) = count {
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            count.to_string(),
            FontId::proportional(11.0),
            color,
        );
    }
    response.on_hover_text(label)
}

fn note_card(ui: &mut Ui, note: &Note, selected: bool, p: Palette) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 108.0), Sense::click());
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, 8, if selected { p.selected } else { p.hover });
    }
    if selected {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                pos2(rect.left(), rect.top() + 13.0),
                vec2(3.0, rect.height() - 26.0),
            ),
            2,
            p.yellow,
        );
    }
    let left = rect.left() + 14.0;
    let width = (rect.width() - 28.0).max(40.0);
    let mut title = egui::text::LayoutJob::simple(
        note.display_title().into(),
        theme::bold(13.5),
        p.text,
        width,
    );
    title.wrap.max_rows = 2;
    title.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(title);
    ui.painter()
        .galley(pos2(left, rect.top() + 13.0), galley, p.text);
    let summary = if note.body.trim().is_empty() {
        "No additional text".into()
    } else {
        note.preview()
    };
    let mut preview =
        egui::text::LayoutJob::simple(summary, FontId::proportional(11.5), p.muted, width);
    preview.wrap.max_rows = 1;
    preview.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(preview);
    ui.painter()
        .galley(pos2(left, rect.top() + 55.0), galley, p.muted);
    ui.painter().text(
        pos2(left, rect.bottom() - 15.0),
        Align2::LEFT_CENTER,
        relative_date(note.updated_at),
        FontId::proportional(10.0),
        p.muted,
    );
    if note.pinned {
        icons::draw(
            ui.painter(),
            egui::Rect::from_min_size(
                pos2(rect.right() - 27.0, rect.bottom() - 22.0),
                vec2(13.0, 13.0),
            ),
            Icon::Pin,
            p.accent,
        );
    }
    response
}

fn section_label(ui: &mut Ui, text: &str, p: Palette) {
    ui.label(RichText::new(text).font(theme::bold(9.0)).color(p.muted));
}

fn dialog_heading(ui: &mut Ui, title: &str, subtitle: &str, p: Palette) {
    ui.add(egui::Label::new(RichText::new(title).font(theme::bold(23.0))).wrap());
    ui.add_space(5.0);
    ui.add(egui::Label::new(RichText::new(subtitle).size(12.0).color(p.muted)).wrap());
    ui.add_space(20.0);
}

fn dialog_buttons(ui: &mut Ui, label: &str, keep: &mut bool, p: Palette) -> bool {
    ui.add_space(17.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let clicked = ui
            .add(
                egui::Button::new(RichText::new(label).color(
                    if label.contains("Delete") || label.contains("Empty") {
                        p.red
                    } else {
                        p.text
                    },
                ))
                .fill(p.selected)
                .min_size(vec2(120.0, 35.0)),
            )
            .clicked();
        if ui
            .add(
                egui::Button::new("Cancel")
                    .frame(false)
                    .min_size(vec2(75.0, 35.0)),
            )
            .clicked()
        {
            *keep = false;
        }
        clicked
    })
    .inner
}

fn inline_error(ui: &mut Ui, error: &str, p: Palette) {
    if !error.is_empty() {
        ui.add_space(7.0);
        ui.add(egui::Label::new(RichText::new(error).size(11.0).color(p.red)).wrap());
    }
}

fn friendly_error(error: storage::Error) -> String {
    match &error {
        storage::Error::Database(rusqlite::Error::SqliteFailure(code, _))
            if code.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            "A folder with that name already exists. Try a different name.".into()
        }
        storage::Error::Io(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            "That file already exists. Choose a new name; Folio never overwrites your files.".into()
        }
        _ => error.to_string(),
    }
}

fn expand_home(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(dirs) = directories::BaseDirs::new()
    {
        return dirs.home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn relocate_file_path(path: &mut String, directory: &Path, operation: FileOperation) {
    let filename = if matches!(operation, FileOperation::Import) {
        None
    } else {
        Path::new(path).file_name().map(|name| name.to_os_string())
    };
    *path = directory
        .join(filename.unwrap_or_default())
        .to_string_lossy()
        .into_owned();
}

fn format_datetime(timestamp: i64) -> String {
    DateTime::from_timestamp_millis(timestamp)
        .map(|date| {
            date.with_timezone(&Local)
                .format("%B %-d, %Y  ·  %-I:%M %p")
                .to_string()
        })
        .unwrap_or_else(|| "Just now".into())
}

fn relative_date(timestamp: i64) -> String {
    let Some(date) =
        DateTime::from_timestamp_millis(timestamp).map(|date| date.with_timezone(&Local))
    else {
        return "Just now".into();
    };
    let days = (Local::now().date_naive() - date.date_naive()).num_days();
    match days {
        0 => date.format("%-I:%M %p").to_string(),
        1 => "Yesterday".into(),
        2..=6 => date.format("%A").to_string(),
        _ => date.format("%b %-d, %Y").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (tempfile::TempDir, egui::Context, Folio) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("data")).unwrap();
        store.seed_welcome().unwrap();
        let ctx = egui::Context::default();
        let app = Folio::new(&ctx, store, None);
        (dir, ctx, app)
    }

    #[test]
    fn switching_notes_flushes_pending_edits_without_waiting_for_autosave() {
        let (_dir, ctx, mut app) = app();
        let note = app.draft.as_mut().unwrap();
        let old_id = note.id;
        note.body = "Unsaved thought 🌿".into();
        app.mark_dirty();
        app.act(Action::NewNote, &ctx);
        assert_ne!(app.draft.as_ref().unwrap().id, old_id);
        assert_eq!(app.store.get(old_id).unwrap().body, "Unsaved thought 🌿");
        assert!(!app.dirty);
        assert!(!app.preview);
    }

    #[test]
    fn failed_saves_preserve_draft_and_block_navigation() {
        let (_dir, ctx, mut app) = app();
        let id = app.draft.as_ref().unwrap().id;
        app.draft.as_mut().unwrap().body = "Keep this edit".into();
        app.mark_dirty();
        // A note made unwritable at the storage layer simulates a rejected write.
        app.store.trash(id).unwrap();
        app.act(Action::Switch(Collection::Trash), &ctx);
        assert_eq!(app.collection, Collection::All);
        assert_eq!(app.draft.as_ref().unwrap().id, id);
        assert_eq!(app.draft.as_ref().unwrap().body, "Keep this edit");
        assert!(app.dirty);
        assert!(app.save_failed);
        assert!(app.notice.as_ref().unwrap().error);
        app.act(Action::NewNote, &ctx);
        assert_eq!(app.draft.as_ref().unwrap().id, id);
        app.store.restore(id).unwrap();
        app.act(Action::Save, &ctx);
        assert!(!app.dirty);
        assert!(!app.save_failed);
        assert!(app.notice.is_none());
        assert_eq!(app.store.get(id).unwrap().body, "Keep this edit");
    }

    #[test]
    fn trash_flushes_and_restore_preserves_content() {
        let (_dir, ctx, mut app) = app();
        let id = app.draft.as_ref().unwrap().id;
        app.draft.as_mut().unwrap().body = "Don't lose me".into();
        app.mark_dirty();
        app.act(Action::Trash(id), &ctx);
        assert_eq!(app.store.get(id).unwrap().body, "Don't lose me");
        app.act(Action::Switch(Collection::Trash), &ctx);
        assert_eq!(app.draft.as_ref().unwrap().id, id);
        app.act(Action::Restore(id), &ctx);
        assert!(app.store.get(id).unwrap().deleted_at.is_none());
    }

    #[test]
    fn pin_move_and_theme_are_persisted() {
        let (_dir, ctx, mut app) = app();
        let id = app.draft.as_ref().unwrap().id;
        let folder = app.store.create_folder("Testing").unwrap();
        app.act(Action::Move(id, Some(folder)), &ctx);
        app.act(Action::Pin(id, false), &ctx);
        assert_eq!(app.store.get(id).unwrap().folder_id, Some(folder));
        assert!(!app.store.get(id).unwrap().pinned);
        app.act(Action::ToggleTheme, &ctx);
        assert_eq!(app.store.setting("theme").unwrap().as_deref(), Some("dark"));
    }

    #[test]
    fn file_browser_retains_export_name_when_switching_directories() {
        let mut path = "/home/example/Documents/Thought.md".into();
        relocate_file_path(
            &mut path,
            Path::new("/home/example/Desktop"),
            FileOperation::Export,
        );
        assert_eq!(path, "/home/example/Desktop/Thought.md");
        relocate_file_path(&mut path, Path::new("/home/example"), FileOperation::Import);
        assert_eq!(Path::new(&path), Path::new("/home/example"));
    }

    #[test]
    fn copy_uses_the_whole_live_draft_without_saving_or_changing_it() {
        let (_dir, ctx, mut app) = app();
        let id = app.draft.as_ref().unwrap().id;
        let saved_body = app.store.get(id).unwrap().body;
        let body = format!(
            "{}\n\nThe last offscreen thought 🌿  \n",
            "A long note.\n".repeat(300)
        );
        app.draft.as_mut().unwrap().title = "A live draft".into();
        app.draft.as_mut().unwrap().body = body.clone();
        app.selection = 3..6;
        app.mark_dirty();
        let output = ctx.run_ui(egui::RawInput::default(), |_| {
            app.act(Action::CopyNote, &ctx)
        });
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == &format!("A live draft\n\n{body}"))
        }));
        assert!(app.dirty);
        assert_eq!(app.draft.as_ref().unwrap().body, body);
        assert_eq!(app.store.get(id).unwrap().body, saved_body);
    }

    #[test]
    fn whole_note_shortcut_does_not_leave_a_partial_copy_event() {
        let (_dir, ctx, mut app) = app();
        let input = egui::RawInput {
            events: vec![
                egui::Event::Key {
                    key: Key::C,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::CTRL | Modifiers::SHIFT,
                },
                egui::Event::Copy,
            ],
            ..Default::default()
        };
        let output = ctx.run_ui(input, |_| {
            app.shortcuts(&ctx);
            assert!(!ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy))
            }));
            for action in std::mem::take(&mut app.actions) {
                app.act(action, &ctx);
            }
        });
        assert!(output.platform_output.commands.iter().any(|command| {
            matches!(command, egui::OutputCommand::CopyText(text) if text == &app.draft.as_ref().unwrap().clipboard_text())
        }));
    }

    #[test]
    fn native_shifted_copy_and_batched_release_copy_the_entire_note() {
        for batched_release in [false, true] {
            let (_dir, ctx, mut app) = app();
            let mut events = vec![egui::Event::Copy];
            if batched_release {
                events.push(egui::Event::Key {
                    key: Key::C,
                    physical_key: Some(Key::C),
                    pressed: false,
                    repeat: false,
                    modifiers: Modifiers::CTRL | Modifiers::SHIFT,
                });
            }
            let input = egui::RawInput {
                events,
                modifiers: if batched_release {
                    Modifiers::NONE
                } else {
                    Modifiers::CTRL | Modifiers::SHIFT
                },
                ..Default::default()
            };
            let output = ctx.run_ui(input, |_| {
                app.shortcuts(&ctx);
                assert!(!ctx.input(|input| {
                    input
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::Copy))
                }));
                for action in std::mem::take(&mut app.actions) {
                    app.act(action, &ctx);
                }
            });
            assert!(output.platform_output.commands.iter().any(|command| {
                matches!(command, egui::OutputCommand::CopyText(text) if text == &app.draft.as_ref().unwrap().clipboard_text())
            }));
        }
    }

    #[test]
    fn ordinary_copy_is_left_to_the_text_selection() {
        let (_dir, ctx, mut app) = app();
        let input = egui::RawInput {
            events: vec![egui::Event::Copy],
            modifiers: Modifiers::CTRL,
            ..Default::default()
        };
        let output = ctx.run_ui(input, |_| {
            app.shortcuts(&ctx);
            assert!(app.actions.is_empty());
            assert!(ctx.input(|input| {
                input
                    .events
                    .iter()
                    .any(|event| matches!(event, egui::Event::Copy))
            }));
        });
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::CopyText(_)))
        );
    }
}
