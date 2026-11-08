//! Service and config-set commands: update, stop, switch, report, edit, mode
//! (port of `commands/service.py`).
//!
//! The reminder service is `rmd daemon`, run by the installer's LaunchAgent
//! (macOS) or systemd user unit (Linux). Restarting it stops the unit, ends
//! any stray daemon (including a legacy `python reminder_macos.py` one), and
//! starts the unit again. Every external program goes through [`Host`], so
//! tests never touch the real service, git, or an editor.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::NaiveDate;

use crate::i18n::{t, tf};
use crate::{data, paths, py};

/// Label of the installer's LaunchAgent (macOS).
pub const LAUNCH_AGENT_LABEL: &str = "com.sergiudm.schedule.management.reminder";
/// Name of the installer's systemd user unit (Linux).
pub const SYSTEMD_SERVICE_NAME: &str = "schedule-management.service";
/// Details from [`restart_reminder_service`] when no installer-managed service exists.
pub const NO_RESTART_HELPER: &str = "No installer restart script found.";

/// `pgrep -f` patterns for the reminder service: `rmd daemon`, and the Python
/// script that older installs ran.
const DAEMON_PATTERNS: [&str; 2] = ["(^|/)rmd daemon( |$)", "python.*reminder_macos\\.py"];

/// Pause between stopping and starting the service (as the installer's restart script did).
const RESTART_PAUSE: Duration = Duration::from_secs(2);

// =============================================================================
// HOST: external programs and OS calls
// =============================================================================

