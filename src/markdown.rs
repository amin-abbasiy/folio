//! An intentionally offline renderer: no HTML execution, image loading, or URL launching.

use std::ops::Range;

use eframe::egui::{self, FontId, RichText, Stroke, TextFormat, Ui, text::LayoutJob};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::selection::{self, SelectionScroll};
use crate::theme::{self, Palette};

#[derive(Clone, Default)]
struct Span {
    text: String,
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    link: Option<String>,
}

#[derive(Clone)]
struct Task {
    checked: bool,
    marker: Range<usize>,
}

enum Block {
    Text {
        spans: Vec<Span>,
        heading: u8,
        indent: usize,
        quote: bool,
        bullet: Option<String>,
        task: Option<Task>,
    },
    Code {
        text: String,
        language: String,
    },
    Rule,
}

#[derive(Default)]
pub struct Document {
    blocks: Vec<Block>,
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    spans: Vec<Span>,
    heading: u8,
    quote_depth: usize,
    lists: Vec<Option<u64>>,
    bullet: Option<String>,
    task: Option<Task>,
    bold: usize,
    italic: usize,
    strike: usize,
    link: Option<String>,
    code: Option<(String, String)>,
    image: Option<(String, String)>,
}

impl Builder {
    fn flush(&mut self) {
        if self.spans.is_empty() && self.task.is_none() {
            return;
        }
        self.blocks.push(Block::Text {
            spans: std::mem::take(&mut self.spans),
            heading: self.heading,
            indent: self.lists.len().saturating_sub(1),
            quote: self.quote_depth > 0,
            bullet: self.bullet.take(),
            task: self.task.take(),
        });
    }

    fn text(&mut self, text: &str, inline_code: bool) {
        if let Some((code, _)) = &mut self.code {
            code.push_str(text);
        } else if let Some((alt, _)) = &mut self.image {
            alt.push_str(text);
        } else {
            self.spans.push(Span {
                text: text.to_owned(),
                bold: self.bold > 0,
                italic: self.italic > 0,
                strike: self.strike > 0,
                code: inline_code,
                link: self.link.clone(),
            });
        }
    }
}

