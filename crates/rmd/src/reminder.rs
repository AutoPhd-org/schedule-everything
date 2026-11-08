//! Background reminder service entry point (port of `reminder_macos.py` and
//! the `runner.py` main). OWNER: daemon agent.

/// `rmd daemon [ARGS...]`: run the reminder service until stopped (the
/// LaunchAgent / systemd unit runs this). `args` are the arguments after
/// `daemon` (for example `--visualize`). Returns the exit code.
pub fn run_service(_args: Vec<String>) -> i32 {
    todo!("daemon agent: port reminder_macos.py / runner.py main")
}
