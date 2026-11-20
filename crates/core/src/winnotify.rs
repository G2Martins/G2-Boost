//! Native Windows notification (round 11): one toast in the notification center when a job ends
//! while G2 Boost sits in the tray or minimized.
//!
//! WinRT through the `windows` crate already in use (no plugin, no C code). An unpackaged exe only
//! gets toasts for an app id that a Start menu shortcut carries: the NSIS installer stamps
//! `com.grovicdata.g2boost` (the bundle identifier) on its shortcut. Without that shortcut
//! (portable copy, a dev build on a clean PC) Windows drops the toast without an error, so
//! [`available`] checks for it first and the caller flashes the taskbar button instead.
//!
//! The text comes from the webview (it renders the i18n line): hostile input, cleaned here.

use std::path::PathBuf;
use std::sync::Mutex;

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::{HSTRING, IInspectable};

/// The bundle identifier: the AppUserModelID the installer writes on the Start menu shortcut.
pub const APP_ID: &str = "com.grovicdata.g2boost";
pub const MAX_TITLE: usize = 80;
pub const MAX_BODY: usize = 200;

/// The last toast stays referenced so its `Activated` (a click while the app runs) still fires.
static LAST: Mutex<Option<ToastNotification>> = Mutex::new(None);

/// Control and bidirectional-override characters dropped, whitespace collapsed, cut to `max`.
pub fn clean(s: &str, max: usize) -> String {
    let bidi = |c: char| matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    let kept: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).filter(|&c| !bidi(c)).collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// The toast document: two lines of plain text, nothing else (no images, actions or launch URI).
pub fn xml(title: &str, body: &str) -> String {
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual></toast>",
        escape(&clean(title, MAX_TITLE)),
        escape(&clean(body, MAX_BODY))
    )
}

/// The installer's Start menu shortcut (per machine, or per user): what makes Windows accept the
/// app id. ponytail: checks the file, not the AUMID inside it; read the property if a third-party
/// shortcut with the same name ever shows up.
pub fn available() -> bool {
    let link = |base: Option<std::ffi::OsString>| {
        base.map(PathBuf::from).is_some_and(|b| b.join(r"Microsoft\Windows\Start Menu\Programs\G2 Boost.lnk").is_file())
    };
    link(std::env::var_os("ProgramData")) || link(std::env::var_os("APPDATA"))
}

/// Show one toast; `on_click` runs when the person clicks it while G2 Boost is still running.
pub fn show(title: &str, body: &str, on_click: impl Fn() + Send + 'static) -> Result<(), String> {
    let err = |e: windows::core::Error| format!("0x{:08X}", e.code().0);
    let doc = XmlDocument::new().map_err(err)?;
    doc.LoadXml(&HSTRING::from(xml(title, body))).map_err(err)?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(err)?;
    toast
        .Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
            on_click();
            Ok(())
        }))
        .map_err(err)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID)).and_then(|n| n.Show(&toast)).map_err(err)?;
    *LAST.lock().unwrap_or_else(|p| p.into_inner()) = Some(toast);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_text_is_cleaned_and_escaped() {
        // markup cannot open a new element or attribute; controls and bidi overrides vanish
        let x = xml("<text>Hi</text><image src='x'/>", "a\u{202E}b\r\nc\u{7}&d");
        assert!(!x.contains("<image"), "{x}");
        assert!(x.contains("&lt;text&gt;Hi&lt;/text&gt;&lt;image src=&apos;x&apos;/&gt;"), "{x}");
        assert!(x.contains("<text>ab c &amp;d</text>"), "{x}");
        assert_eq!(x.matches("<text>").count(), 2);
        // capped after cleaning
        assert_eq!(clean(&"x".repeat(500), MAX_TITLE).chars().count(), MAX_TITLE);
        assert_eq!(clean("  \u{200F} ", MAX_BODY), "");
    }

    /// Shows a real toast on this PC (needs the installed copy's Start menu shortcut).
    #[test]
    #[ignore]
    fn shows_a_real_toast() {
        assert!(available(), "no G2 Boost shortcut in the Start menu");
        show("G2 Boost", "Teste de notificação (cargo test)", || {}).unwrap();
    }
}
