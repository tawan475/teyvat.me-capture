// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Toasts (spec §3 `Toast`, §8): inverse snackbars, bottom right above the
//! footer, newest on top, at most three, with an optional action.
//!
//! Cheap by design: nothing animates. A visible toast asks for one repaint
//! when the next one expires, so an idle window with a toast up draws one
//! more frame, not sixty a second.

use std::time::{Duration, Instant};

use egui::{Align2, Area, Context, CornerRadius, Frame, Id, Margin, Order, RichText, vec2};

use super::icons;
use super::theme::{self, style, weight};
use crate::activity::Kind;

const MAX_SHOWN: usize = 3;
const MAX_WIDTH: f32 = 400.0;

/// What a toast's button does; the app acts on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Reveal this file in the file manager.
    Show(std::path::PathBuf),
    /// Open this page anyway (the achievement links).
    OpenAnyway(String),
    /// Try the manual upload again.
    RetryUpload,
}

impl Action {
    fn label(&self) -> &'static str {
        match self {
            Action::Show(_) => "Show",
            Action::OpenAnyway(_) => "Open anyway",
            Action::RetryUpload => "Retry",
        }
    }
}

pub struct Toast {
    kind: Kind,
    text: String,
    action: Option<Action>,
    shown_at: Instant,
    duration: Option<Duration>,
    saved: bool,
}

impl Toast {
    /// How long it stays (`None`: until dismissed).
    pub fn duration(&mut self, duration: Option<Duration>) -> &mut Self {
        self.duration = duration;
        self
    }

    pub fn action(&mut self, action: Action) -> &mut Self {
        self.action = Some(action);
        self
    }

    /// Draw the download icon (a file was saved).
    pub fn saved(&mut self) -> &mut Self {
        self.saved = true;
        self
    }

    fn default_duration(kind: Kind) -> Duration {
        match kind {
            Kind::Ok | Kind::Info => Duration::from_secs(4),
            Kind::Warn => Duration::from_secs(15),
            Kind::Error => Duration::from_secs(8),
        }
    }

    fn remaining(&self, now: Instant) -> Option<Duration> {
        self.duration
            .map(|duration| duration.saturating_sub(now - self.shown_at))
    }
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    /// Every toast raised, for the activity log; the app drains it.
    raised: Vec<(Kind, String)>,
}

impl Toasts {
    fn push(&mut self, kind: Kind, text: impl Into<String>) -> &mut Toast {
        let text = text.into();
        self.raised.push((kind, text.clone()));
        self.items.push(Toast {
            kind,
            text,
            action: None,
            shown_at: Instant::now(),
            duration: Some(Toast::default_duration(kind)),
            saved: false,
        });
        self.items.last_mut().expect("just pushed")
    }

    pub fn success(&mut self, text: impl Into<String>) -> &mut Toast {
        self.push(Kind::Ok, text)
    }

    pub fn info(&mut self, text: impl Into<String>) -> &mut Toast {
        self.push(Kind::Info, text)
    }

    pub fn warning(&mut self, text: impl Into<String>) -> &mut Toast {
        self.push(Kind::Warn, text)
    }

    pub fn error(&mut self, text: impl Into<String>) -> &mut Toast {
        self.push(Kind::Error, text)
    }

    /// A toast for an event the activity log already has.
    pub fn quiet(&mut self, kind: Kind, text: impl Into<String>) -> &mut Toast {
        let toast = self.push(kind, text);
        let _ = toast;
        self.raised.pop();
        self.items.last_mut().expect("just pushed")
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The toasts raised since the last call, for the activity log.
    pub fn take_raised(&mut self) -> Vec<(Kind, String)> {
        std::mem::take(&mut self.raised)
    }

    /// Draw them above the footer; returns the action clicked, if any.
    pub fn show(&mut self, ctx: &Context, footer_height: f32) -> Option<Action> {
        let now = Instant::now();
        self.items
            .retain(|toast| toast.remaining(now).is_none_or(|left| !left.is_zero()));
        // Only the newest few; older ones wait (and time out) unseen.
        let start = self.items.len().saturating_sub(MAX_SHOWN);
        if self.items.is_empty() {
            return None;
        }

        let pal = theme::palette(ctx);
        let screen = ctx.screen_rect();
        let width = (screen.width() - 32.0).min(MAX_WIDTH);
        let mut clicked = None;
        let mut dismissed = None;
        let mut y = screen.bottom() - footer_height - 12.0;

        for (index, toast) in self.items.iter().enumerate().skip(start).rev() {
            let area = Area::new(Id::new("toast").with(index).with(toast.shown_at))
                .order(Order::Foreground)
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(egui::pos2(screen.right() - 16.0, y))
                .interactable(true);
            let response = area.show(ctx, |ui| {
                Frame::new()
                    .fill(pal.snack_bg)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(14, 10))
                    .shadow(ctx.style().visuals.popup_shadow)
                    .show(ui, |ui| {
                        ui.set_max_width(width - 28.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            let icon = if toast.saved {
                                icons::download()
                            } else {
                                match toast.kind {
                                    Kind::Ok => icons::circle_check(),
                                    Kind::Info => icons::info(),
                                    Kind::Warn => icons::triangle_alert(),
                                    Kind::Error => icons::circle_x(),
                                }
                            };
                            ui.add(icons::image(icon, 16.0, pal.snack_ac));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&toast.text)
                                        .text_style(style::secondary())
                                        .color(pal.snack_tx),
                                )
                                .wrap(),
                            );
                            if let Some(action) = &toast.action {
                                let button = egui::Button::new(
                                    RichText::new(action.label())
                                        .text_style(style::secondary())
                                        .family(weight::semibold())
                                        .color(pal.snack_ac),
                                )
                                .frame(false);
                                if ui.add(button).clicked() {
                                    clicked = Some(action.clone());
                                    dismissed = Some(index);
                                }
                            }
                            if toast.kind == Kind::Error {
                                let close = egui::Button::image(icons::image(
                                    icons::x(),
                                    14.0,
                                    pal.snack_tx,
                                ))
                                .frame(false);
                                if ui.add(close).on_hover_text("Dismiss").clicked() {
                                    dismissed = Some(index);
                                }
                            }
                        });
                    });
            });
            y = response.response.rect.top() - 8.0;
        }

        if let Some(index) = dismissed {
            self.items.remove(index);
        }
        // One repaint when the next visible toast expires.
        if let Some(next) = self.items[self.items.len().saturating_sub(MAX_SHOWN)..]
            .iter()
            .filter_map(|toast| toast.remaining(now))
            .min()
        {
            ctx.request_repaint_after(next + Duration::from_millis(16));
        }
        let _ = vec2;
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_toast_is_also_raised_for_the_activity_log() {
        let mut toasts = Toasts::default();
        toasts.error("bad");
        toasts.info("fine");
        assert_eq!(
            toasts.take_raised(),
            vec![
                (Kind::Error, "bad".to_string()),
                (Kind::Info, "fine".to_string())
            ]
        );
        assert!(toasts.take_raised().is_empty());
    }

    #[test]
    fn warnings_stay_longer_than_errors_and_errors_longer_than_info() {
        assert!(Toast::default_duration(Kind::Warn) > Toast::default_duration(Kind::Error));
        assert!(Toast::default_duration(Kind::Error) > Toast::default_duration(Kind::Info));
    }
}
