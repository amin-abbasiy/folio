//! Pure, Unicode-safe Markdown editing operations. Cursor offsets are characters, not bytes.

use std::ops::Range;

#[derive(Clone, Copy)]
pub enum Format {
    Bold,
    Italic,
    Heading,
    Bullet,
    Checklist,
    Quote,
    Code,
}

pub fn apply(body: &mut String, selection: Range<usize>, format: Format) -> Range<usize> {
    let count = body.chars().count();
    let start = selection.start.min(count);
    let end = selection.end.min(count).max(start);
    match format {
        Format::Bold => wrap(body, start..end, "**", "bold text"),
        Format::Italic => wrap(body, start..end, "*", "italic text"),
        Format::Code => wrap(body, start..end, "`", "code"),
        Format::Heading => prefix_lines(body, start..end, "## "),
        Format::Bullet => prefix_lines(body, start..end, "- "),
        Format::Checklist => prefix_lines(body, start..end, "- [ ] "),
        Format::Quote => prefix_lines(body, start..end, "> "),
    }
}

fn byte_offset(text: &str, character: usize) -> usize {
    text.char_indices()
        .nth(character)
        .map_or(text.len(), |(index, _)| index)
}

fn wrap(body: &mut String, range: Range<usize>, marker: &str, placeholder: &str) -> Range<usize> {
    let start = byte_offset(body, range.start);
    let end = byte_offset(body, range.end);
    let selected = &body[start..end];
    let marker_chars = marker.chars().count();
    // Toggle formatting if the selected content is already wrapped.
    if start >= marker.len() && body[..start].ends_with(marker) && body[end..].starts_with(marker) {
        body.replace_range(end..end + marker.len(), "");
        body.replace_range(start - marker.len()..start, "");
        return range.start - marker_chars..range.end - marker_chars;
    }
    let text = if selected.is_empty() {
        placeholder
    } else {
        selected
    };
    let length = text.chars().count();
    body.replace_range(start..end, &format!("{marker}{text}{marker}"));
    range.start + marker_chars..range.start + marker_chars + length
}

fn prefix_lines(body: &mut String, range: Range<usize>, prefix: &str) -> Range<usize> {
    let start_byte = byte_offset(body, range.start);
    let end_byte = byte_offset(body, range.end);
    let start = body[..start_byte]
        .rfind('\n')
        .map_or(0, |position| position + 1);
    // A selection ending at the next line's start must not modify that unselected line.
    let effective_end = if range.end > range.start && body[..end_byte].ends_with('\n') {
        end_byte - 1
    } else {
        end_byte
    };
    let end = body[effective_end..]
        .find('\n')
        .map_or(body.len(), |position| effective_end + position);
    let text = &body[start..end];
    let remove = text.lines().all(|line| line.starts_with(prefix)) && !text.is_empty();
    let replacement = text
        .split('\n')
        .map(|line| {
            if remove {
                line.strip_prefix(prefix).unwrap_or(line).to_owned()
            } else {
                format!("{prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let character_start = body[..start].chars().count();
    let length = replacement.chars().count();
    body.replace_range(start..end, &replacement);
    character_start..character_start + length
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_unicode_without_splitting_utf8() {
        let mut text = "a 🌿 café".to_owned();
        let range = apply(&mut text, 2..3, Format::Bold);
        assert_eq!(text, "a **🌿** café");
        assert_eq!(range, 4..5);
        assert_eq!(apply(&mut text, range, Format::Bold), 2..3);
        assert_eq!(text, "a 🌿 café");
    }

    #[test]
    fn empty_selection_inserts_selected_placeholder() {
        let mut text = String::new();
        assert_eq!(apply(&mut text, 0..0, Format::Italic), 1..12);
        assert_eq!(text, "*italic text*");
    }

    #[test]
    fn list_prefixes_toggle_on_all_selected_lines() {
        let mut text = "one\ntwo\nthree".into();
        let range = apply(&mut text, 0..8, Format::Checklist);
        assert_eq!(text, "- [ ] one\n- [ ] two\nthree");
        apply(&mut text, range, Format::Checklist);
        assert_eq!(text, "one\ntwo\nthree");
    }

    #[test]
    fn offsets_are_clamped_after_note_switches() {
        let mut text = "Hi".into();
        apply(&mut text, 50..100, Format::Code);
        assert_eq!(text, "Hi`code`");
    }

    #[test]
    fn heading_handles_empty_and_last_lines() {
        let mut text = String::new();
        apply(&mut text, 0..0, Format::Heading);
        assert_eq!(text, "## ");
        let mut text = "α\nβ".into();
        apply(&mut text, 2..3, Format::Quote);
        assert_eq!(text, "α\n> β");
    }
}
