use eframe::egui::{
    self, Color32, Painter, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};

#[derive(Clone, Copy)]
pub enum Icon {
    Notes,
    Folder,
    Pin,
    Trash,
    Search,
    Plus,
    Sun,
    Moon,
    Sidebar,
    Help,
    More,
    Restore,
    Eye,
    Edit,
    ChevronDown,
}

pub fn button(ui: &mut Ui, icon: Icon, color: Color32, tooltip: &str) -> Response {
    let response = ui.add(
        egui::Button::new("")
            .frame(false)
            .min_size(vec2(32.0, 32.0)),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(response.rect, 6, ui.visuals().widgets.hovered.bg_fill);
    }
    draw(ui.painter(), response.rect.shrink(7.0), icon, color);
    response.on_hover_text(tooltip)
}

pub fn small(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    draw(ui.painter(), rect, icon, color);
}

pub fn draw(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let at = |x: f32, y: f32| {
        pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let stroke = Stroke::new(1.5, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    let path = |points: &[(f32, f32)]| {
        p.add(Shape::line(
            points.iter().map(|&(x, y)| at(x, y)).collect(),
            stroke,
        ));
    };
    match icon {
        Icon::Notes => {
            p.rect_stroke(
                Rect::from_min_max(at(0.2, 0.1), at(0.82, 0.9)),
                2,
                stroke,
                StrokeKind::Inside,
            );
            line((0.34, 0.34), (0.68, 0.34));
            line((0.34, 0.5), (0.68, 0.5));
            line((0.34, 0.66), (0.57, 0.66));
        }
        Icon::Folder => {
            path(&[
                (0.09, 0.32),
                (0.09, 0.19),
                (0.42, 0.19),
                (0.52, 0.32),
                (0.91, 0.32),
                (0.91, 0.82),
                (0.09, 0.82),
                (0.09, 0.32),
                (0.91, 0.32),
            ]);
        }
        Icon::Pin => {
            path(&[
                (0.34, 0.12),
                (0.73, 0.12),
                (0.66, 0.23),
                (0.66, 0.44),
                (0.82, 0.59),
                (0.25, 0.59),
                (0.41, 0.44),
                (0.41, 0.23),
                (0.34, 0.12),
            ]);
            line((0.53, 0.59), (0.53, 0.92));
        }
        Icon::Trash => {
            path(&[(0.22, 0.29), (0.28, 0.89), (0.74, 0.89), (0.8, 0.29)]);
            line((0.12, 0.26), (0.89, 0.26));
            path(&[(0.35, 0.26), (0.35, 0.1), (0.66, 0.1), (0.66, 0.26)]);
            line((0.42, 0.41), (0.44, 0.73));
            line((0.61, 0.41), (0.59, 0.73));
        }
        Icon::Search => {
            p.circle_stroke(at(0.43, 0.43), rect.width() * 0.29, stroke);
            line((0.65, 0.65), (0.9, 0.9));
        }
        Icon::Plus => {
            line((0.5, 0.15), (0.5, 0.85));
            line((0.15, 0.5), (0.85, 0.5));
        }
        Icon::Sun => {
            p.circle_stroke(at(0.5, 0.5), rect.width() * 0.2, stroke);
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::TAU / 8.0;
                line(
                    (0.5 + angle.cos() * 0.34, 0.5 + angle.sin() * 0.34),
                    (0.5 + angle.cos() * 0.47, 0.5 + angle.sin() * 0.47),
                );
            }
        }
        Icon::Moon => {
            path(&[
                (0.76, 0.73),
                (0.6, 0.83),
                (0.4, 0.84),
                (0.23, 0.7),
                (0.15, 0.49),
                (0.2, 0.29),
                (0.36, 0.14),
                (0.48, 0.12),
                (0.42, 0.3),
                (0.43, 0.48),
                (0.54, 0.65),
                (0.76, 0.73),
            ]);
        }
        Icon::Sidebar => {
            p.rect_stroke(
                Rect::from_min_max(at(0.1, 0.16), at(0.9, 0.84)),
                2,
                stroke,
                StrokeKind::Inside,
            );
            line((0.37, 0.16), (0.37, 0.84));
        }
        Icon::Help => {
            p.circle_stroke(at(0.5, 0.5), rect.width() * 0.4, stroke);
            path(&[
                (0.37, 0.37),
                (0.42, 0.28),
                (0.55, 0.27),
                (0.63, 0.35),
                (0.61, 0.44),
                (0.5, 0.52),
                (0.5, 0.61),
            ]);
            p.circle_filled(at(0.5, 0.74), 1.0, color);
        }
        Icon::More => {
            for x in [0.2, 0.5, 0.8] {
                p.circle_filled(at(x, 0.5), 1.6, color);
            }
        }
        Icon::Restore => {
            path(&[
                (0.14, 0.46),
                (0.26, 0.24),
                (0.52, 0.16),
                (0.75, 0.29),
                (0.85, 0.52),
                (0.78, 0.74),
                (0.57, 0.85),
                (0.36, 0.8),
            ]);
            path(&[(0.11, 0.19), (0.11, 0.48), (0.41, 0.48)]);
        }
        Icon::Eye => {
            path(&[
                (0.07, 0.5),
                (0.25, 0.29),
                (0.5, 0.2),
                (0.75, 0.29),
                (0.93, 0.5),
                (0.75, 0.71),
                (0.5, 0.8),
                (0.25, 0.71),
                (0.07, 0.5),
            ]);
            p.circle_stroke(at(0.5, 0.5), rect.width() * 0.14, stroke);
        }
        Icon::Edit => {
            path(&[
                (0.18, 0.66),
                (0.69, 0.15),
                (0.85, 0.31),
                (0.34, 0.82),
                (0.12, 0.88),
                (0.18, 0.66),
                (0.34, 0.82),
            ]);
            line((0.59, 0.25), (0.75, 0.41));
        }
        Icon::ChevronDown => path(&[(0.2, 0.35), (0.5, 0.65), (0.8, 0.35)]),
    }
}

pub fn logo(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 8, Color32::from_rgb(226, 178, 61));
    let page = Rect::from_min_max(
        rect.min + rect.size() * vec2(0.25, 0.18),
        rect.max - rect.size() * vec2(0.24, 0.15),
    );
    painter.rect_filled(page, 3, Color32::from_rgb(255, 253, 243));
    for (y, width) in [(0.42, 0.31), (0.57, 0.31), (0.72, 0.21)] {
        let start = rect.min + rect.size() * vec2(0.35, y);
        painter.line_segment(
            [start, start + vec2(rect.width() * width, 0.0)],
            Stroke::new(1.4, Color32::from_rgb(174, 154, 107)),
        );
    }
}

pub fn app_icon() -> egui::IconData {
    let size = 64_usize;
    let mut rgba = vec![0; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let page = (17..48).contains(&x) && (12..54).contains(&y);
            let rule = (24..41).contains(&x) && [30, 31, 37, 38, 44, 45].contains(&y);
            let color = if rule {
                [180, 164, 128, 255]
            } else if page {
                [255, 253, 243, 255]
            } else {
                [226, 178, 61, 255]
            };
            let index = (y * size + x) * 4;
            rgba[index..index + 4].copy_from_slice(&color);
        }
    }
    egui::IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}
