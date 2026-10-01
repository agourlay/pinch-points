//! Hand an address to the desktop's browser. The one thing here that
//! leaves the process, so it is kept apart from whoever asks for it, and
//! tests only note the ask.

#[cfg(not(test))]
use bevy::log::{info, warn};

/// Hand a URL to the desktop's opener. Only ever given a release page's
/// GitHub `https://` address, checked at parse time.
///
/// `Ok` means the opener started, not that a browser has the page: on a
/// desktop with no handler `xdg-open` starts fine and fails a moment later,
/// and it can sit for as long as the browser it launched runs. So it is not
/// waited on: a thread reaps it and logs how it went, and the notice says
/// "handed to".
pub(crate) fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(test)]
    {
        tests::stand_in(url)
    }
    #[cfg(not(test))]
    open_url_for_real(url)
}

/// The opener itself, kept apart so tests can stand in for it.
#[cfg(not(test))]
fn open_url_for_real(url: &str) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "windows") {
        // `start` is a cmd built-in; the empty string is the window title
        // it would otherwise take the URL for.
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || match child.wait() {
        Ok(status) if status.success() => info!("update check: the release page was handed over"),
        Ok(status) => warn!("update check: the opener gave up: {status}"),
        Err(e) => warn!("update check: could not wait on the opener: {e}"),
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    /// Every address the page asked a browser for, in order.
    pub(crate) static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    /// An address the stand-in fails to open, as a desktop with no opener
    /// would: by the address rather than a switch, since tests run side by
    /// side and a switch thrown by one would fail another's browser.
    pub(crate) const UNOPENABLE: &str = "https://github.com/x/y/releases/tag/unopenable";

    /// What tests get instead of a browser: the ask is noted, and nobody's
    /// desktop is touched.
    pub(super) fn stand_in(url: &str) -> std::io::Result<()> {
        OPENED.lock().unwrap().push(url.to_string());
        match url == UNOPENABLE {
            true => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no opener on this desktop",
            )),
            false => Ok(()),
        }
    }
}
