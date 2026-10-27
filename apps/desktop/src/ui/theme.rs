// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! The look: the tracker's design system tokens, mapped once into egui's
//! visuals and a [`Palette`] the widgets read. Widgets never use raw colours.
//!
//! Values are the design spec's (§1), the tracker's tokens at the desktop's 16 px root.

use egui::epaint::Shadow;
use egui::{
    Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Theme, Visuals,
};

use crate::i18n::Lang;

/// Every colour role the design uses, for one theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub bar: Color32,
    pub card: Color32,
    pub over: Color32,
    pub line: Color32,
    pub line2: Color32,
    pub tx: Color32,
    pub tx2: Color32,
    pub tx3: Color32,
    pub fill: Color32,
    pub ac_tx: Color32,
    pub ac_con: Color32,
    pub ac_on: Color32,
    pub ok: Color32,
    pub ok_con: Color32,
    pub ok_line: Color32,
    pub ok_ink: Color32,
    pub warn_con: Color32,
    pub warn_on: Color32,
    pub bad: Color32,
    pub bad_con: Color32,
    pub bad_line: Color32,
    pub bad_ink: Color32,
    pub amber: Color32,
    pub amber_glow: Color32,
    pub snack_bg: Color32,
    pub snack_tx: Color32,
    pub snack_ac: Color32,
    pub scrim: Color32,
    pub card_shadow: Color32,
    pub overlay_shadow: Color32,
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// An unmultiplied colour (the CSS `rgba()` the tokens are written in), as
/// egui's premultiplied one.
const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color32 {
    let alpha = (a * 255.0) as u16;
    Color32::from_rgba_premultiplied(
        premultiply(r, alpha),
        premultiply(g, alpha),
        premultiply(b, alpha),
        alpha as u8,
    )
}

const fn premultiply(channel: u8, alpha: u16) -> u8 {
    (channel as u16 * alpha / 255) as u8
}

pub const LIGHT: Palette = Palette {
    bg: hex(0xF8FAFC),
    bar: hex(0xFFFFFF),
    card: hex(0xFFFFFF),
    over: hex(0xF1F5F9),
    line: hex(0xE2E8F0),
    line2: hex(0xCBD5E1),
    tx: hex(0x0F172A),
    tx2: hex(0x475569),
    tx3: hex(0x64748B),
    fill: hex(0x4F46E5),
    ac_tx: hex(0x4F46E5),
    ac_con: hex(0xE0E7FF),
    ac_on: hex(0x3730A3),
    ok: hex(0x047857),
    ok_con: hex(0xECFDF5),
    ok_line: hex(0xA7F3D0),
    ok_ink: hex(0xFFFFFF),
    warn_con: hex(0xFEF3C7),
    warn_on: hex(0x92400E),
    bad: hex(0xB91C1C),
    bad_con: hex(0xFEF2F2),
    bad_line: hex(0xFECACA),
    bad_ink: hex(0xFFFFFF),
    amber: hex(0xF59E0B),
    amber_glow: rgba(245, 158, 11, 0.40),
    snack_bg: hex(0x1E293B),
    snack_tx: hex(0xF1F5F9),
    snack_ac: hex(0xA5B4FC),
    scrim: rgba(15, 23, 42, 0.45),
    card_shadow: rgba(15, 23, 42, 0.06),
    overlay_shadow: rgba(15, 23, 42, 0.18),
};