impl Document {
    pub fn parse(source: &str) -> Self {
        let mut builder = Builder::default();
        let options =
            Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
        for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
            match event {
                Event::Start(tag) => match tag {
                    Tag::Paragraph => {}
                    Tag::Heading { level, .. } => {
                        builder.flush();
                        builder.heading = level as u8;
                    }
                    Tag::BlockQuote(_) => {
                        builder.flush();
                        builder.quote_depth += 1;
                    }
                    Tag::List(start) => {
                        builder.flush();
                        builder.lists.push(start);
                    }
                    Tag::Item => {
                        builder.flush();
                        builder.bullet = Some(if let Some(Some(next)) = builder.lists.last_mut() {
                            let current = *next;
                            *next += 1;
                            format!("{current}.")
                        } else {
                            "•".into()
                        });
                    }
                    Tag::Strong => builder.bold += 1,
                    Tag::Emphasis => builder.italic += 1,
                    Tag::Strikethrough => builder.strike += 1,
                    Tag::CodeBlock(kind) => {
                        builder.flush();
                        let language = match kind {
                            CodeBlockKind::Fenced(language) => language.into_string(),
                            CodeBlockKind::Indented => String::new(),
                        };
                        builder.code = Some((String::new(), language));
                    }
                    Tag::Link { dest_url, .. } => builder.link = Some(dest_url.into_string()),
                    Tag::Image { dest_url, .. } => {
                        builder.image = Some((String::new(), dest_url.into_string()))
                    }
                    Tag::TableCell => {
                        if !builder.spans.is_empty() {
                            builder.text("    |    ", false);
                        }
                    }
                    _ => {}
                },
                Event::End(tag) => match tag {
                    TagEnd::Paragraph | TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead => {
                        builder.flush()
                    }
                    TagEnd::Heading(_) => {
                        builder.flush();
                        builder.heading = 0;
                    }
                    TagEnd::BlockQuote(_) => {
                        builder.flush();
                        builder.quote_depth = builder.quote_depth.saturating_sub(1);
                    }
                    TagEnd::List(_) => {
                        builder.flush();
                        builder.lists.pop();
                    }
                    TagEnd::Strong => builder.bold = builder.bold.saturating_sub(1),
                    TagEnd::Emphasis => builder.italic = builder.italic.saturating_sub(1),
                    TagEnd::Strikethrough => builder.strike = builder.strike.saturating_sub(1),
                    TagEnd::Link => builder.link = None,
                    TagEnd::CodeBlock => {
                        if let Some((text, language)) = builder.code.take() {
                            builder.blocks.push(Block::Code { text, language });
                        }
                    }
                    TagEnd::Image => {
                        if let Some((alt, destination)) = builder.image.take() {
                            builder.spans.push(Span {
                                text: format!(
                                    "[Image: {} — not loaded]",
                                    if alt.is_empty() { "untitled" } else { &alt }
                                ),
                                italic: true,
                                link: Some(destination),
                                ..Default::default()
                            });
                        }
                    }
                    _ => {}
                },
                Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                    builder.text(&text, false)
                }
                Event::Code(text) => builder.text(&text, true),
                Event::SoftBreak => builder.text(" ", false),
                Event::HardBreak => builder.text("\n", false),
                Event::Rule => {
                    builder.flush();
                    builder.blocks.push(Block::Rule);
                }
                Event::TaskListMarker(checked) => {
                    // Offsets originate in the parser, but verify the exact syntax before any edit.
                    if source
                        .get(range.clone())
                        .is_some_and(|text| matches!(text, "[ ]" | "[x]" | "[X]"))
                    {
                        builder.task = Some(Task {
                            checked,
                            marker: range,
                        });
                        builder.bullet = None;
                    }
                }
                Event::FootnoteReference(text) => builder.text(&format!("[{text}]"), false),
                _ => {}
            }
        }
        builder.flush();
        Self {
            blocks: builder.blocks,
        }
    }

    /// Returns a byte offset for an explicitly clicked checklist, never a URL or file operation.
    pub fn show(
        &self,
        ui: &mut Ui,
        p: Palette,
        interactive: bool,
        scrolling: &mut SelectionScroll,
    ) -> Option<(Range<usize>, bool)> {
        let mut toggle = None;
        ui.spacing_mut().item_spacing.y = 5.0;
        for (index, block) in self.blocks.iter().enumerate() {
            ui.push_id(index, |ui| match block {
                Block::Text { spans, heading, indent, quote, bullet, task } => {
                    if *heading > 0 { ui.add_space(14.0); }
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if *indent > 0 { ui.add_space(*indent as f32 * 18.0); }
                        if *quote {
                            ui.label(RichText::new("│").color(p.yellow).size(22.0));
                        }
                        if let Some(task) = task {
                            let mut checked = task.checked;
                            if ui.add_enabled(interactive, egui::Checkbox::without_text(&mut checked)).on_hover_text("Update this checklist").changed() {
                                toggle = Some((task.marker.clone(), checked));
                            }
                        } else if let Some(bullet) = bullet {
                            ui.label(RichText::new(bullet).color(p.accent).size(16.0));
                        }
                        let mut job = LayoutJob::default();
                        job.wrap.max_width = ui.available_width();
                        let size = match heading { 1 => 25.0, 2 => 20.0, 3 => 18.0, _ => 16.0 };
                        let mut links = Vec::new();
                        for span in spans {
                            let color = if span.link.is_some() { p.accent } else if *quote { p.muted } else { p.text };
                            let font_id = if span.code { FontId::monospace(size - 1.0) } else if span.bold || *heading > 0 { theme::bold(size) } else { FontId::proportional(size) };
                            job.append(&span.text, 0.0, TextFormat {
                                font_id,
                                color,
                                italics: span.italic || *quote,
                                strikethrough: if span.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
                                background: if span.code { p.sidebar } else { egui::Color32::TRANSPARENT },
                                line_height: Some(size * 1.6),
                                ..Default::default()
                            });
                            if let Some(link) = &span.link { links.push(link.as_str()); }
                        }
                        let response = selection::label(ui, job, p.text, if links.is_empty() { egui::Sense::hover() } else { egui::Sense::click() }, scrolling);
                        if !links.is_empty() {
                            if response.clicked() { ui.ctx().copy_text(links.join("\n")); }
                            response.on_hover_text(format!("{}\n\nClick to copy. Links and images are never opened or fetched by Folio.", links.join("\n")));
                        }
                    });
                    if bullet.is_none() && task.is_none() { ui.add_space(7.0); }
                }
                Block::Code { text, language } => {
                    ui.add_space(7.0);
                    egui::Frame::new().fill(p.sidebar).corner_radius(7).inner_margin(14).show(ui, |ui| {
                        ui.set_min_width((ui.available_width() - 2.0).max(0.0));
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(if language.is_empty() { "CODE" } else { language }).size(10.0).color(p.muted)).selectable(false));
                            if ui.small_button("Copy").clicked() { ui.ctx().copy_text(text.clone()); }
                        });
                        selection::label(ui, RichText::new(text.trim_end()).monospace().size(14.0), p.text, egui::Sense::hover(), scrolling);
                    });
                    ui.add_space(7.0);
                }
                Block::Rule => { ui.add_space(12.0); ui.separator(); ui.add_space(12.0); }
            });
        }
        toggle
    }
}

pub fn toggle_task(source: &mut String, range: Range<usize>, checked: bool) -> bool {
    if source
        .get(range.clone())
        .is_some_and(|text| matches!(text, "[ ]" | "[x]" | "[X]"))
    {
        source.replace_range(range, if checked { "[x]" } else { "[ ]" });
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_lists_code_and_unicode_tasks() {
        let source = "# Hello\n\n- [ ] café 🌿\n- [x] done\n\n```rust\nlet a = 1;\n```\n";
        let doc = Document::parse(source);
        assert!(
            doc.blocks
                .iter()
                .any(|block| matches!(block, Block::Code { language, .. } if language == "rust"))
        );
        let tasks: Vec<_> = doc
            .blocks
            .iter()
            .filter_map(|block| {
                if let Block::Text { task, .. } = block {
                    task.as_ref()
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(tasks.len(), 2);
        assert!(!tasks[0].checked);
        assert!(tasks[1].checked);
        let mut updated = source.to_owned();
        assert!(toggle_task(&mut updated, tasks[0].marker.clone(), true));
        assert!(updated.contains("[x] café 🌿"));
        assert!(!toggle_task(&mut updated, 999..1000, false));
    }

    #[test]
    fn images_are_inert_text_and_html_is_not_executed() {
        let doc =
            Document::parse("![alt](https://example.com/track.png)\n\n<script>bad()</script>");
        let text: String = doc
            .blocks
            .iter()
            .filter_map(|block| {
                if let Block::Text { spans, .. } = block {
                    Some(
                        spans
                            .iter()
                            .map(|span| span.text.as_str())
                            .collect::<String>(),
                    )
                } else {
                    None
                }
            })
            .collect();
        assert!(text.contains("not loaded"));
        assert!(text.contains("<script>bad()</script>"));
    }
}
