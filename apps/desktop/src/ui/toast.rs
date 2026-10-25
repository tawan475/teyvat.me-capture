// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Toasts (spec §3 `Toast`, §8): inverse snackbars at the bottom right,
//! newest at the bottom, at most three, with an optional action.
//!
//! They slide in from the right and fade, count down with a thin bar, wait
//! while the pointer is over them, fade out, and the ones above glide into
//! place. That costs frames only while a toast is up: full rate for the
//! quarter second it moves, a slow tick for the bar, nothing once the last
//! one is gone.

use std::time::{Duration, Instant};

use egui::{Align2, Area, Context, CornerRadius, Frame, Id, Margin, Order, RichText, vec2};

use super::icons;
use super::theme::{self, style, weight};
use crate::activity::Kind;

const MAX_SHOWN: usize = 3;
const MAX_WIDTH: f32 = 400.0;
/// How long a toast takes to slide in, and to fade out.
const ENTER: Duration = Duration::from_millis(220);
const LEAVE: Duration = Duration::from_millis(180);
/// How far it slides in from.
const SLIDE: f32 = 48.0;
/// The countdown bar's frame interval while nothing moves.
const TICK: Duration = Duration::from_millis(40);

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
    id: u64,
    kind: Kind,
    text: String,
    action: Option<Action>,
    shown_at: Instant,
    /// Time spent under the pointer, which does not count down.
    held: Duration,
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
            Kind::Ok | Kind::Info => Duration::from_secs(5),
            Kind::Warn => Duration::from_secs(15),
            Kind::Error => Duration::from_secs(8),
        }
    }

    fn age(&self, now: Instant) -> Duration {
        (now - self.shown_at).saturating_sub(self.held)
    }

    fn remaining(&self, now: Instant) -> Option<Duration> {
        self.duration
            .map(|duration| duration.saturating_sub(self.age(now)))
    }

    /// 0 → 1 while it slides in, then 1, then 1 → 0 while it fades out.
    fn presence(&self, now: Instant) -> f32 {
        let entering = (self.age(now).as_secs_f32() / ENTER.as_secs_f32()).min(1.0);
        let leaving = self.remaining(now).map_or(1.0, |left| {
            (left.as_secs_f32() / LEAVE.as_secs_f32()).min(1.0)
        });
        ease_out(entering).min(leaving)
    }

    fn moving(&self, now: Instant) -> bool {
        self.age(now) < ENTER || self.remaining(now).is_some_and(|left| left < LEAVE)
    }
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
    next_id: u64,
    /// Every toast raised, for the activity log; the app drains it.
    raised: Vec<(Kind, String)>,
}

