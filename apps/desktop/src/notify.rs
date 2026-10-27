// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

//! Windows notifications (Action Center), for the resin alert.
//!
//! An unpackaged app shows toasts under an AppUserModelID registered in the
//! user's registry (`HKCU\Software\Classes\AppUserModelId\<id>`, with the name
//! and icon Windows shows), so no Start menu shortcut is needed. Elsewhere
//! there is nothing to show them with yet: `show` returns an error.

use anyhow::Result;

/// Show one notification: a title line and a text line.
pub fn show(title: &str, text: &str) -> Result<()> {
    imp::show(title, text)
}

/// `&`, `<`, `>`, quotes: the toast is XML.
#[cfg_attr(not(windows), allow(dead_code))]
fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(windows)]
mod imp {
    use anyhow::{Context, Result};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::{HSTRING, PCWSTR};

    /// The AppUserModelID notifications go out under; the isolated copy's own.
    fn app_id() -> &'static str {
        if crate::isolated() {
            "Teyvat.me.Capture.Isolated"
        } else {
            "Teyvat.me.Capture"
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn set_string(key: HKEY, name: &str, value: &str) -> Result<()> {
        let value: Vec<u8> = wide(value).iter().flat_map(|c| c.to_le_bytes()).collect();
        let name = wide(name);
        unsafe { RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(&value)) }
            .ok()
            .context("cannot write the notification registration")
    }

    /// Name and icon under the app id, so Windows shows "Teyvat.me" with the
    /// app's icon. The icon is the app's own PNG, written to its data folder.
    fn register() -> Result<()> {
        let icon = crate::data_dir()?.join("notification-icon.png");
        if !icon.exists() {
            if let Some(dir) = icon.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&icon, include_bytes!("../assets/icon-256.png"))?;
        }
        let path = wide(&format!(r"Software\Classes\AppUserModelId\{}", app_id()));
        let mut key = HKEY::default();
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
        }
        .ok()
        .context("cannot register for notifications")?;
        let written = set_string(key, "DisplayName", "Teyvat.me")
            .and_then(|()| set_string(key, "IconUri", &icon.display().to_string()));
        unsafe {
            let _ = RegCloseKey(key);
        }
        written
    }

    pub fn show(title: &str, text: &str) -> Result<()> {
        // A thread of its own may not have WinRT yet; "already" is fine.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
        register()?;
        let xml = format!(
            "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
            super::xml_escape(title),
            super::xml_escape(text)
        );
        let doc = XmlDocument::new()?;
        doc.LoadXml(&HSTRING::from(xml))?;
        let toast = ToastNotification::CreateToastNotification(&doc)?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id()))?
            .Show(&toast)
            .context("Windows refused the notification")
    }
}

#[cfg(not(windows))]
mod imp {
    use anyhow::{Result, bail};

    pub fn show(_title: &str, _text: &str) -> Result<()> {
        bail!("notifications are only shown on Windows so far")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shows a real notification: run by hand (`--ignored`) on Windows.
    #[test]
    #[ignore]
    fn windows_takes_the_notification() {
        show("Resin 160/200", "This is how the resin alert looks").unwrap();
    }

    #[test]
    fn toast_text_is_escaped() {
        assert_eq!(
            xml_escape(r#"A & B <"c"> 'd'"#),
            "A &amp; B &lt;&quot;c&quot;&gt; &apos;d&apos;"
        );
    }
}
