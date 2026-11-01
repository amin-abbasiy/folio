//! Selection scrolling is explicit: pointer movement alone must not be required
//! to keep scrolling, and selecting note text must not drag the whole pane.

use std::time::Duration;

use eframe::egui::{
    self, Color32, Rect, Response, Sense, Stroke, Ui, WidgetInfo, WidgetText, WidgetType,
    text_selection::LabelSelectionState, vec2,
};

#[derive(Default)]
pub struct SelectionScroll {
    dragging: bool,
    viewport: Option<Rect>,
}

impl SelectionScroll {
    /// Keep a captured selection endpoint at the visible edge while the physical
    /// mouse continues outside the pane. Other UI input is never clamped.
    pub fn capture_pointer(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if !self.dragging || !input.focused {
            return;
        }
        let Some(viewport) = self
            .viewport
            .filter(|rect| rect.width() > 2.0 && rect.height() > 2.0)
        else {
            return;
        };
        let viewport = viewport.shrink(1.0);
        let mut saw_position = false;
        for event in &mut input.events {
            if !self.dragging {
                break;
            }
            match event {
                egui::Event::PointerMoved(position) => {
                    *position = viewport.clamp(*position);
                    saw_position = true;
                }
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    *pos = viewport.clamp(*pos);
                    saw_position = true;
                    // A release and the next click can arrive in the same frame.
                    // Do not capture a subsequent click on Copy note or other UI.
                    if !*pressed {
                        self.dragging = false;
                    }
                }
                egui::Event::PointerGone => {
                    self.dragging = false;
                    return;
                }
                _ => {}
            }
        }
        if !saw_position
            && let Some(position) = ctx.pointer_interact_pos()
            && !viewport.contains(position)
        {
            input
                .events
                .insert(0, egui::Event::PointerMoved(viewport.clamp(position)));
        }
    }

    pub fn observe(&mut self, response: &Response) {
        if response.is_pointer_button_down_on()
            && response.ctx.input(|input| input.pointer.primary_down())
        {
            self.dragging = true;
        }
    }

    pub fn finish(&mut self, ui: &Ui, viewport: Rect) {
        self.viewport = Some(viewport);
        let (down, focused, pointer, elapsed) = ui.input(|input| {
            (
                input.pointer.primary_down(),
                input.focused,
                input.pointer.interact_pos(),
                input.unstable_dt,
            )
        });
        self.dragging &= down && focused && pointer.is_some();
        if !self.dragging {
            return;
        }
        // The text widgets can otherwise jump straight to an offscreen cursor.
        // During a pointer selection, only our bounded edge scroll drives the pane.
        ui.scroll_to_rect(viewport, None);
        if let Some(pointer) = pointer {
            let delta = edge_delta(viewport, pointer, elapsed);
            if delta != 0.0 {
                ui.scroll_with_delta_animation(
                    vec2(0.0, delta),
                    egui::style::ScrollAnimation::none(),
                );
                ui.request_repaint_after(Duration::from_millis(16));
            }
        }
    }
}

fn edge_delta(viewport: Rect, pointer: egui::Pos2, elapsed: f32) -> f32 {
    if pointer.x < viewport.left() - 32.0 || pointer.x > viewport.right() + 32.0 {
        return 0.0;
    }
    let band = 32.0_f32.min(viewport.height() / 3.0);
    if band <= 0.0 {
        return 0.0;
    }
    let strength = if pointer.y < viewport.top() + band {
        ((viewport.top() + band - pointer.y) / band).min(1.0)
    } else if pointer.y > viewport.bottom() - band {
        -((pointer.y - (viewport.bottom() - band)) / band).min(1.0)
    } else {
        0.0
    };
    strength * 720.0 * elapsed.clamp(0.0, 0.1)
}

/// Unlike the stock Label widget, keep offscreen note paragraphs in the selection
/// model. Otherwise egui clears a selection as soon as its anchor scrolls away.
pub fn label(
    ui: &mut Ui,
    text: impl Into<WidgetText>,
    color: Color32,
    sense: Sense,
    scrolling: &mut SelectionScroll,
) -> Response {
    let (position, galley, response) = egui::Label::new(text)
        .wrap()
        .selectable(true)
        .sense(sense)
        .layout_in_ui(ui);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), galley.text()));
    LabelSelectionState::label_text_selection(ui, &response, position, galley, color, Stroke::NONE);
    scrolling.observe(&response);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn edges_scroll_in_opposite_directions_and_center_stays_still() {
        let viewport = Rect::from_min_max(pos2(100.0, 100.0), pos2(500.0, 500.0));
        assert!(edge_delta(viewport, pos2(200.0, 105.0), 1.0 / 60.0) > 0.0);
        assert!(edge_delta(viewport, pos2(200.0, 495.0), 1.0 / 60.0) < 0.0);
        assert_eq!(edge_delta(viewport, pos2(200.0, 300.0), 1.0), 0.0);
        assert_eq!(edge_delta(viewport, pos2(20.0, 495.0), 1.0), 0.0);
    }

    #[test]
    fn scrolling_is_bounded_and_independent_of_frame_rate() {
        let viewport = Rect::from_min_max(pos2(100.0, 100.0), pos2(500.0, 500.0));
        let pointer = pos2(200.0, 800.0);
        let at_60 = edge_delta(viewport, pointer, 1.0 / 60.0) * 60.0;
        let at_30 = edge_delta(viewport, pointer, 1.0 / 30.0) * 30.0;
        let at_10 = edge_delta(viewport, pointer, 1.0 / 10.0) * 10.0;
        assert!((at_60 - at_30).abs() < 0.01);
        assert!((at_60 - at_10).abs() < 0.01);
        assert!(edge_delta(viewport, pointer, 30.0).abs() <= 72.0);
    }

    #[test]
    fn pointer_capture_only_applies_to_an_active_note_selection() {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input
            .events
            .push(egui::Event::PointerMoved(pos2(900.0, 900.0)));
        let mut scrolling = SelectionScroll {
            dragging: false,
            viewport: Some(Rect::from_min_max(pos2(100.0, 100.0), pos2(500.0, 500.0))),
        };
        scrolling.capture_pointer(&ctx, &mut input);
        assert!(
            matches!(input.events[0], egui::Event::PointerMoved(pos) if pos == pos2(900.0, 900.0))
        );
        scrolling.dragging = true;
        scrolling.capture_pointer(&ctx, &mut input);
        assert!(
            matches!(input.events[0], egui::Event::PointerMoved(pos) if pos == pos2(499.0, 499.0))
        );
        input.focused = false;
        input.events[0] = egui::Event::PointerMoved(pos2(1.0, 1.0));
        scrolling.capture_pointer(&ctx, &mut input);
        assert!(matches!(input.events[0], egui::Event::PointerMoved(pos) if pos == pos2(1.0, 1.0)));

        input.focused = true;
        input.events = vec![
            egui::Event::PointerButton {
                pos: pos2(900.0, 900.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerMoved(pos2(10.0, 10.0)),
            egui::Event::PointerButton {
                pos: pos2(10.0, 10.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ];
        scrolling.capture_pointer(&ctx, &mut input);
        assert!(!scrolling.dragging);
        assert!(
            matches!(input.events[0], egui::Event::PointerButton { pos, .. } if pos == pos2(499.0, 499.0))
        );
        assert!(
            matches!(input.events[1], egui::Event::PointerMoved(pos) if pos == pos2(10.0, 10.0))
        );
        assert!(
            matches!(input.events[2], egui::Event::PointerButton { pos, .. } if pos == pos2(10.0, 10.0))
        );
    }
}
