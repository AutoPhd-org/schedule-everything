//! How `rmd web` opens the browser workspace in the default browser
//! (`[settings] open_workspace_in`): in a window of its own (the default), in
//! an app window without tabs or an address bar, or in a tab of a window that
//! is already open.
//!
//! A tab is what the system's "open this link" does. A window needs the
//! browser's help: Chromium browsers (Chrome, Edge, Brave, Vivaldi, Opera,
//! Chromium) take `--new-window` or `--app=<url>`, Firefox `-new-window`, and
//! Safari makes a new document over AppleScript. A browser this does not know,
//! or a command that fails, gets a tab instead.

use std::process::{Command, Stdio};

use serde_json::Value;

/// The `[settings]` key that chooses how the workspace opens.
pub const SETTING: &str = "open_workspace_in";
/// Its values, as written in `settings.toml`.
pub const CHOICES: [&str; 3] = ["window", "app", "tab"];

/// Where the workspace opens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OpenIn {
    /// A window of its own.
    #[default]
    Window,
    /// A window without tabs or an address bar (Chromium browsers; others get a window).
    App,
    /// A tab in a window that is already open.
    Tab,
}

impl OpenIn {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_lowercase().as_str() {
            "window" => Some(Self::Window),
            "app" => Some(Self::App),
            "tab" => Some(Self::Tab),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::App => "app",
            Self::Tab => "tab",
        }
    }

    /// The choice in the active `settings.toml`: a window when it is unset or
    /// not one of [`CHOICES`].
    pub fn configured() -> Self {
        crate::config::load_toml_file(&crate::paths::runtime().settings_path)
            .ok()
            .and_then(|config| config.get("settings")?.get(SETTING)?.as_str().and_then(Self::parse))
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Chromium,
    Firefox,
    Safari,
}

/// Chromium browsers by macOS bundle id and by Linux desktop file name. Their
/// beta, dev and canary builds share the name with a suffix.
const CHROMIUM: [&str; 13] = [
    "com.google.chrome",
    "com.microsoft.edgemac",
    "com.brave.browser",
    "org.chromium.chromium",
    "com.vivaldi.vivaldi",
    "com.operasoftware.opera",
    "google-chrome",
    "chromium",
    "chromium-browser",
    "microsoft-edge",
    "brave-browser",
    "vivaldi",
    "opera",
];

/// The family of a browser, by its macOS bundle id or Linux desktop file name.
fn family(browser: &str) -> Option<Family> {
    let id = browser.trim().to_lowercase();
    let id = id.strip_suffix(".desktop").unwrap_or(&id);
    let named = |base: &str| id == base || id.starts_with(&format!("{base}.")) || id.starts_with(&format!("{base}-"));
    if CHROMIUM.iter().any(|base| named(base)) {
        Some(Family::Chromium)
    } else if id.starts_with("org.mozilla.firefox") || named("firefox") {
        Some(Family::Firefox)
    } else if id == "com.apple.safari" || id == "com.apple.safaritechnologypreview" {
        Some(Family::Safari)
    } else {
        None
    }
}

const SAFARI_WINDOW: &str = "on run argv
tell application id \"com.apple.Safari\"
make new document with properties {URL:item 1 of argv}
activate
end tell
end run";

/// The command that opens `url` as `how` asks in `browser` (a macOS bundle
/// id); `None` for a tab.
fn macos_command(browser: &str, url: &str, how: OpenIn) -> Option<Vec<String>> {
    if how == OpenIn::Tab {
        return None;
    }
    // `open -n` hands the arguments to the browser even while it runs, and
    // the running browser opens the window.
    let open = |args: &[&str]| -> Vec<String> {
        ["open", "-n", "-b", browser, "--args"].iter().chain(args).map(|arg| arg.to_string()).collect()
    };
    match family(browser)? {
        Family::Chromium if how == OpenIn::App => Some(open(&[&format!("--app={url}")])),
        Family::Chromium => Some(open(&["--new-window", url])),
        Family::Firefox => Some(open(&["-new-window", url])),
        Family::Safari => Some(vec!["osascript".into(), "-e".into(), SAFARI_WINDOW.into(), url.into()]),
    }
}