/// Exit code and output of a finished command (`subprocess.run(capture_output=True, text=True)`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Captured {
    /// Python's `returncode`: the exit code, or minus the signal number.
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl Captured {
    fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// The programs and OS calls the service commands use; tests substitute a fake.
trait Host {
    fn platform(&self) -> &'static str;
    fn home(&self) -> PathBuf;
    fn current_pid(&self) -> i64;
    /// Run a command to completion, capturing its output.
    fn output(&self, command: &mut Command) -> io::Result<Captured>;
    /// Run a command attached to this terminal (the editor) and wait for it.
    fn run_attached(&self, command: &mut Command) -> io::Result<()>;
    fn which(&self, program: &str) -> bool;
    fn terminate(&self, pid: i64) -> io::Result<()>;
    fn sleep(&self, duration: Duration);
    fn open_file(&self, path: &Path) -> bool;
    fn generate_report(&self, report_type: &str, target_date: Option<NaiveDate>) -> anyhow::Result<Option<PathBuf>>;
    fn restart_service(&self) -> (bool, String);
}

/// The real machine.
struct System;

impl Host for System {
    fn platform(&self) -> &'static str {
        crate::platform::get_platform()
    }

    fn home(&self) -> PathBuf {
        paths::home_dir()
    }

    fn current_pid(&self) -> i64 {
        i64::from(std::process::id())
    }

    fn output(&self, command: &mut Command) -> io::Result<Captured> {
        let output = command.output()?;
        Ok(Captured {
            code: returncode(&output.status),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn run_attached(&self, command: &mut Command) -> io::Result<()> {
        command.status().map(|_| ())
    }

    fn which(&self, program: &str) -> bool {
        crate::platform::which(program).is_some()
    }

    fn terminate(&self, pid: i64) -> io::Result<()> {
        send_sigterm(pid)
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn open_file(&self, path: &Path) -> bool {
        crate::platform::open_file(path)
    }

    fn generate_report(&self, report_type: &str, target_date: Option<NaiveDate>) -> anyhow::Result<Option<PathBuf>> {
        crate::report::generate_manual_report(report_type, target_date, None)
    }

    fn restart_service(&self) -> (bool, String) {
        restart_with(self)
    }
}

/// Python's `returncode`: the exit code, or minus the signal that ended the process.
fn returncode(status: &std::process::ExitStatus) -> Option<i32> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.code().or_else(|| status.signal().map(|signal| -signal))
    }
    #[cfg(not(unix))]
    {
        status.code()
    }
}

/// Send SIGTERM to a process (`os.kill(pid, signal.SIGTERM)`).
pub(crate) fn send_sigterm(pid: i64) -> io::Result<()> {
    #[cfg(unix)]
    {
        // Never signal a process group or every process (pid 0 / -1).
        let pid = libc::pid_t::try_from(pid)
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| io::Error::from_raw_os_error(libc::EINVAL))?;
        // SAFETY: kill(2) takes plain integers and has no memory-safety requirements.
        if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Err(io::Error::new(io::ErrorKind::Unsupported, "signals are not supported on this platform"))
    }
}

/// Python's `str(OSError)`: `[Errno 2] No such file or directory: 'git'`.
pub(crate) fn os_error_text(error: &io::Error, filename: Option<&str>) -> String {
    let Some(code) = error.raw_os_error() else {
        return error.to_string();
    };
    let text = error.to_string();
    let description = text.strip_suffix(&format!(" (os error {code})")).unwrap_or(&text);
    match filename {
        Some(name) => format!("[Errno {code}] {description}: {}", py::repr_str(name)),
        None => format!("[Errno {code}] {description}"),
    }
}

/// The program and arguments of a command, for messages and tests.
fn argv(command: &Command) -> Vec<String> {
    std::iter::once(command.get_program()).chain(command.get_args()).map(|s| s.to_string_lossy().into_owned()).collect()
}

/// Print one line (Python `print`), ignoring a closed stdout.
fn say(out: &mut dyn Write, line: impl AsRef<str>) {
    let _ = writeln!(out, "{}", line.as_ref());
}

// =============================================================================
// SERVICE RESTART
// =============================================================================

/// The LaunchAgent plist the installer writes (macOS).
pub fn launch_agent_plist(home: &Path) -> PathBuf {
    home.join("Library").join("LaunchAgents").join(format!("{LAUNCH_AGENT_LABEL}.plist"))
}

/// The systemd user unit the installer writes (Linux).
pub fn systemd_unit_path(home: &Path) -> PathBuf {
    home.join(".config").join("systemd").join("user").join(SYSTEMD_SERVICE_NAME)
}

fn launchctl(action: &str, plist: &Path) -> Command {
    let mut command = Command::new("launchctl");
    command.arg(action).arg(plist);
    command
}

fn systemctl(action: &str) -> Command {
    let mut command = Command::new("systemctl");
    command.args(["--user", action, SYSTEMD_SERVICE_NAME]);
    command
}

/// PIDs (as `pgrep` prints them) of running reminder services, without duplicates.
fn daemon_pids(host: &dyn Host) -> io::Result<Vec<String>> {
    let mut pids: Vec<String> = Vec::new();
    for pattern in DAEMON_PATTERNS {
        let result = host.output(Command::new("pgrep").args(["-f", pattern]))?;
        if !result.success() {
            continue;
        }
        for line in result.stdout.trim().split('\n') {
            if !line.trim().is_empty() && !pids.iter().any(|known| known.trim() == line.trim()) {
                pids.push(line.to_string());
            }
        }
    }
    Ok(pids)
}

/// Restart the installer-managed service: stop the unit, end stray daemons,
/// wait, and start the unit again.
fn restart_with(host: &dyn Host) -> (bool, String) {
    let home = host.home();
    let (mut stop, mut start) = match host.platform() {
        "macos" => {
            let plist = launch_agent_plist(&home);
            if !plist.is_file() {
                return (false, NO_RESTART_HELPER.to_string());
            }
            (launchctl("unload", &plist), launchctl("load", &plist))
        }
        "linux" => {
            if !systemd_unit_path(&home).is_file() {
                return (false, NO_RESTART_HELPER.to_string());
            }
            (systemctl("stop"), systemctl("start"))
        }
        _ => return (false, NO_RESTART_HELPER.to_string()),
    };
    // Stopping fails harmlessly when the service is not loaded.
    let _ = host.output(&mut stop);
    let current = host.current_pid();
    for pid in daemon_pids(host).unwrap_or_default() {
        if let Some(pid) = py::parse_int_str(&pid).filter(|pid| *pid != current) {
            let _ = host.terminate(pid);
        }
    }
    host.sleep(RESTART_PAUSE);
    let start_argv = argv(&start);
    match host.output(&mut start) {
        Err(error) => (false, os_error_text(&error, Some(&start_argv[0]))),
        Ok(result) if result.success() => (true, String::new()),
        Ok(result) => {
            let mut details = result.stderr.trim().to_string();
            if details.is_empty() {
                details = result.stdout.trim().to_string();
            }
            if details.is_empty() {
                let code = result.code.map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                details = format!("`{}` exited with status {code}.", start_argv.join(" "));
            }
            (false, details)
        }
    }
}

/// Restart the background reminder service. Returns `(ok, message)`;
/// `message` is [`NO_RESTART_HELPER`] when no installer-managed service exists.
pub fn restart_reminder_service() -> (bool, String) {
    restart_with(&System)
}

/// Restart the service and report the outcome; `false` when it failed.
fn restart_and_report(host: &dyn Host, out: &mut dyn Write) -> bool {
    let (restarted, details) = host.restart_service();
    if restarted {
        say(out, t("✅ Reminder service restarted"));
    } else if details == NO_RESTART_HELPER {
        say(out, t("ℹ️  No installer restart script found."));
        say(out, t("   Restart the reminder service manually if it is already running."));
    } else {
        say(out, t("❌ Reminder service restart failed:"));
        say(out, details);
        return false;
    }
    true
}

// =============================================================================
// UPDATE / SWITCH / STOP
// =============================================================================

/// Whether the config directory is backed by a local git checkout.
fn has_git_metadata(config_dir: &Path) -> bool {
    config_dir.join(".git").exists()
}

fn update_with(host: &dyn Host, out: &mut dyn Write) -> i32 {
    say(out, t("📥 Updating schedule files..."));
    let config_dir = paths::resolve_config_root_dir();
    if !config_dir.exists() {
        say(out, tf("❌ Config directory not found: {config_dir}", &[("config_dir", &config_dir.display())]));
        return 1;
    }
    if has_git_metadata(&config_dir) {
        let mut pull = Command::new("git");
        pull.arg("-C").arg(&config_dir).args(["pull", "--rebase"]);
        match host.output(&mut pull) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                say(out, t("❌ Git not found. Please install git to use update."));
                return 1;
            }
            Err(error) => {
                say(out, tf("❌ Error updating: {e}", &[("e", &os_error_text(&error, Some("git")))]));
                return 1;
            }
            Ok(result) if result.success() => {
                say(out, t("✅ Successfully pulled latest changes"));
                if result.stdout.contains("Already up to date") {
                    say(out, t("   Already up to date."));
                } else {
                    say(out, result.stdout.trim());
                }
            }
            Ok(result) => {
                say(out, t("❌ Git pull failed:"));
                say(out, result.stderr.trim());
                return 1;
            }
        }
    } else {
        say(
            out,
            tf("ℹ️  Config directory is not a git repository: {config_dir}", &[("config_dir", &config_dir.display())]),
        );
        say(out, t("   Skipping git pull and using local schedule files as-is."));
    }
    if !restart_and_report(host, out) {
        return 1;
    }
    say(out, t("✅ Update finished"));
    0
}