impl Toasts {
    fn push(&mut self, kind: Kind, text: impl Into<String>) -> &mut Toast {
        let text = text.into();
        self.raised.push((kind, text.clone()));
        self.next_id += 1;
        self.items.push(Toast {
            id: self.next_id,
            kind,
            text,
            action: None,
            shown_at: Instant::now(),
            held: Duration::ZERO,
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
        self.push(kind, text);
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

    /// Draw them at the window's bottom right, `inset` above its edge;
    /// returns the action clicked, if any.
    pub fn show(&mut self, ctx: &Context, inset: f32) -> Option<Action> {
        let now = Instant::now();
        self.items
            .retain(|toast| toast.remaining(now).is_none_or(|left| !left.is_zero()));
        if self.items.is_empty() {
            return None;
        }
        // Only the newest few; older ones wait (and time out) unseen.
        let start = self.items.len().saturating_sub(MAX_SHOWN);

        let pal = theme::palette(ctx);
        let screen = ctx.screen_rect();
        let width = (screen.width() - 32.0).min(MAX_WIDTH);
        let dt = Duration::from_secs_f32(ctx.input(|i| i.stable_dt).clamp(0.0, 0.1));
        let mut clicked = None;
        let mut dismissed = None;
        let mut moving = false;
        let mut bottom = screen.bottom() - 16.0 - inset;

        for (index, toast) in self.items.iter_mut().enumerate().skip(start).rev() {
            let id = Id::new("toast").with(toast.id);
            let presence = toast.presence(now);
            moving |= toast.moving(now);
            // Glide to its slot when one below it leaves.
            let y = ctx.animate_value_with_time(id.with("y"), bottom, 0.15);
            moving |= (y - bottom).abs() > 0.5;
            let x = screen.right() - 16.0 + (1.0 - presence) * SLIDE;
            let area = Area::new(id)
                .order(Order::Foreground)
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(egui::pos2(x, y))
                .interactable(true);
            let response = area.show(ctx, |ui| {
                ui.multiply_opacity(presence);
                let frame = Frame::new()
                    .fill(pal.snack_bg)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin {
                        left: 14,
                        right: 14,
                        top: 10,
                        bottom: 12,
                    })
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
                            ui.add(icons::image(icon, 16.0, accent(&pal, toast.kind)));
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
                            let close = egui::Button::image(icons::image(
                                icons::x(),
                                14.0,
                                pal.snack_tx.gamma_multiply(0.7),
                            ))
                            .frame(false);
                            if ui.add(close).on_hover_text("Dismiss").clicked() {
                                dismissed = Some(index);
                            }
                        });
                    });
                // The countdown along the foot.
                if let (Some(duration), Some(left)) = (toast.duration, toast.remaining(now)) {
                    let rect = frame.response.rect;
                    let share = left.as_secs_f32() / duration.as_secs_f32();
                    let bar = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 8.0, rect.bottom() - 4.0),
                        vec2((rect.width() - 16.0) * share, 2.0),
                    );
                    ui.painter().rect_filled(
                        bar,
                        CornerRadius::same(1),
                        accent(&pal, toast.kind).gamma_multiply(0.7),
                    );
                }
            });
            // Read, not counted down, while the pointer is on it.
            if response.response.contains_pointer() {
                toast.held += dt;
            }
            bottom -= response.response.rect.height() + 8.0;
        }

        if let Some(index) = dismissed {
            self.items.remove(index);
        }
        if moving {
            ctx.request_repaint();
        } else if self.items.iter().any(|toast| toast.duration.is_some()) {
            ctx.request_repaint_after(TICK);
        }
        clicked
    }
}

/// The icon and countdown colour of a toast's kind. The toast is the other
/// theme's surface, so its warning and error inks are the other theme's.
fn accent(pal: &theme::Palette, kind: Kind) -> egui::Color32 {
    let inverse = if pal.bg == theme::DARK.bg {
        theme::LIGHT
    } else {
        theme::DARK
    };
    match kind {
        Kind::Ok | Kind::Info => pal.snack_ac,
        Kind::Warn => inverse.warn_on,
        Kind::Error => inverse.bad,
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

    #[test]
    fn a_toast_slides_in_stays_then_fades_out() {
        let mut toasts = Toasts::default();
        let start = Instant::now();
        let toast = toasts.info("x");
        toast.shown_at = start;
        assert!(toast.presence(start) < 0.01);
        assert!(toast.moving(start));
        let middle = start + Duration::from_secs(2);
        assert_eq!(toast.presence(middle), 1.0);
        assert!(!toast.moving(middle));
        let end = start + Toast::default_duration(Kind::Info) - Duration::from_millis(90);
        assert!(toast.presence(end) < 0.6);
        assert!(toast.moving(end));
    }

    #[test]
    fn time_under_the_pointer_does_not_count_down() {
        let mut toasts = Toasts::default();
        let start = Instant::now();
        let toast = toasts.info("x");
        toast.shown_at = start;
        toast.held = Duration::from_secs(3);
        let later = start + Toast::default_duration(Kind::Info);
        assert_eq!(toast.remaining(later), Some(Duration::from_secs(3)));
    }
}