pub const DARK: Palette = Palette {
    bg: hex(0x0F172A),
    bar: hex(0x1E293B),
    card: hex(0x1E293B),
    over: hex(0x334155),
    line: hex(0x334155),
    line2: hex(0x475569),
    tx: hex(0xF1F5F9),
    tx2: hex(0xCBD5E1),
    tx3: hex(0x94A3B8),
    fill: hex(0x4F46E5),
    ac_tx: hex(0xA5B4FC),
    ac_con: hex(0x312E81),
    ac_on: hex(0xE0E7FF),
    ok: hex(0x6EE7B7),
    ok_con: rgba(6, 95, 70, 0.35),
    ok_line: rgba(16, 185, 129, 0.35),
    ok_ink: hex(0x064E3B),
    warn_con: rgba(120, 53, 15, 0.45),
    warn_on: hex(0xFCD34D),
    bad: hex(0xFCA5A5),
    bad_con: rgba(127, 29, 29, 0.30),
    bad_line: rgba(185, 28, 28, 0.60),
    bad_ink: hex(0x450A0A),
    amber: hex(0xF59E0B),
    amber_glow: rgba(245, 158, 11, 0.40),
    snack_bg: hex(0xE2E8F0),
    snack_tx: hex(0x0F172A),
    snack_ac: hex(0x4338CA),
    scrim: rgba(2, 6, 23, 0.60),
    card_shadow: rgba(0, 0, 0, 0.30),
    overlay_shadow: rgba(0, 0, 0, 0.45),
};

/// The palette of the theme egui is drawing in.
pub fn palette(ctx: &Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

/// The type styles of spec §1.2 that egui has no built-in name for.
pub mod style {
    use egui::TextStyle;

    pub fn display() -> TextStyle {
        TextStyle::Name("display".into())
    }
    pub fn about() -> TextStyle {
        TextStyle::Name("about".into())
    }
    pub fn tile() -> TextStyle {
        TextStyle::Name("tile".into())
    }
    pub fn dialog() -> TextStyle {
        TextStyle::Name("dialog".into())
    }
    pub fn brand() -> TextStyle {
        TextStyle::Name("brand".into())
    }
    pub fn secondary() -> TextStyle {
        TextStyle::Name("secondary".into())
    }
    pub fn small() -> TextStyle {
        TextStyle::Small
    }
}

/// Outfit's weights, as egui font families (egui has no weight axis).
pub mod weight {
    use egui::FontFamily;

    pub fn medium() -> FontFamily {
        FontFamily::Name("medium".into())
    }
    pub fn semibold() -> FontFamily {
        FontFamily::Name("semibold".into())
    }
}

/// Radii of spec §1.3.
pub mod radius {
    pub const HERO: u8 = 14;
    pub const CARD: u8 = 12;
    pub const DIALOG: u8 = 14;
    pub const MENU: u8 = 10;
    pub const BUTTON: u8 = 8;
}

/// Install the fonts, text styles and both themes' visuals. Called once.
pub fn install(ctx: &Context, lang: Lang) {
    ctx.set_fonts(fonts(lang, false));
    for theme in [Theme::Light, Theme::Dark] {
        let palette = match theme {
            Theme::Light => LIGHT,
            Theme::Dark => DARK,
        };
        ctx.set_visuals_of(theme, visuals(theme, &palette));
        ctx.style_mut_of(theme, |style| {
            style.text_styles = text_styles();
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.spacing.interact_size.y = 32.0;
            style.spacing.menu_margin = Margin::same(6);
            style.spacing.window_margin = Margin::same(20);
            style.interaction.selectable_labels = false;
            style.interaction.tooltip_delay = 0.25;
            style.animation_time = 0.15;
        });
    }
}

/// The fonts again after a language change (a script font in or out),
/// with (`open`) or without every language's own script font for
/// the language picker: each name is written in its own script, and those
/// fonts (tens of MB for Chinese) are held only while the list is open.
pub fn set_picker_fonts(ctx: &Context, lang: Lang, open: bool) {
    ctx.set_fonts(fonts(lang, open));
}

/// The font family a language's name is drawn in while the picker's fonts
/// are installed ([`set_picker_fonts`]); the usual one otherwise, and for
/// the languages Outfit writes.
pub fn name_family(installed: &[FontFamily], lang: Lang) -> Option<FontFamily> {
    let family = FontFamily::Name(format!("name-{}", lang.code()).into());
    installed.contains(&family).then_some(family)
}

/// The system's font for a script Outfit and egui's fonts lack, as
/// (path, face in the collection): read from disk only for the language
/// that needs it, so English pays nothing. The first file found is used.
fn script_font_candidates(lang: Lang) -> &'static [(&'static str, u32)] {
    #[cfg(windows)]
    {
        match lang {
            Lang::ZhCn => &[("msyh.ttc", 0), ("simsun.ttc", 0)],
            Lang::ZhTw => &[("msjh.ttc", 0), ("mingliu.ttc", 0)],
            Lang::Ja => &[("YuGothM.ttc", 0), ("meiryo.ttc", 0), ("msgothic.ttc", 0)],
            Lang::Ko => &[("malgun.ttf", 0), ("gulim.ttc", 0)],
            Lang::Th => &[("LeelawUI.ttf", 0), ("leelawad.ttf", 0), ("tahoma.ttf", 0)],
            Lang::Vi | Lang::Ru => &[("segoeui.ttf", 0)],
            _ => &[],
        }
    }
    #[cfg(target_os = "macos")]
    {
        match lang {
            Lang::ZhCn | Lang::ZhTw => &[
                ("/System/Library/Fonts/PingFang.ttc", 0),
                ("/System/Library/Fonts/STHeiti Light.ttc", 0),
            ],
            Lang::Ja => &[("/System/Library/Fonts/Hiragino Sans GB.ttc", 0)],
            Lang::Ko => &[("/System/Library/Fonts/AppleSDGothicNeo.ttc", 0)],
            Lang::Th => &[("/System/Library/Fonts/Thonburi.ttc", 0)],
            _ => &[],
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Noto Sans CJK's collection order: JP, KR, SC, TC, HK.
        const CJK: [&str; 4] = [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
        ];
        match lang {
            Lang::Ja => &[(CJK[0], 0), (CJK[1], 0), (CJK[2], 0), (CJK[3], 0)],
            Lang::Ko => &[(CJK[0], 1), (CJK[1], 1), (CJK[2], 1), (CJK[3], 1)],
            Lang::ZhCn => &[(CJK[0], 2), (CJK[1], 2), (CJK[2], 2), (CJK[3], 2)],
            Lang::ZhTw => &[(CJK[0], 3), (CJK[1], 3), (CJK[2], 3), (CJK[3], 3)],
            Lang::Th => &[
                ("/usr/share/fonts/truetype/noto/NotoSansThai-Regular.ttf", 0),
                ("/usr/share/fonts/noto/NotoSansThai-Regular.ttf", 0),
                ("/usr/share/fonts/google-noto/NotoSansThai-Regular.ttf", 0),
                ("/usr/share/fonts/truetype/tlwg/Loma.ttf", 0),
            ],
            _ => &[],
        }
    }
}

fn script_font(lang: Lang) -> Option<FontData> {
    for &(path, index) in script_font_candidates(lang) {
        #[cfg(windows)]
        let path =
            std::path::Path::new(&std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()))
                .join("Fonts")
                .join(path);
        #[cfg(not(windows))]
        let path = std::path::PathBuf::from(path);
        if let Ok(bytes) = std::fs::read(&path) {
            tracing::info!("font for {}: {}", lang.code(), path.display());
            let mut data = FontData::from_owned(bytes);
            data.index = index;
            return Some(data);
        }
    }
    None
}