/// `rmd update`: pull the config repo when git-managed, then reload the service.
pub fn update_command() -> i32 {
    update_with(&System, &mut io::stdout())
}

fn stop_with(host: &dyn Host, out: &mut dyn Write) -> i32 {
    say(out, t("🛑 Stopping reminder service..."));
    let pids = match daemon_pids(host) {
        Ok(pids) => pids,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            say(out, t("❌ 'pgrep' command not found."));
            say(out, t("   Try finding the process manually with 'ps aux | grep reminder-runner'"));
            return 1;
        }
        Err(error) => {
            say(out, tf("❌ Error stopping service: {e}", &[("e", &os_error_text(&error, Some("pgrep")))]));
            return 1;
        }
    };
    if pids.is_empty() {
        say(out, t("⚠️  No running reminder-runner process found."));
        return 0;
    }
    let current = host.current_pid();
    let mut stopped = 0;
    for pid_text in &pids {
        let Some(pid) = py::parse_int_str(pid_text) else {
            let error = format!("invalid literal for int() with base 10: {}", py::repr_str(pid_text.trim()));
            say(out, tf("⚠️  Could not stop PID {pid}: {e}", &[("pid", pid_text), ("e", &error)]));
            continue;
        };
        // Don't stop ourselves if somehow matched.
        if pid == current {
            continue;
        }
        match host.terminate(pid) {
            Ok(()) => {
                say(out, tf("✅ Stopped reminder-runner (PID: {pid})", &[("pid", &pid)]));
                stopped += 1;
            }
            Err(error) => {
                let error = os_error_text(&error, None);
                say(out, tf("⚠️  Could not stop PID {pid}: {e}", &[("pid", pid_text), ("e", &error)]));
            }
        }
    }
    if stopped == 0 {
        say(out, t("⚠️  No reminder-runner processes were stopped."));
    } else {
        say(out, tf("   Total processes stopped: {count}", &[("count", &stopped)]));
    }
    0
}

/// `rmd stop`: stop the reminder service.
pub fn stop_command() -> i32 {
    stop_with(&System, &mut io::stdout())
}

fn switch_with(host: &dyn Host, out: &mut dyn Write, config_id: &str) -> i32 {
    let root = paths::resolve_config_root_dir();
    let available = paths::list_config_ids(Some(&root));
    if available.is_empty() {
        say(out, tf("❌ No config sets found under: {config_root_dir}", &[("config_root_dir", &root.display())]));
        say(out, t("   Create or migrate a schedule first so user_config_0 exists."));
        return 1;
    }
    let ids = available.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
    let raw = config_id.trim();
    let requested = match py::parse_int_str(raw) {
        Some(id) if available.contains(&id) => id,
        parsed => {
            let shown = match parsed {
                Some(id) => id.to_string(),
                None if raw.is_empty() => "(empty)".to_string(),
                None => raw.to_string(),
            };
            say(out, tf("❌ Invalid config id: {config_id}", &[("config_id", &shown)]));
            say(out, tf("   Valid config ids: {ids}", &[("ids", &ids)]));
            return 1;
        }
    };
    if let Err(error) = paths::write_active_config_id(Some(&root), requested) {
        say(out, tf("❌ Unexpected error: {e}", &[("e", &os_error_text(&error, None))]));
        return 1;
    }
    let active_dir = paths::preview_active_config_dir(Some(&root));
    say(out, tf("✅ Switched to user_config_{requested_id}", &[("requested_id", &requested)]));
    say(out, tf("   Active config directory: {directory}", &[("directory", &active_dir.display())]));
    if restart_and_report(host, out) { 0 } else { 1 }
}

/// `rmd switch <config_id>`: activate another `user_config_n` and reload.
pub fn switch_command(config_id: &str) -> i32 {
    switch_with(&System, &mut io::stdout(), config_id)
}

// =============================================================================
// REPORT
// =============================================================================