/// The command that opens `url` as `how` asks in `browser` (a Linux desktop
/// file name, which names the browser's executable); `None` for a tab.
fn linux_command(browser: &str, url: &str, how: OpenIn) -> Option<Vec<String>> {
    if how == OpenIn::Tab {
        return None;
    }
    let program = browser.trim().strip_suffix(".desktop").unwrap_or(browser.trim()).to_string();
    match family(&program)? {
        Family::Chromium if how == OpenIn::App => Some(vec![program, format!("--app={url}")]),
        Family::Chromium | Family::Firefox => Some(vec![program, "--new-window".into(), url.into()]),
        Family::Safari => None,
    }
}

/// The default browser: its bundle id on macOS, its desktop file on Linux.
fn default_browser() -> Option<String> {
    let output = if cfg!(target_os = "macos") {
        Command::new("osascript")
            .args([
                "-l",
                "JavaScript",
                "-e",
                "ObjC.import('AppKit'); \
                 const app = $.NSWorkspace.sharedWorkspace.URLForApplicationToOpenURL($.NSURL.URLWithString('https://example.com')); \
                 app.isNil() ? '' : $.NSBundle.bundleWithURL(app).bundleIdentifier.js",
            ])
            .stderr(Stdio::null())
            .output()
            .ok()?
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-settings").args(["get", "default-web-browser"]).stderr(Stdio::null()).output().ok()?
    } else {
        return None;
    };
    let browser = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (output.status.success() && !browser.is_empty()).then_some(browser)
}

fn command_for(url: &str, how: OpenIn) -> Option<Vec<String>> {
    if how == OpenIn::Tab {
        return None;
    }
    let browser = default_browser()?;
    if cfg!(target_os = "macos") { macos_command(&browser, url, how) } else { linux_command(&browser, url, how) }
}

