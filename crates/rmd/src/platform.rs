//! Platform detection, sound playback, and native dialogs.
//!
//! macOS uses `afplay` and `osascript`; Linux uses `paplay`/`aplay`/`play`
//! and `zenity`/`kdialog`/`notify-send`. Every external tool is optional: when
//! one is missing the call degrades instead of failing.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// `"macos"`, `"linux"`, `"windows"`, or `"unknown"`.
pub fn get_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

/// Whether an executable is on `PATH` (`shutil.which`).
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(program)).find(|candidate| {
        candidate.is_file()
            && std::fs::metadata(candidate).is_ok_and(|m| {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    m.permissions().mode() & 0o111 != 0
                }
                #[cfg(not(unix))]
                {
                    let _ = m;
                    true
                }
            })
    })
}

/// Open a file or directory with the platform's default application.
pub fn open_file(path: &Path) -> bool {
    let openers: &[&str] = match get_platform() {
        "macos" => &["open"],
        "linux" => &["xdg-open"],
        _ => &["xdg-open", "open"],
    };
    for opener in openers {
        if which(opener).is_none() {
            continue;
        }
        if Command::new(opener)
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
        {
            return true;
        }
    }
    false
}

// =============================================================================
// Sound
// =============================================================================

/// Wait for a detached child on a background thread so it does not linger as
/// a zombie in the long-running reminder service.
fn reap_in_background(mut child: std::process::Child) {
    let _ = std::thread::Builder::new().spawn(move || child.wait());
}

/// Play a sound without waiting for it to finish.
pub fn play_sound(sound_file: &str) {
    match get_platform() {
        "macos" => {
            if let Ok(child) =
                Command::new("afplay").arg(sound_file).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
            {
                reap_in_background(child);
            }
        }
        "linux" => {
            for player in ["paplay", "aplay", "play"] {
                if let Ok(child) =
                    Command::new(player).arg(sound_file).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
                {
                    reap_in_background(child);
                    return;
                }
            }
            println!("Warning: Could not play sound {sound_file}");
        }
        other => println!("Sound playback not supported on {other}"),
    }
}

// =============================================================================
// Dialogs
// =============================================================================

/// Escape text for an AppleScript string literal.
pub fn escape_applescript_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', " ")
}

/// Run a command with a timeout, returning (status success, stdout).
pub fn run_with_timeout(command: &mut Command, timeout: Option<Duration>) -> std::io::Result<Option<(bool, String)>> {
    let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            let mut stdout = String::new();
            if let Some(mut out) = child.stdout.take() {
                use std::io::Read;
                let _ = out.read_to_string(&mut stdout);
            }
            return Ok(Some((status.success(), stdout)));
        }
        if let Some(limit) = timeout
            && start.elapsed() >= limit
        {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Large-font modal dialog (the Python version used tkinter: Helvetica 16pt
/// message, 13pt buttons, centred, always on top, Return = default button,
/// Escape/close = `None`, optional auto-click of the default after `timeout`).
///
/// Returns the clicked label, or `None` when closed without a choice or when
/// no large-font backend is available (callers then fall back to AppleScript).
pub fn show_large_dialog(
    message: &str,
    buttons: &[&str],
    default_button: &str,
    title: &str,
    timeout: Option<u64>,
) -> Option<String> {
    crate::dialogs::show_large_dialog(message, buttons, default_button, title, timeout)
}

fn show_dialog_macos(message: &str) -> String {
    let button = "停止闹铃";
    if let Some(clicked) = show_large_dialog(message, &[button], button, "Reminder", None) {
        return clicked;
    }
    let escaped = escape_applescript_string(message);
    let script = format!("display dialog \"{escaped}\" buttons {{\"{button}\"}} default button \"{button}\"");
    match Command::new("osascript").arg("-e").arg(script).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if stdout.is_empty() { button.to_string() } else { stdout }
        }
        Err(_) => button.to_string(),
    }
}