fn report_with(host: &dyn Host, out: &mut dyn Write, report_type: &str, date: Option<&str>, days: Option<i64>) -> i32 {
    say(out, t("📊 Generating report..."));
    let target_date = match date.filter(|raw| !raw.is_empty()) {
        None => None,
        Some(raw) => match py::strptime_date(raw, "%Y-%m-%d") {
            Some(parsed) => Some(parsed),
            None => {
                say(out, tf("❌ Invalid date format: {date}", &[("date", &raw)]));
                say(out, t("   Use YYYY-MM-DD format (e.g., 2024-01-15)"));
                return 1;
            }
        },
    };
    if report_type != "weekly" && report_type != "monthly" {
        say(out, tf("❌ Unsupported report type: {type}", &[("type", &report_type)]));
        return 1;
    }
    if report_type == "weekly" {
        if days.is_some_and(|days| days != 7) {
            say(out, t("❌ Custom day ranges are not supported for weekly reports."));
            say(out, t("   Use '--days 7' or omit the flag."));
            return 1;
        }
    } else if days.is_some() {
        say(out, t("❌ '--days' is not supported for monthly reports."));
        return 1;
    }
    say(out, tf("   Report type: {type}", &[("type", &report_type)]));
    if let Some(target) = target_date {
        say(out, tf("   Target date: {date}", &[("date", &target.format("%Y-%m-%d"))]));
    }
    match host.generate_report(report_type, target_date) {
        Ok(Some(path)) => {
            say(out, format!("\n{}", tf("✅ Report generated: {path}", &[("path", &path.display())])));
            // Without an opener the printed path is all the user needs.
            let _ = host.open_file(&path);
            0
        }
        Ok(None) => {
            say(out, t("⚠️  Report generation completed but no file was created."));
            1
        }
        Err(error) => {
            say(out, tf("❌ Error generating report: {e}", &[("e", &error)]));
            1
        }
    }
}

/// `rmd report <weekly|monthly> [-d DATE] [--days N]`.
pub fn report_command(report_type: &str, date: Option<&str>, days: Option<i64>) -> i32 {
    report_with(&System, &mut io::stdout(), report_type, date, days)
}

// =============================================================================
// EDIT
// =============================================================================

fn edit_with(host: &dyn Host, out: &mut dyn Write, file: &str) -> i32 {
    let runtime = paths::runtime();
    let file_map: [(&str, &Path); 6] = [
        ("settings", &runtime.settings_path),
        ("odd", &runtime.odd_path),
        ("even", &runtime.even_path),
        ("deadlines", &runtime.ddl_path),
        ("ddl", &runtime.ddl_path),
        ("habits", &runtime.habit_path),
    ];
    let target = file.to_lowercase();
    let Some((_, file_path)) = file_map.iter().find(|(name, _)| *name == target) else {
        let files = file_map.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", ");
        say(out, tf("❌ Unknown file: {file}", &[("file", &target)]));
        say(out, tf("   Available: {files}", &[("files", &files)]));
        return 1;
    };
    if !file_path.exists() {
        say(out, tf("⚠️  File does not exist: {path}", &[("path", &file_path.display())]));
        say(out, t("   Creating empty file..."));
        let created = file_path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| OpenOptions::new().create(true).append(true).open(file_path).map(|_| ()));
        if let Err(error) = created {
            say(out, tf("❌ Unexpected error: {e}", &[("e", &os_error_text(&error, None))]));
            return 1;
        }
    }

    // $EDITOR wins even when empty; then $VISUAL; then a common editor on PATH.
    let mut editor = std::env::var_os("EDITOR")
        .or_else(|| std::env::var_os("VISUAL"))
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    if editor.is_empty() {
        editor = ["code", "vim", "nano", "vi"]
            .into_iter()
            .find(|candidate| host.which(candidate))
            .unwrap_or_default()
            .to_string();
    }
    if editor.is_empty() {
        say(out, t("❌ No editor found. Set $EDITOR environment variable."));
        say(out, tf("   File path: {path}", &[("path", &file_path.display())]));
        return 1;
    }
    say(out, tf("📝 Opening {file} in {editor}...", &[("file", &target), ("editor", &editor)]));
    let _ = out.flush();
    let mut command = Command::new(&editor);
    command.arg(file_path);
    match host.run_attached(&mut command) {
        Ok(()) => 0,
        Err(error) => {
            say(out, tf("❌ Could not open editor: {e}", &[("e", &os_error_text(&error, Some(&editor)))]));
            1
        }
    }
}

/// `rmd edit [settings|odd|even|deadlines|ddl|habits]`.
pub fn edit_schedule_command(file: &str) -> i32 {
    edit_with(&System, &mut io::stdout(), file)
}

// =============================================================================
// MODE
// =============================================================================

fn mode_with(host: &dyn Host, out: &mut dyn Write, mode: Option<&str>) -> i32 {
    let Some(requested) = mode else {
        say(out, tf("Current mode is {mode} mode", &[("mode", &data::load_mode())]));
        return 0;
    };
    let requested = requested.to_lowercase().trim().to_string();
    if requested != "j" && requested != "p" {
        say(out, tf("❌ Invalid mode: {mode}. Only 'j' or 'p' is supported.", &[("mode", &requested)]));
        return 1;
    }
    if data::load_mode() == requested {
        say(out, tf("Already in {mode} mode", &[("mode", &requested)]));
        return 0;
    }
    if let Err(error) = data::save_mode(&requested) {
        say(out, tf("❌ Failed to switch mode: {e}", &[("e", &error)]));
        return 1;
    }
    say(out, tf("✅ Mode switched to {mode} mode successfully!", &[("mode", &requested)]));
    // Reload the service so it picks up the new mode immediately.
    if restart_and_report(host, out) { 0 } else { 1 }
}

