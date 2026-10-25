// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Lucide line icons (ISC, see THIRD_PARTY_NOTICES.md), embedded as SVG and
//! tinted when drawn. Made from the tracker's `lucide-vue-next` copy, so the
//! app and the website share one icon set.

use egui::{Color32, Image, ImageSource, Vec2};

macro_rules! icons {
    ($($name:ident => $file:literal),* $(,)?) => {
        $(
            pub fn $name() -> ImageSource<'static> {
                egui::include_image!(concat!("../../assets/icons/", $file, ".svg"))
            }
        )*
    };
}

icons! {
    arrow_left => "arrow-left",
    bug => "bug",
    check => "check",
    chevron_right => "chevron-right",
    circle_check => "circle-check",
    circle_x => "circle-x",
    cloud => "cloud",
    cloud_upload => "cloud-upload",
    copy => "copy",
    database => "database",
    download => "download",
    ellipsis => "ellipsis",
    external_link => "external-link",
    eye => "eye",
    folder_open => "folder-open",
    gamepad => "gamepad-2",
    github => "github",
    globe => "globe",
    heart => "heart",
    inbox => "inbox",
    info => "info",
    key_round => "key-round",
    log_in => "log-in",
    log_out => "log-out",
    minus => "minus",
    panel_bottom => "panel-bottom",
    power => "power",
    refresh => "refresh-cw",
    rotate_ccw => "rotate-ccw",
    settings => "settings",
    shield_alert => "shield-alert",
    shield_check => "shield-check",
    sparkles => "sparkles",
    trash => "trash-2",
    triangle_alert => "triangle-alert",
    x => "x",
}

/// An icon at `size` px in `color`.
pub fn image(source: ImageSource<'static>, size: f32, color: Color32) -> Image<'static> {
    Image::new(source)
        .fit_to_exact_size(Vec2::splat(size))
        .tint(color)
}