fn fonts(lang: Lang, names: bool) -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [
        (
            "Outfit-Regular",
            &include_bytes!("../../assets/fonts/Outfit-Regular.ttf")[..],
        ),
        (
            "Outfit-Medium",
            &include_bytes!("../../assets/fonts/Outfit-Medium.ttf")[..],
        ),
        (
            "Outfit-SemiBold",
            &include_bytes!("../../assets/fonts/Outfit-SemiBold.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes).into());
    }
    // egui's own fonts stay behind Outfit, for the glyphs it lacks (✦,
    // symbols), and the language's script font right after Outfit (before
    // it for Vietnamese, whose stacked accents Outfit lacks).
    let mut fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let defaults = fallbacks.clone();
    let script = script_font(lang).map(|data| {
        fonts.font_data.insert("script".to_owned(), data.into());
        "script".to_owned()
    });
    if names {
        for other in Lang::ALL {
            let font = if other == lang {
                script.clone()
            } else {
                script_font(other).map(|data| {
                    let name = format!("script-{}", other.code());
                    fonts.font_data.insert(name.clone(), data.into());
                    name
                })
            };
            let Some(font) = font else {
                continue;
            };
            let mut list = vec![font, "Outfit-SemiBold".to_owned()];
            list.extend(defaults.iter().cloned());
            fonts.families.insert(
                FontFamily::Name(format!("name-{}", other.code()).into()),
                list,
            );
        }
    }
    let script_first = lang == Lang::Vi;
    if let Some(name) = &script
        && !script_first
    {
        fallbacks.insert(0, name.clone());
    }
    let family = |first: &str| {
        let mut list = Vec::new();
        if script_first && let Some(name) = &script {
            list.push(name.clone());
        }
        list.push(first.to_owned());
        list.extend(fallbacks.iter().cloned());
        list
    };
    fonts
        .families
        .insert(FontFamily::Proportional, family("Outfit-Regular"));
    fonts
        .families
        .insert(weight::medium(), family("Outfit-Medium"));
    fonts
        .families
        .insert(weight::semibold(), family("Outfit-SemiBold"));
    fonts
}

fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    use FontFamily::{Monospace, Proportional};
    [
        (TextStyle::Heading, FontId::new(20.0, weight::semibold())),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(13.0, weight::semibold())),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, Monospace)),
        (style::display(), FontId::new(28.0, weight::semibold())),
        (style::about(), FontId::new(22.0, weight::semibold())),
        (style::tile(), FontId::new(18.0, weight::semibold())),
        (style::dialog(), FontId::new(17.0, weight::semibold())),
        (style::brand(), FontId::new(15.0, weight::semibold())),
        (style::secondary(), FontId::new(13.0, Proportional)),
    ]
    .into()
}

