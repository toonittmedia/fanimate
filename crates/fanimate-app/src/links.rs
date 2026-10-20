//! Opening web links in the user's browser.
//!
//! eframe's own `open_url` needs its `links` feature, which pulls in a URL
//! parser and the Unicode tables for it just to start a browser, so Fanimate
//! asks the system directly, the way `render_queue` opens folders.

use eframe::egui;

/// Open `url` in the default browser. Only web addresses are opened: a link
/// is never a way to start a program or a file.
pub fn open(url: &str) {
    if !is_web_address(url) {
        log::warn!("not opening {url}: only http and https links are opened");
        return;
    }
    if let Err(e) = command(url).spawn() {
        log::warn!("could not open {url}: {e}");
    }
}

/// A link that opens in the browser, with its address as the tooltip.
pub fn link(ui: &mut egui::Ui, text: &str, url: &str) -> egui::Response {
    let r = ui.link(text).on_hover_text(url);
    if r.clicked() {
        open(url);
    }
    r
}

fn is_web_address(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://")) && !url.chars().any(char::is_whitespace)
}

#[cfg(windows)]
fn command(url: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    // `rundll32 url.dll,FileProtocolHandler` hands the address to the shell
    // without a command line to quote, so an `&` in it stays part of it
    let mut cmd = std::process::Command::new("rundll32");
    cmd.arg("url.dll,FileProtocolHandler").arg(url);
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    cmd
}

#[cfg(target_os = "macos")]
fn command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("open");
    cmd.arg(url);
    cmd
}

#[cfg(not(any(windows, target_os = "macos")))]
fn command(url: &str) -> std::process::Command {
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(url);
    cmd
}

#[cfg(test)]
mod tests {
    use super::is_web_address;

    #[test]
    fn only_web_addresses_open() {
        assert!(is_web_address("https://github.com/toonittmedia/fanimate"));
        assert!(is_web_address("http://example.org/a?b=1&c=2"));
        assert!(!is_web_address("file:///C:/Windows/System32/calc.exe"));
        assert!(!is_web_address("calc.exe"));
        assert!(!is_web_address("https://example.org/ && calc"));
        assert!(!is_web_address(""));
    }
}