/// `rmd mode [j|p]`.
pub fn mode_command(mode: Option<&str>) -> i32 {
    mode_with(&System, &mut io::stdout(), mode)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::test_support::TestEnv;

    type Responder = Box<dyn Fn(&[String]) -> io::Result<Captured>>;

    /// Records every call; never runs a program.
    struct FakeHost {
        platform: &'static str,
        home: PathBuf,
        respond: Responder,
        restart: Option<(bool, String)>,
        report: RefCell<Option<anyhow::Result<Option<PathBuf>>>>,
        on_path: Vec<&'static str>,
        commands: RefCell<Vec<Vec<String>>>,
        attached: RefCell<Vec<Vec<String>>>,
        terminated: RefCell<Vec<i64>>,
        slept: RefCell<Vec<Duration>>,
        opened: RefCell<Vec<PathBuf>>,
        reports: RefCell<Vec<(String, Option<NaiveDate>)>>,
        restarts: Cell<usize>,
    }

    impl FakeHost {
        fn new() -> Self {
            Self {
                platform: "macos",
                home: PathBuf::from("/nonexistent-home"),
                respond: Box::new(|_| Ok(Captured { code: Some(1), ..Captured::default() })),
                restart: Some((true, String::new())),
                report: RefCell::new(None),
                on_path: Vec::new(),
                commands: RefCell::new(Vec::new()),
                attached: RefCell::new(Vec::new()),
                terminated: RefCell::new(Vec::new()),
                slept: RefCell::new(Vec::new()),
                opened: RefCell::new(Vec::new()),
                reports: RefCell::new(Vec::new()),
                restarts: Cell::new(0),
            }
        }

        fn responding(mut self, respond: impl Fn(&[String]) -> io::Result<Captured> + 'static) -> Self {
            self.respond = Box::new(respond);
            self
        }

        fn restart_result(mut self, ok: bool, details: &str) -> Self {
            self.restart = Some((ok, details.to_string()));
            self
        }

        fn commands(&self) -> Vec<Vec<String>> {
            self.commands.borrow().clone()
        }
    }

    impl Host for FakeHost {
        fn platform(&self) -> &'static str {
            self.platform
        }

        fn home(&self) -> PathBuf {
            self.home.clone()
        }

        fn current_pid(&self) -> i64 {
            999
        }

        fn output(&self, command: &mut Command) -> io::Result<Captured> {
            let argv = argv(command);
            self.commands.borrow_mut().push(argv.clone());
            (self.respond)(&argv)
        }

        fn run_attached(&self, command: &mut Command) -> io::Result<()> {
            self.attached.borrow_mut().push(argv(command));
            Ok(())
        }

        fn which(&self, program: &str) -> bool {
            self.on_path.contains(&program)
        }

        fn terminate(&self, pid: i64) -> io::Result<()> {
            self.terminated.borrow_mut().push(pid);
            if pid == 404 { Err(io::Error::from_raw_os_error(3)) } else { Ok(()) }
        }

        fn sleep(&self, duration: Duration) {
            self.slept.borrow_mut().push(duration);
        }

        fn open_file(&self, path: &Path) -> bool {
            self.opened.borrow_mut().push(path.to_path_buf());
            true
        }

        fn generate_report(
            &self,
            report_type: &str,
            target_date: Option<NaiveDate>,
        ) -> anyhow::Result<Option<PathBuf>> {
            self.reports.borrow_mut().push((report_type.to_string(), target_date));
            self.report.borrow_mut().take().unwrap_or(Ok(None))
        }

        fn restart_service(&self) -> (bool, String) {
            self.restarts.set(self.restarts.get() + 1);
            match &self.restart {
                Some(result) => result.clone(),
                None => restart_with(self),
            }
        }
    }

    fn ok(stdout: &str) -> io::Result<Captured> {
        Ok(Captured { code: Some(0), stdout: stdout.to_string(), stderr: String::new() })
    }

    fn run(f: impl FnOnce(&mut Vec<u8>) -> i32) -> (i32, String) {
        let mut out = Vec::new();
        let code = f(&mut out);
        (code, String::from_utf8(out).unwrap())
    }

    // ---- update (TestUpdateCommand) ------------------------------------------

    #[test]
    fn update_pulls_git_config_and_restarts() {
        let env = TestEnv::new();
        std::fs::create_dir(env.root().join(".git")).unwrap();
        let host = FakeHost::new().responding(|_| ok("Already up to date."));
        let (code, output) = run(|out| update_with(&host, out));

        assert_eq!(code, 0);
        let root = paths::resolve_config_root_dir().display().to_string();
        assert_eq!(host.commands(), vec![vec!["git".to_string(), "-C".into(), root, "pull".into(), "--rebase".into()]]);
        assert_eq!(host.restarts.get(), 1);
        assert!(output.contains("✅ Successfully pulled latest changes\n   Already up to date.\n"));
        assert!(output.ends_with("✅ Reminder service restarted\n✅ Update finished\n"));
    }

    #[test]
    fn update_stops_when_git_pull_fails() {
        let env = TestEnv::new();
        std::fs::create_dir(env.root().join(".git")).unwrap();
        let host = FakeHost::new().responding(|_| {
            Ok(Captured { code: Some(1), stdout: String::new(), stderr: "error: failed to pull\n".into() })
        });
        let (code, output) = run(|out| update_with(&host, out));

        assert_eq!(code, 1);
        assert_eq!(host.restarts.get(), 0);
        assert!(output.contains("❌ Git pull failed:\nerror: failed to pull\n"));
    }

    #[test]
    fn update_reports_missing_git() {
        let env = TestEnv::new();
        std::fs::create_dir(env.root().join(".git")).unwrap();
        let host = FakeHost::new().responding(|_| Err(io::Error::from(io::ErrorKind::NotFound)));
        let (code, output) = run(|out| update_with(&host, out));
        assert_eq!(code, 1);
        assert!(output.contains("❌ Git not found. Please install git to use update."));
    }

    #[test]
    fn update_fails_without_config_dir() {
        let mut env = TestEnv::new();
        let missing = env.root().join("missing");
        env.set_var("REMINDER_CONFIG_DIR", missing.to_str().unwrap());
        let host = FakeHost::new();
        let (code, output) = run(|out| update_with(&host, out));
        assert_eq!(code, 1);
        assert!(output.contains("❌ Config directory not found:"));
        assert_eq!(host.restarts.get(), 0);
    }

    #[test]
    fn update_skips_pull_for_local_config() {
        let _env = TestEnv::new();
        let host = FakeHost::new().restart_result(false, NO_RESTART_HELPER);
        let (code, output) = run(|out| update_with(&host, out));

        assert_eq!(code, 0);
        assert!(host.commands().is_empty());
        assert_eq!(host.restarts.get(), 1);
        assert!(output.contains("   Skipping git pull and using local schedule files as-is."));
        assert!(output.contains(
            "ℹ️  No installer restart script found.\n   Restart the reminder service manually if it is already running.\n"
        ));
        assert!(output.ends_with("✅ Update finished\n"));
    }

    #[test]
    fn update_fails_when_restart_fails() {
        let _env = TestEnv::new();
        let host = FakeHost::new().restart_result(false, "launchctl failed");
        let (code, output) = run(|out| update_with(&host, out));
        assert_eq!(code, 1);
        assert!(output.ends_with("❌ Reminder service restart failed:\nlaunchctl failed\n"));
    }

    // ---- switch (TestSwitchCommand) ------------------------------------------

    #[test]
    fn switch_updates_active_config_and_restarts_service() {
        let env = TestEnv::new();
        std::fs::create_dir_all(env.root().join("user_config_0")).unwrap();
        std::fs::create_dir_all(env.root().join("user_config_1")).unwrap();
        let host = FakeHost::new();
        let (code, output) = run(|out| switch_with(&host, out, "1"));

        assert_eq!(code, 0);
        assert_eq!(env.read(".active_config").trim(), "1");
        assert_eq!(host.restarts.get(), 1);
        assert!(output.starts_with("✅ Switched to user_config_1\n   Active config directory: "));
    }

    #[test]
    fn switch_rejects_invalid_config_id() {
        let env = TestEnv::new();
        std::fs::create_dir_all(env.root().join("user_config_0")).unwrap();
        std::fs::create_dir_all(env.root().join("user_config_2")).unwrap();
        let host = FakeHost::new();
        let (code, output) = run(|out| switch_with(&host, out, "5"));
        assert_eq!(code, 1);
        assert_eq!(host.restarts.get(), 0);
        assert!(output.contains("Invalid config id: 5"));
        assert!(output.contains("Valid config ids: 0, 2"));

        let (code, output) = run(|out| switch_with(&host, out, "  "));
        assert_eq!(code, 1);
        assert!(output.contains("❌ Invalid config id: (empty)"));
        let (_, output) = run(|out| switch_with(&host, out, "two"));
        assert!(output.contains("❌ Invalid config id: two"));
    }

    #[test]
    fn switch_needs_a_config_set() {
        let _env = TestEnv::new();
        let host = FakeHost::new();
        let (code, output) = run(|out| switch_with(&host, out, "0"));
        assert_eq!(code, 1);
        assert!(output.contains("   Create or migrate a schedule first so user_config_0 exists."));
    }

    // ---- report (TestReportCommand) ------------------------------------------

    fn report_host(path: &str) -> FakeHost {
        let host = FakeHost::new();
        *host.report.borrow_mut() = Some(Ok(Some(PathBuf::from(path))));
        host
    }

    #[test]
    fn report_weekly_and_monthly_success() {
        let _env = TestEnv::new();
        let host = report_host("/tmp/weekly_report.pdf");
        let (code, output) = run(|out| report_with(&host, out, "weekly", Some("2024-02-01"), Some(7)));
        assert_eq!(code, 0);
        assert_eq!(*host.reports.borrow(), vec![("weekly".to_string(), NaiveDate::from_ymd_opt(2024, 2, 1))]);
        assert!(output.contains(
            "   Report type: weekly\n   Target date: 2024-02-01\n\n✅ Report generated: /tmp/weekly_report.pdf\n"
        ));

        let host = report_host("/tmp/monthly_report.pdf");
        let (code, _) = run(|out| report_with(&host, out, "monthly", Some("2024-02-01"), None));
        assert_eq!(code, 0);
        assert_eq!(*host.reports.borrow(), vec![("monthly".to_string(), NaiveDate::from_ymd_opt(2024, 2, 1))]);
    }

    #[test]
    fn report_rejects_bad_arguments() {
        let _env = TestEnv::new();
        let host = FakeHost::new();
        let (code, output) = run(|out| report_with(&host, out, "weekly", Some("2024/02/01"), None));
        assert_eq!(code, 1);
        assert!(output.contains("Invalid date format"));

        let (code, output) = run(|out| report_with(&host, out, "weekly", None, Some(14)));
        assert_eq!(code, 1);
        assert!(output.contains("Custom day ranges are not supported for weekly reports."));

        let (code, output) = run(|out| report_with(&host, out, "monthly", None, Some(7)));
        assert_eq!(code, 1);
        assert!(output.contains("'--days' is not supported for monthly reports."));

        let (code, output) = run(|out| report_with(&host, out, "daily", None, None));
        assert_eq!(code, 1);
        assert!(output.contains("❌ Unsupported report type: daily"));
        assert!(host.reports.borrow().is_empty());
    }

    #[test]
    fn report_opens_the_generated_file() {
        let _env = TestEnv::new();
        let host = report_host("/tmp/weekly_report.pdf");
        let (code, _) = run(|out| report_with(&host, out, "weekly", None, Some(7)));
        assert_eq!(code, 0);
        assert_eq!(*host.reports.borrow(), vec![("weekly".to_string(), None)]);
        assert_eq!(*host.opened.borrow(), vec![PathBuf::from("/tmp/weekly_report.pdf")]);
    }

    #[test]
    fn report_without_a_file_or_with_an_error_fails() {
        let _env = TestEnv::new();
        let host = FakeHost::new();
        let (code, output) = run(|out| report_with(&host, out, "weekly", None, None));
        assert_eq!(code, 1);
        assert!(output.contains("⚠️  Report generation completed but no file was created."));

        *host.report.borrow_mut() = Some(Err(anyhow::anyhow!("disk full")));
        let (code, output) = run(|out| report_with(&host, out, "weekly", None, None));
        assert_eq!(code, 1);
        assert!(output.contains("❌ Error generating report: disk full"));
    }

    // ---- mode (TestModeCLI) --------------------------------------------------

    #[test]
    fn mode_command_display_and_switch() {
        let _env = TestEnv::new();
        let host = FakeHost::new();

        let (code, output) = run(|out| mode_with(&host, out, None));
        assert_eq!(code, 0);
        assert_eq!(output, "Current mode is j mode\n");

        let (code, _) = run(|out| mode_with(&host, out, Some("p")));
        assert_eq!(code, 0);
        assert_eq!(data::load_mode(), "p");
        assert_eq!(host.restarts.get(), 1);

        let (code, _) = run(|out| mode_with(&host, out, Some("j")));
        assert_eq!(code, 0);
        assert_eq!(data::load_mode(), "j");
        assert_eq!(host.restarts.get(), 2);

        let (code, output) = run(|out| mode_with(&host, out, Some("J")));
        assert_eq!((code, output.as_str()), (0, "Already in j mode\n"));
        let (code, output) = run(|out| mode_with(&host, out, Some("x")));
        assert_eq!(code, 1);
        assert_eq!(output, "❌ Invalid mode: x. Only 'j' or 'p' is supported.\n");
        assert_eq!(host.restarts.get(), 2);
    }

    // ---- stop ---------------------------------------------------------------

    #[test]
    fn stop_terminates_daemon_and_legacy_processes() {
        let _env = TestEnv::new();
        let host = FakeHost::new().responding(|argv| match argv[2].as_str() {
            "(^|/)rmd daemon( |$)" => ok("101\n999\n"),
            _ => ok("202\n101\n"),
        });
        let (code, output) = run(|out| stop_with(&host, out));

        assert_eq!(code, 0);
        assert_eq!(
            host.commands(),
            DAEMON_PATTERNS.iter().map(|p| vec!["pgrep".to_string(), "-f".into(), p.to_string()]).collect::<Vec<_>>()
        );
        // Our own pid (999) is skipped and duplicates are stopped once.
        assert_eq!(*host.terminated.borrow(), vec![101, 202]);
        assert_eq!(
            output,
            "🛑 Stopping reminder service...\n✅ Stopped reminder-runner (PID: 101)\n\
             ✅ Stopped reminder-runner (PID: 202)\n   Total processes stopped: 2\n"
        );
    }

    #[test]
    fn stop_reports_nothing_to_stop_and_failures() {
        let _env = TestEnv::new();
        let host = FakeHost::new();
        let (code, output) = run(|out| stop_with(&host, out));
        assert_eq!(code, 0);
        assert!(output.ends_with("⚠️  No running reminder-runner process found.\n"));

        let host = FakeHost::new().responding(|_| ok("404\n"));
        let (code, output) = run(|out| stop_with(&host, out));
        assert_eq!(code, 0);
        assert!(output.contains("⚠️  Could not stop PID 404: [Errno 3] No such process\n"));
        assert!(output.ends_with("⚠️  No reminder-runner processes were stopped.\n"));

        let host = FakeHost::new().responding(|_| Err(io::Error::from(io::ErrorKind::NotFound)));
        let (code, output) = run(|out| stop_with(&host, out));
        assert_eq!(code, 1);
        assert!(output.contains("❌ 'pgrep' command not found."));
    }

    // ---- restart ------------------------------------------------------------

    #[test]
    fn restart_reloads_the_launch_agent_and_ends_stray_daemons() {
        let env = TestEnv::new();
        let plist = launch_agent_plist(env.root());
        std::fs::create_dir_all(plist.parent().unwrap()).unwrap();
        std::fs::write(&plist, "<plist/>").unwrap();
        let mut host = FakeHost::new().responding(|argv| match argv[0].as_str() {
            "pgrep" if argv[2].contains("rmd daemon") => ok("4242\n"),
            "launchctl" => ok(""),
            _ => Ok(Captured { code: Some(1), ..Captured::default() }),
        });
        host.home = env.root().to_path_buf();
        host.restart = None;

        assert_eq!(host.restart_service(), (true, String::new()));
        let plist = plist.display().to_string();
        let commands = host.commands();
        assert_eq!(commands.first().unwrap(), &vec!["launchctl".to_string(), "unload".into(), plist.clone()]);
        assert_eq!(commands.last().unwrap(), &vec!["launchctl".to_string(), "load".into(), plist]);
        assert_eq!(commands.len(), 4);
        assert_eq!(*host.terminated.borrow(), vec![4242]);
        assert_eq!(*host.slept.borrow(), vec![RESTART_PAUSE]);
    }

    #[test]
    fn restart_uses_the_systemd_unit_on_linux_and_reports_failures() {
        let env = TestEnv::new();
        let unit = systemd_unit_path(env.root());
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "[Unit]\n").unwrap();
        let mut host = FakeHost::new().responding(|argv| match argv.get(2).map(String::as_str) {
            Some("start") => Ok(Captured { code: Some(5), stdout: String::new(), stderr: "unit failed\n".into() }),
            _ => ok(""),
        });
        host.platform = "linux";
        host.home = env.root().to_path_buf();

        assert_eq!(restart_with(&host), (false, "unit failed".to_string()));
        let commands = host.commands();
        assert_eq!(commands[0], ["systemctl", "--user", "stop", SYSTEMD_SERVICE_NAME]);
        assert_eq!(commands.last().unwrap(), &["systemctl", "--user", "start", SYSTEMD_SERVICE_NAME]);

        host.respond = Box::new(|argv| match argv.get(2).map(String::as_str) {
            Some("start") => Ok(Captured { code: Some(3), ..Captured::default() }),
            _ => ok(""),
        });
        assert_eq!(
            restart_with(&host).1,
            format!("`systemctl --user start {SYSTEMD_SERVICE_NAME}` exited with status 3.")
        );
    }

    #[test]
    fn restart_without_an_installed_service_does_nothing() {
        let env = TestEnv::new();
        let mut host = FakeHost::new();
        host.home = env.root().to_path_buf();
        for platform in ["macos", "linux", "windows"] {
            host.platform = platform;
            assert_eq!(restart_with(&host), (false, NO_RESTART_HELPER.to_string()));
        }
        assert!(host.commands().is_empty());
        assert!(host.slept.borrow().is_empty());
    }

    #[test]
    fn daemon_patterns_match_rmd_daemon_and_legacy_script_only() {
        let daemon = regex::Regex::new(DAEMON_PATTERNS[0]).unwrap();
        let legacy = regex::Regex::new(DAEMON_PATTERNS[1]).unwrap();
        for line in ["rmd daemon", "/Users/me/SCHEDULE_MANAGEMENT/rmd daemon", "/opt/bin/rmd daemon --visualize"] {
            assert!(daemon.is_match(line), "{line}");
        }
        for line in ["rmd stop", "rmd __web-server --root /x --port 0", "sh -c rmd daemon", "vim /tmp/rmd daemon.txt"] {
            assert!(!daemon.is_match(line), "{line}");
        }
        assert!(legacy.is_match("/Users/me/.venv/bin/python /Users/me/src/schedule_management/reminder_macos.py"));
        assert!(!legacy.is_match("vim reminder_macos.py"));
    }

    // ---- edit ---------------------------------------------------------------

    #[test]
    fn edit_opens_existing_or_new_file_in_editor() {
        let mut env = TestEnv::new();
        env.config_dir();
        env.set_var("EDITOR", "my-editor");
        env.remove_var("VISUAL");
        let host = FakeHost::new();
        let (code, output) = run(|out| edit_with(&host, out, "Habits"));

        assert_eq!(code, 0);
        let habits = paths::runtime().habit_path;
        assert!(habits.exists());
        assert!(output.contains("⚠️  File does not exist: "));
        assert!(output.ends_with("📝 Opening habits in my-editor...\n"));
        assert_eq!(*host.attached.borrow(), vec![vec!["my-editor".to_string(), habits.display().to_string()]]);
    }

    #[test]
    fn edit_falls_back_to_an_editor_on_path_or_fails() {
        let mut env = TestEnv::new();
        env.config_dir();
        env.set_var("EDITOR", "");
        env.set_var("VISUAL", "ignored-because-editor-is-set");
        let mut host = FakeHost::new();
        host.on_path = vec!["nano", "vi"];
        let (code, _) = run(|out| edit_with(&host, out, "ddl"));
        assert_eq!(code, 0);
        assert_eq!(host.attached.borrow()[0][0], "nano");

        host.on_path.clear();
        let (code, output) = run(|out| edit_with(&host, out, "odd"));
        assert_eq!(code, 1);
        assert!(output.contains("❌ No editor found. Set $EDITOR environment variable."));

        let (code, output) = run(|out| edit_with(&host, out, "profile"));
        assert_eq!(code, 1);
        assert!(output.ends_with("   Available: settings, odd, even, deadlines, ddl, habits\n"));
    }

    #[test]
    fn os_errors_read_like_python() {
        let error = io::Error::from_raw_os_error(2);
        assert_eq!(os_error_text(&error, Some("code --wait")), "[Errno 2] No such file or directory: 'code --wait'");
        assert_eq!(os_error_text(&io::Error::other("boom"), None), "boom");
    }
}
