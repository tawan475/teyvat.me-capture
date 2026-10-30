// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Times of day as the user reads them: "14:45" or "2:45 PM".
//!
//! Settings › App › Clock picks 12- or 24-hour, or follows the system's own
//! short time format (the default). Every time of day the app shows is
//! formatted with [`hm`], or [`hms`] where seconds matter; file names and
//! logs keep their fixed 24-hour stamps.

use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// The clock setting (Settings › App).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClockChoice {
    #[default]
    System,
    #[serde(rename = "12h")]
    TwelveHour,
    #[serde(rename = "24h")]
    TwentyFourHour,
}

static TWELVE_HOUR: AtomicBool = AtomicBool::new(false);

/// Use `choice` for every time shown from now on.
pub fn set(choice: ClockChoice) {
    let twelve = match choice {
        ClockChoice::System => system_is_twelve_hour(),
        ClockChoice::TwelveHour => true,
        ClockChoice::TwentyFourHour => false,
    };
    TWELVE_HOUR.store(twelve, Ordering::Relaxed);
}

/// Whether the system shows times in 12 hours. Read again on each call, so
/// a change in Windows' settings is picked up when the setting is applied.
pub fn system_is_twelve_hour() -> bool {
    imp::short_time_format().is_some_and(|format| uses_twelve_hours(&format))
}

/// A Windows short time pattern ("h:mm tt", "HH:mm", "tt h:mm") counts its
/// hours in 12 when it has a lowercase `h` outside quoted literals.
fn uses_twelve_hours(format: &str) -> bool {
    let mut quoted = false;
    for c in format.chars() {
        match c {
            '\'' => quoted = !quoted,
            'h' if !quoted => return true,
            _ => {}
        }
    }
    false
}

/// The chrono pattern of an hour and minute: "14:45" or "2:45 PM".
pub fn hm() -> &'static str {
    pattern(TWELVE_HOUR.load(Ordering::Relaxed), false)
}

/// [`hm`] with seconds: "14:45:09" or "2:45:09 PM".
pub fn hms() -> &'static str {
    pattern(TWELVE_HOUR.load(Ordering::Relaxed), true)
}

fn pattern(twelve: bool, seconds: bool) -> &'static str {
    match (twelve, seconds) {
        (true, false) => "%-I:%M %p",
        (true, true) => "%-I:%M:%S %p",
        (false, false) => "%H:%M",
        (false, true) => "%H:%M:%S",
    }
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::w;

    /// The user's short time pattern from Settings › Time & language
    /// (`HKCU\Control Panel\International` `sShortTime`), as "h:mm tt".
    pub fn short_time_format() -> Option<String> {
        let mut data = [0u16; 80];
        let mut size = (data.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Control Panel\\International"),
                w!("sShortTime"),
                RRF_RT_REG_SZ,
                None,
                Some(data.as_mut_ptr().cast()),
                Some(&mut size),
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).min(data.len());
        let text = String::from_utf16_lossy(&data[..len]);
        Some(text.trim_end_matches('\0').to_owned())
    }
}

#[cfg(not(windows))]
mod imp {
    /// No system pattern is read off Windows: 24 hours unless chosen.
    pub fn short_time_format() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveTime;

    use super::*;

    #[test]
    fn windows_patterns_tell_twelve_from_twenty_four_hours() {
        for twelve in ["h:mm tt", "hh:mm tt", "tt h:mm", "tt hh:mm", "H'h' h:mm"] {
            assert!(uses_twelve_hours(twelve), "{twelve}");
        }
        for twenty_four in ["HH:mm", "H:mm", "HH'h'mm", "H.mm", ""] {
            assert!(!uses_twelve_hours(twenty_four), "{twenty_four}");
        }
    }

    #[test]
    fn times_follow_the_choice() {
        let at = NaiveTime::from_hms_opt(14, 45, 9).unwrap();
        let shown = |twelve, seconds| at.format(pattern(twelve, seconds)).to_string();
        assert_eq!(shown(true, false), "2:45 PM");
        assert_eq!(shown(true, true), "2:45:09 PM");
        assert_eq!(shown(false, false), "14:45");
        assert_eq!(shown(false, true), "14:45:09");
        let morning = NaiveTime::from_hms_opt(9, 5, 0).unwrap();
        assert_eq!(morning.format(pattern(true, false)).to_string(), "9:05 AM");
    }
}