/// Run `command`; whether it worked. On macOS it returns as soon as the
/// browser has the link. On Linux the command may be the browser itself, so it
/// is started apart from this terminal and not waited for.
fn launch(command: &[String]) -> bool {
    let Some((program, args)) = command.split_first() else {
        return false;
    };
    let mut process = Command::new(program);
    process.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if cfg!(target_os = "macos") {
        return process.status().is_ok_and(|status| status.success());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid(2) is async-signal-safe and only detaches the child
        // from this terminal's session.
        unsafe {
            process.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    process.spawn().is_ok()
}

/// Open `url` in the default browser as `how` asks, or in a tab when that is
/// not possible.
pub fn open(url: &str, how: OpenIn) {
    if let Some(command) = command_for(url, how)
        && launch(&command)
    {
        return;
    }
    let _ = open::that_detached(url);
}

/// Bring the default browser forward, after a workspace tab that is already
/// open was asked to focus itself (on macOS; elsewhere the tab's own request
/// is all there is).
pub fn bring_forward() {
    if cfg!(target_os = "macos")
        && let Some(browser) = default_browser()
    {
        let _ = Command::new("open").args(["-b", &browser]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// The sign-in link in `lx open --print` output: its last link.
pub fn last_link(text: &str) -> Option<String> {
    text.split_whitespace()
        .rev()
        .find(|word| word.starts_with("http://") || word.starts_with("https://"))
        .map(str::to_string)
}

/// The `hasTab` answer of a workspace server's `POST /activate`.
pub fn has_tab(response: &Value) -> bool {
    response.get("hasTab").and_then(Value::as_bool) == Some(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    const URL: &str = "http://127.0.0.1:8765/#token=abc";

    #[test]
    fn the_setting_defaults_to_a_window() {
        let env = TestEnv::new();
        assert_eq!(OpenIn::configured(), OpenIn::Window);
        env.write("user_config_0/settings.toml", "[settings]\nopen_workspace_in = \"App\"\n");
        assert_eq!(OpenIn::configured(), OpenIn::App);
        env.write("user_config_0/settings.toml", "[settings]\nopen_workspace_in = \"tab\"\n");
        assert_eq!(OpenIn::configured(), OpenIn::Tab);
        env.write("user_config_0/settings.toml", "[settings]\nopen_workspace_in = \"popup\"\n");
        assert_eq!(OpenIn::configured(), OpenIn::Window);
        for choice in CHOICES {
            assert_eq!(OpenIn::parse(choice).unwrap().as_str(), choice);
        }
    }

    #[test]
    fn browsers_are_recognized_with_their_channels() {
        for id in ["com.microsoft.edgemac", "com.google.Chrome", "com.google.chrome.canary", "com.brave.Browser.beta"] {
            assert_eq!(family(id), Some(Family::Chromium), "{id}");
        }
        for id in ["google-chrome.desktop", "microsoft-edge-dev.desktop", "chromium-browser.desktop"] {
            assert_eq!(family(id), Some(Family::Chromium), "{id}");
        }
        assert_eq!(family("org.mozilla.firefoxdeveloperedition"), Some(Family::Firefox));
        assert_eq!(family("org.mozilla.firefox"), Some(Family::Firefox));
        assert_eq!(family("firefox.desktop"), Some(Family::Firefox));
        assert_eq!(family("com.apple.Safari"), Some(Family::Safari));
        assert_eq!(family("company.thebrowser.Browser"), None);
    }

    #[test]
    fn macos_commands_open_a_window_or_an_app_window() {
        let edge = "com.microsoft.edgemac";
        assert_eq!(
            macos_command(edge, URL, OpenIn::Window).unwrap(),
            ["open", "-n", "-b", edge, "--args", "--new-window", URL]
        );
        assert_eq!(
            macos_command(edge, URL, OpenIn::App).unwrap(),
            ["open", "-n", "-b", edge, "--args", &format!("--app={URL}")]
        );
        assert_eq!(macos_command(edge, URL, OpenIn::Tab), None);
        // Firefox has no app window, so it gets a window.
        assert_eq!(
            macos_command("org.mozilla.firefox", URL, OpenIn::App).unwrap(),
            ["open", "-n", "-b", "org.mozilla.firefox", "--args", "-new-window", URL]
        );
        let safari = macos_command("com.apple.Safari", URL, OpenIn::Window).unwrap();
        assert_eq!((safari[0].as_str(), safari[3].as_str()), ("osascript", URL));
        assert_eq!(macos_command("company.thebrowser.Browser", URL, OpenIn::Window), None);
    }

    #[test]
    fn linux_commands_run_the_browser_itself() {
        assert_eq!(
            linux_command("google-chrome.desktop", URL, OpenIn::Window).unwrap(),
            ["google-chrome", "--new-window", URL]
        );
        assert_eq!(
            linux_command("brave-browser.desktop", URL, OpenIn::App).unwrap(),
            ["brave-browser", &format!("--app={URL}")]
        );
        assert_eq!(linux_command("firefox.desktop", URL, OpenIn::App).unwrap(), ["firefox", "--new-window", URL]);
        assert_eq!(linux_command("epiphany.desktop", URL, OpenIn::Window), None);
        assert_eq!(linux_command("firefox.desktop", URL, OpenIn::Tab), None);
    }

    #[test]
    fn sign_in_links_and_activate_answers_are_read() {
        assert_eq!(
            last_link(
                "✓ rmd running\nhttp://rmd.localhost:7600/\nhttp://launchx.localhost:7600/.launchx/login?code=x\n"
            ),
            Some("http://launchx.localhost:7600/.launchx/login?code=x".into())
        );
        assert_eq!(last_link("lx: LaunchX is not running."), None);
        assert!(has_tab(&serde_json::json!({"hasTab": true})));
        assert!(!has_tab(&serde_json::json!({"hasTab": false})));
        assert!(!has_tab(&serde_json::json!({})));
    }
}
