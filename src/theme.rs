use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub fn from_setting(value: Option<&str>) -> Self {
        if value == Some("dark") {
            Self::Dark
        } else {
            Self::Light
        }
    }

    pub fn setting(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub fn palette(self) -> Palette {
        match self {
            Self::Light => Palette {
                paper: Color32::from_rgb(255, 254, 250),
                sidebar: Color32::from_rgb(244, 242, 236),
                list: Color32::from_rgb(250, 249, 245),
                text: Color32::from_rgb(43, 45, 43),
                muted: Color32::from_rgb(133, 133, 122),
                faint: Color32::from_rgb(173, 172, 160),
                line: Color32::from_rgb(230, 228, 219),
                accent: Color32::from_rgb(169, 119, 20),
                yellow: Color32::from_rgb(235, 190, 75),
                selected: Color32::from_rgb(245, 233, 198),
                hover: Color32::from_rgb(236, 233, 224),
                green: Color32::from_rgb(81, 131, 98),
                red: Color32::from_rgb(180, 69, 63),
            },
            Self::Dark => Palette {
                paper: Color32::from_rgb(28, 29, 31),
                sidebar: Color32::from_rgb(33, 34, 36),
                list: Color32::from_rgb(31, 32, 34),
                text: Color32::from_rgb(235, 232, 222),
                muted: Color32::from_rgb(158, 157, 148),
                faint: Color32::from_rgb(105, 107, 102),
                line: Color32::from_rgb(53, 54, 54),
                accent: Color32::from_rgb(230, 187, 85),
                yellow: Color32::from_rgb(235, 190, 75),
                selected: Color32::from_rgb(65, 56, 35),
                hover: Color32::from_rgb(45, 46, 45),
                green: Color32::from_rgb(124, 176, 139),
                red: Color32::from_rgb(232, 135, 121),
            },
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let p = self.palette();
        let mut visuals = match self {
            Self::Light => egui::Visuals::light(),
            Self::Dark => egui::Visuals::dark(),
        };
        visuals.override_text_color = Some(p.text);
        visuals.panel_fill = p.paper;
        visuals.window_fill = p.paper;
        visuals.extreme_bg_color = p.list;
        visuals.faint_bg_color = p.sidebar;
        visuals.window_stroke = Stroke::new(1.0, p.line);
        visuals.window_corner_radius = 12.into();
        visuals.selection.bg_fill = p.selected;
        visuals.selection.stroke = Stroke::new(1.0, p.accent);
        visuals.hyperlink_color = p.accent;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.line);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.muted);
        visuals.widgets.inactive.weak_bg_fill = p.sidebar;
        visuals.widgets.inactive.bg_fill = p.sidebar;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, p.line);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        visuals.widgets.hovered.weak_bg_fill = p.hover;
        visuals.widgets.hovered.bg_fill = p.hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, p.faint);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
        visuals.widgets.active.weak_bg_fill = p.selected;
        visuals.widgets.active.bg_fill = p.selected;
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, p.accent);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = 6.into();
        }
        ctx.set_visuals(visuals);
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 8.0);
            style.spacing.interact_size.y = 30.0;
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(15.0));
            style
                .text_styles
                .insert(TextStyle::Button, FontId::proportional(14.0));
            style
                .text_styles
                .insert(TextStyle::Small, FontId::proportional(12.0));
            style.text_styles.insert(TextStyle::Heading, bold(22.0));
            style
                .text_styles
                .insert(TextStyle::Monospace, FontId::monospace(14.0));
        });
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub paper: Color32,
    pub sidebar: Color32,
    pub list: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub line: Color32,
    pub accent: Color32,
    pub yellow: Color32,
    pub selected: Color32,
    pub hover: Color32,
    pub green: Color32,
    pub red: Color32,
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("Inter Semibold".into()))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        FontData::from_static(include_bytes!("../assets/Inter-Regular.otf")).into(),
    );
    fonts.font_data.insert(
        "Inter Semibold".into(),
        FontData::from_static(include_bytes!("../assets/Inter-SemiBold.otf")).into(),
    );
    fonts
        .families
        .get_mut(&FontFamily::Proportional)
        .expect("default proportional font")
        .insert(0, "Inter".into());
    let mut bold_fallback = fonts.families[&FontFamily::Proportional].clone();
    bold_fallback.insert(0, "Inter Semibold".into());
    fonts
        .families
        .insert(FontFamily::Name("Inter Semibold".into()), bold_fallback);
    ctx.set_fonts(fonts);
}
