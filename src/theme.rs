use crate::config::CategoryColor;
use crate::model::{Event, Pool, RowKind, Severity};
use std::collections::HashMap;
use eframe::egui::{self, Color32, CornerRadius, Stroke};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub panel: Color32,
    pub panel2: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    pub pattern: Color32,
    pub sev_info: Color32,
    pub sev_warn: Color32,
    pub sev_err: Color32,
    pub sev_fatal: Color32,
    pub row_warn: Color32,
    pub row_err: Color32,
    pub row_fatal: Color32,
    pub row_info: Color32,
    pub row_usermsg: Color32,
    pub row_sel: Color32,
    pub search_hit: Color32,
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

impl Palette {
    pub fn light() -> Self {
        Palette {
            bg: Color32::from_rgb(0xf4, 0xf6, 0xf9),
            panel: Color32::from_rgb(0xff, 0xff, 0xff),
            panel2: Color32::from_rgb(0xec, 0xef, 0xf4),
            border: Color32::from_rgb(0xd5, 0xdb, 0xe5),
            text: Color32::from_rgb(0x1c, 0x22, 0x2b),
            text_weak: Color32::from_rgb(0x66, 0x70, 0x80),
            accent: Color32::from_rgb(0x1f, 0x6f, 0xeb),
            pattern: Color32::from_rgb(0xf0, 0x80, 0x10),
            sev_info: Color32::from_rgb(0x6b, 0x76, 0x87),
            sev_warn: Color32::from_rgb(0xc9, 0x9a, 0x00),
            sev_err: Color32::from_rgb(0xd9, 0x22, 0x22),
            sev_fatal: Color32::from_rgb(0x8e, 0x2b, 0xd6),
            row_warn: Color32::from_rgb(0xff, 0xe8, 0x7a),
            row_err: Color32::from_rgb(0xff, 0xa3, 0xa3),
            row_fatal: Color32::from_rgb(0xdc, 0xb0, 0xff),
            row_info: Color32::from_rgb(0xe2, 0xf7, 0xe2),
            row_usermsg: Color32::from_rgb(0x8c, 0xcf, 0x9c),
            row_sel: Color32::from_rgb(0xc4, 0xdc, 0xff),
            search_hit: Color32::from_rgba_unmultiplied(0x1f, 0x6f, 0xeb, 38),
        }
    }

    pub fn sev_color(&self, s: Severity) -> Color32 {
        match s {
            Severity::Info => self.sev_info,
            Severity::Warning => self.sev_warn,
            Severity::Error => self.sev_err,
            Severity::Fatal => self.sev_fatal,
            Severity::Other => self.text_weak,
        }
    }

    /// takes the configured Error / Warning / selection colors (used outside the table too)
    pub fn set_row_colors(&mut self, rc: &RowColors, selected: [u8; 3]) {
        let d = Palette::light();
        self.row_err = rc.named("error").unwrap_or(d.row_err);
        self.row_warn = rc.named("warning").unwrap_or(d.row_warn);
        self.row_fatal = rc.named("fatal").unwrap_or(d.row_fatal);
        self.row_info = rc.named("information").unwrap_or(d.row_info);
        self.row_usermsg = rc.named("usermessage").unwrap_or(d.row_usermsg);
        self.row_sel = rgb(selected);
    }

    pub fn mode_color(&self, mode: &str) -> Color32 {
        let a = 170;
        let m = mode.to_ascii_lowercase();
        let c = if m.contains("normal") {
            [0x3d, 0xc4, 0x6e]
        } else if m.contains("startup") {
            [0x3d, 0x9c, 0xff]
        } else if m.contains("shutdown") {
            [0x70, 0x78, 0x88]
        } else if m.contains("restart") {
            [0xff, 0x9f, 0x43]
        } else if m.contains("service") {
            [0xb0, 0x6c, 0xff]
        } else {
            [0x55, 0x60, 0x70]
        };
        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], a)
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = egui::Visuals::light();
        v.panel_fill = self.bg;
        v.window_fill = self.panel;
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(0xf7, 0xf9, 0xfc);
        v.window_stroke = Stroke::new(1.0_f32, self.border);
        v.window_corner_radius = CornerRadius::same(10);
        v.menu_corner_radius = CornerRadius::same(8);
        v.selection.bg_fill = Color32::from_rgb(0xc4, 0xdc, 0xff);
        v.selection.stroke = Stroke::new(1.0_f32, self.text);
        v.hyperlink_color = self.accent;
        v.override_text_color = Some(self.text);
        let r = CornerRadius::same(6);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, self.border);
        v.widgets.noninteractive.bg_fill = self.panel;
        v.widgets.inactive.weak_bg_fill = self.panel2;
        v.widgets.inactive.bg_fill = self.panel2;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, self.border);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xe0, 0xe6, 0xef);
        v.widgets.hovered.bg_fill = v.widgets.hovered.weak_bg_fill;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, self.accent);
        v.widgets.active.weak_bg_fill = self.accent;
        v.widgets.active.bg_fill = self.accent;
        v.striped = true;
        ctx.set_theme(egui::Theme::Light);
        ctx.set_visuals_of(egui::Theme::Light, v.clone());
        ctx.set_visuals_of(egui::Theme::Dark, v);

        ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = egui::vec2(6.0, 5.0);
            s.spacing.button_padding = egui::vec2(9.0, 4.0);
            s.spacing.interact_size.y = 24.0;
            s.interaction.selectable_labels = false;
            use egui::{FontFamily, FontId, TextStyle};
            s.text_styles.insert(TextStyle::Body, FontId::new(13.5, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Button, FontId::new(13.5, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
            s.text_styles.insert(TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace));
            s.text_styles.insert(TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional));
        });
    }
}

/// Row background per event category, built from the config list.
#[derive(Default, Clone)]
pub struct RowColors {
    map: HashMap<String, Option<Color32>>,
}

/// the default category whose color a new / unknown category starts with
pub fn fallback_key(k: RowKind) -> &'static str {
    match k {
        RowKind::Fatal => "fatal",
        RowKind::Error => "error",
        RowKind::Warning => "warning",
        RowKind::UserMessage => "usermessage",
        RowKind::Command => "command",
        RowKind::Info => "information",
        RowKind::Other => "",
    }
}

impl RowColors {
    pub fn new(list: &[CategoryColor]) -> Self {
        RowColors { map: list.iter().map(|c| (c.name.trim().to_lowercase(), c.color.map(rgb))).collect() }
    }
    pub fn contains(&self, name: &str) -> bool {
        self.map.contains_key(&name.trim().to_lowercase())
    }
    /// color of a category by name (lowercase)
    pub fn named(&self, lower: &str) -> Option<Color32> {
        self.map.get(lower).copied().flatten()
    }
    pub fn row_bg(&self, e: &Event, pool: &Pool) -> Option<Color32> {
        let low = match e.kind {
            RowKind::UserMessage => "usermessage",
            RowKind::Command => "command",
            _ => pool.lower(e.category).trim(),
        };
        match self.map.get(low) {
            Some(c) => *c,
            None => self.named(fallback_key(e.kind)),
        }
    }
}