fn show_dialog_linux(message: &str) -> String {
    let attempts: [Vec<String>; 3] = [
        vec!["zenity".into(), "--info".into(), format!("--text={message}")],
        vec!["kdialog".into(), "--msgbox".into(), message.to_string()],
        vec!["notify-send".into(), "--urgency=critical".into(), "Reminder".into(), message.to_string()],
    ];
    for argv in attempts {
        let mut command = Command::new(&argv[0]);
        command.args(&argv[1..]);
        match run_with_timeout(&mut command, Some(Duration::from_secs(30))) {
            Ok(Some((true, _))) => return "OK".to_string(),
            Ok(Some((false, _))) => return "Cancel".to_string(),
            Ok(None) | Err(_) => continue,
        }
    }
    println!("Warning: Could not show dialog: {message}");
    "OK".to_string()
}

/// Show a dialog/notification; returns the button clicked (or `"OK"`).
pub fn show_dialog(message: &str) -> String {
    match get_platform() {
        "macos" => show_dialog_macos(message),
        "linux" => show_dialog_linux(message),
        other => {
            println!("Dialog display not supported on {other}");
            "OK".to_string()
        }
    }
}

/// Multi-select list. `Some(selected)` (possibly empty), or `None` when
/// cancelled or no GUI backend is available.
pub fn choose_multiple(options: &[String], title: &str, prompt: &str) -> Option<Vec<String>> {
    match get_platform() {
        "macos" => {
            let items =
                options.iter().map(|o| format!("\"{}\"", escape_applescript_string(o))).collect::<Vec<_>>().join(",");
            let script = format!(
                "set optionsList to {{{items}}}\nset promptText to \"{}\"\nset titleText to \"{}\"\nset choice to choose from list optionsList with title titleText with prompt promptText with multiple selections allowed\nif choice is false then\n  return \"__CANCEL__\"\nend if\nset AppleScript's text item delimiters to \"\\n\"\nreturn choice as text",
                escape_applescript_string(prompt),
                escape_applescript_string(title)
            );
            let mut command = Command::new("osascript");
            command.arg("-e").arg(script);
            match run_with_timeout(&mut command, Some(Duration::from_secs(120))) {
                Err(_) => {
                    println!("Warning: osascript not found; cannot show habit prompt window.");
                    None
                }
                Ok(None) => {
                    println!("Warning: habit prompt window timed out.");
                    None
                }
                Ok(Some((false, _))) => None,
                Ok(Some((true, stdout))) => parse_selection(&stdout, true),
            }
        }
        "linux" => {
            let mut command = Command::new("zenity");
            command.args(["--list", "--checklist", "--title", title, "--text", prompt]);
            command.args(["--column", "Done", "--column", "Habit"]);
            for option in options {
                command.arg("FALSE").arg(option);
            }
            command.args(["--separator", "\n"]);
            match run_with_timeout(&mut command, Some(Duration::from_secs(120))) {
                Ok(Some((true, stdout))) => parse_selection(&stdout, false),
                _ => None,
            }
        }
        _ => None,
    }
}

fn parse_selection(stdout: &str, cancel_marker: bool) -> Option<Vec<String>> {
    let stdout = stdout.trim();
    if cancel_marker && stdout == "__CANCEL__" {
        return None;
    }
    if stdout.is_empty() {
        return Some(Vec::new());
    }
    Some(stdout.lines().filter(|l| !l.trim().is_empty()).map(str::to_string).collect())
}

/// Yes/No/Stop question: `Some(true)`, `Some(false)`, or `None` for stop/cancel.
pub fn ask_yes_no(question: &str, title: &str) -> Option<bool> {
    if get_platform() == "macos" {
        return match show_large_dialog(question, &["Stop", "No", "Yes"], "Yes", title, None).as_deref() {
            Some("Yes") => Some(true),
            Some("No") => Some(false),
            _ => None,
        };
    }
    println!("\n{title}: {question}");
    loop {
        use std::io::Write;
        print!(" (y/n/s[top]): ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return None;
        }
        let choice = line.trim().to_lowercase();
        if choice.starts_with('y') {
            return Some(true);
        }
        if choice.starts_with('n') {
            return Some(false);
        }
        if choice.starts_with('s') {
            return None;
        }
    }
}