fn visuals(theme: Theme, p: &Palette) -> Visuals {
    let mut v = match theme {
        Theme::Light => Visuals::light(),
        Theme::Dark => Visuals::dark(),
    };
    v.override_text_color = Some(p.tx);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.bg;
    v.faint_bg_color = p.over;
    v.code_bg_color = p.over;
    v.hyperlink_color = p.ac_tx;
    v.warn_fg_color = p.warn_on;
    v.error_fg_color = p.bad;
    v.window_corner_radius = CornerRadius::same(radius::DIALOG);
    v.menu_corner_radius = CornerRadius::same(radius::MENU);
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: p.overlay_shadow,
    };
    v.popup_shadow = v.window_shadow;
    v.selection.bg_fill = p.fill;
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.text_cursor.stroke = Stroke::new(2.0_f32, p.ac_tx);

    let w = &mut v.widgets;
    for (state, fill, stroke) in [
        (&mut w.noninteractive, p.card, p.line),
        (&mut w.inactive, p.card, p.line2),
        (&mut w.hovered, p.over, p.line2),
        (&mut w.active, p.over, p.ac_tx),
        (&mut w.open, p.over, p.line2),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::new(1.0_f32, stroke);
        state.corner_radius = CornerRadius::same(radius::BUTTON);
        state.fg_stroke = Stroke::new(1.5_f32, p.tx);
        state.expansion = 0.0;
    }
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.tx2);
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    v
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// The embedded fonts are the upstream files byte for byte. Stored as text,
    /// git once dropped their CR bytes and the release drew words with letters
    /// missing (`.gitattributes` now marks them binary).
    #[test]
    fn the_embedded_fonts_are_intact() {
        for (name, bytes, expected) in [
            (
                "Outfit-Regular",
                &include_bytes!("../../assets/fonts/Outfit-Regular.ttf")[..],
                "3b64ac4f6ab6a8eebddd4b0bc03c811c43602e11e176382ab0ee6be615ab861b",
            ),
            (
                "Outfit-Medium",
                &include_bytes!("../../assets/fonts/Outfit-Medium.ttf")[..],
                "dc8d9212fc57556a55d01e863071f727cd6264b748e28885d853807aeb186142",
            ),
            (
                "Outfit-SemiBold",
                &include_bytes!("../../assets/fonts/Outfit-SemiBold.ttf")[..],
                "bf2e1d2a6ec2a67952e8b36edd2b2bb9f340c0cdd10b0ad5145b4dbbc1339608",
            ),
        ] {
            assert_eq!(sha256(bytes), expected, "{name}.ttf was altered");
        }
    }
}
