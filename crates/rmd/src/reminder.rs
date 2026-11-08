//! Background reminder service entry point (port of `reminder_macos.py` and
//! the `runner.py` main). OWNER: daemon agent.
//!
//! The LaunchAgent (macOS) / systemd user unit (Linux) runs `rmd daemon`,
//! which loads the active config set once and checks the clock every 20
//! seconds until the process is stopped. Runtime events are logged to stdout
//! as `[YYYY-MM-DD HH:MM:SS] message` lines (the service redirects them to
//! `logs/schedule_management.out`). SIGTERM/SIGINT stop the service
//! immediately, like the Python runner; alarms and prompts run on detached
//! threads and never delay the loop.

use std::path::PathBuf;

use crate::config::{ScheduleConfig, WeeklySchedule};
use crate::i18n::{t, tf};
use crate::{data, paths, platform, runner, visualizer};

const USAGE: &str = "\
usage: rmd daemon [--visualize]

Run the background reminder service (what the LaunchAgent / systemd unit starts).

options:
  --visualize, --view  generate the schedule visualization PDF and exit
  -h, --help           show this help message and exit
";

/// What `rmd daemon [ARGS...]` was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Run the reminder loop (the default).
    Run,
    /// Generate the schedule visualization PDF and exit.
    Visualize,
    /// Print usage and exit.
    Help,
}

/// Unknown arguments are ignored, as the Python service entry point ignored
/// its command line.
fn parse_args(args: &[String]) -> Mode {
    let mut mode = Mode::Run;
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Mode::Help,
            "--visualize" | "--view" => mode = Mode::Visualize,
            _ => {}
        }
    }
    mode
}

/// `rmd daemon [ARGS...]`: run the reminder service until stopped (the
/// LaunchAgent / systemd unit runs this). `args` are the arguments after
/// `daemon` (for example `--visualize`). Returns the exit code.
pub fn run_service(args: Vec<String>) -> i32 {
    match parse_args(&args) {
        Mode::Help => {
            print!("{USAGE}");
            0
        }
        Mode::Visualize => visualize_schedule(),
        Mode::Run => runner::main(),
    }
}

/// `--visualize`: the schedule PDF, like `rmd view` (the installer's
/// `visualize_schedule.sh` passes this flag).
fn visualize_schedule() -> i32 {
    if data::load_mode() == "p" {
        println!("{}", t("❌ Currently in p mode. Switch back to j mode to execute this command."));
        return 1;
    }

    println!("{}", t("📊 Generating schedule visualizations..."));
    let runtime_paths = paths::runtime();
    let generated = (|| -> anyhow::Result<PathBuf> {
        let config = ScheduleConfig::load(&runtime_paths.settings_path)?;
        let weekly = WeeklySchedule::load(&runtime_paths.odd_path, &runtime_paths.even_path)?;
        visualizer::visualize(&config, &weekly.odd_data, &weekly.even_data, None)
    })();
    match generated {
        Ok(pdf_path) => {
            println!("\n{}", t("📁 Visualization file generated:"));
            println!("\n{}", t("🖼️  Opening visualization..."));
            if !platform::open_file(&pdf_path) {
                println!(
                    "{}",
                    tf(
                        "⚠️  Could not open file automatically. File saved to: {path}",
                        &[("path", &pdf_path.display())]
                    )
                );
            }
            0
        }
        Err(error) => {
            println!("{}", tf("❌ Error generating visualizations: {e}", &[("e", &error)]));
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn plain_daemon_runs_the_reminder_loop() {
        assert_eq!(parse_args(&args(&[])), Mode::Run);
        assert_eq!(parse_args(&args(&["--unknown", "x"])), Mode::Run);
    }

    #[test]
    fn visualize_flags_render_the_schedule() {
        assert_eq!(parse_args(&args(&["--visualize"])), Mode::Visualize);
        assert_eq!(parse_args(&args(&["--view"])), Mode::Visualize);
        assert_eq!(parse_args(&args(&["--visualize", "--help"])), Mode::Help);
        assert_eq!(parse_args(&args(&["-h"])), Mode::Help);
    }

    #[test]
    fn visualize_is_disabled_in_p_mode() {
        let env = crate::test_support::TestEnv::new();
        env.write("tasks/mode.txt", "p");
        assert_eq!(visualize_schedule(), 1);
    }

    #[test]
    fn missing_settings_fail_without_starting_the_loop() {
        let _env = crate::test_support::TestEnv::new();
        assert_eq!(run_service(Vec::new()), 1);
    }
}
