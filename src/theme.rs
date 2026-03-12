use egui::Color32;

pub struct CatppuccinMocha;

impl CatppuccinMocha {
    // Base colors
    pub const BASE: Color32 = Color32::from_rgb(30, 30, 46);
    pub const MANTLE: Color32 = Color32::from_rgb(24, 24, 37);
    pub const CRUST: Color32 = Color32::from_rgb(17, 17, 27);
    pub const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68);
    pub const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90);
    pub const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112);
    pub const OVERLAY0: Color32 = Color32::from_rgb(108, 112, 134);
    pub const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156);
    pub const TEXT: Color32 = Color32::from_rgb(205, 214, 244);
    pub const SUBTEXT0: Color32 = Color32::from_rgb(166, 173, 200);
    pub const SUBTEXT1: Color32 = Color32::from_rgb(186, 194, 222);

    // Accent colors
    pub const BLUE: Color32 = Color32::from_rgb(137, 180, 250);
    pub const GREEN: Color32 = Color32::from_rgb(166, 227, 161);
    pub const PEACH: Color32 = Color32::from_rgb(250, 179, 135);
    pub const MAUVE: Color32 = Color32::from_rgb(203, 166, 247);
    pub const RED: Color32 = Color32::from_rgb(243, 139, 168);
    pub const YELLOW: Color32 = Color32::from_rgb(249, 226, 175);
    pub const TEAL: Color32 = Color32::from_rgb(148, 226, 213);
    pub const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254);
    pub const FLAMINGO: Color32 = Color32::from_rgb(242, 205, 205);
    pub const ROSEWATER: Color32 = Color32::from_rgb(245, 224, 220);
    pub const SKY: Color32 = Color32::from_rgb(137, 220, 235);
    pub const SAPPHIRE: Color32 = Color32::from_rgb(116, 199, 236);
    pub const MAROON: Color32 = Color32::from_rgb(235, 160, 172);
    pub const PINK: Color32 = Color32::from_rgb(245, 194, 231);

    pub fn apply(ctx: &egui::Context) {
        let mut style = (*ctx.style()).clone();
        let visuals = &mut style.visuals;

        visuals.dark_mode = true;
        visuals.override_text_color = Some(Self::TEXT);
        visuals.panel_fill = Self::BASE;
        visuals.window_fill = Self::BASE;
        visuals.extreme_bg_color = Self::CRUST;
        visuals.faint_bg_color = Self::MANTLE;
        visuals.code_bg_color = Self::SURFACE0;

        visuals.widgets.noninteractive.bg_fill = Self::BASE;
        visuals.widgets.noninteractive.fg_stroke.color = Self::SUBTEXT0;
        visuals.widgets.noninteractive.bg_stroke.color = Self::SURFACE1;

        visuals.widgets.inactive.bg_fill = Self::SURFACE0;
        visuals.widgets.inactive.fg_stroke.color = Self::TEXT;
        visuals.widgets.inactive.bg_stroke.color = Self::SURFACE1;

        visuals.widgets.hovered.bg_fill = Self::SURFACE1;
        visuals.widgets.hovered.fg_stroke.color = Self::TEXT;
        visuals.widgets.hovered.bg_stroke.color = Self::BLUE;

        visuals.widgets.active.bg_fill = Self::SURFACE2;
        visuals.widgets.active.fg_stroke.color = Self::TEXT;
        visuals.widgets.active.bg_stroke.color = Self::BLUE;

        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(137, 180, 250, 40);
        visuals.selection.stroke.color = Self::BLUE;

        visuals.hyperlink_color = Self::BLUE;
        visuals.warn_fg_color = Self::YELLOW;
        visuals.error_fg_color = Self::RED;

        visuals.window_stroke.color = Self::SURFACE1;
        visuals.window_shadow.color = Color32::from_rgba_premultiplied(0, 0, 0, 60);

        ctx.set_style(style);
    }
}

pub fn type_color(type_name: &str) -> Color32 {
    match type_name {
        "i64" | "u64" | "f64" => CatppuccinMocha::PEACH,
        "String" => CatppuccinMocha::GREEN,
        "bool" => CatppuccinMocha::MAUVE,
        "null" | "None" => CatppuccinMocha::OVERLAY0,
        s if s.starts_with("Vec<") => CatppuccinMocha::YELLOW,
        s if s.starts_with("Option<") || s.ends_with("?") => CatppuccinMocha::FLAMINGO,
        "Mixed" => CatppuccinMocha::YELLOW,
        s if s.contains("DateTime") || s.contains("Date") || s.contains("Time") => {
            CatppuccinMocha::BLUE
        }
        _ => CatppuccinMocha::LAVENDER, // structs
    }
}
