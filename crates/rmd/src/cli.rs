//! `rmd` argument parsing and dispatch.
//!
//! The command tree, defaults, choices and help strings mirror the argparse
//! parser. Help, usage lines and usage errors are rendered in argparse's
//! layout from the clap definitions; usage errors exit with status 2, and a
//! bare `rmd` prints the help and exits with status 1.
//!
//! Hidden subcommands used internally (not shown in help):
//! - `rmd __api [JSON]`            -> `web::api::cli_main` (JSON request from argv or stdin)
//! - `rmd __web-server --root R --port P` -> `web::server::run`
//! - `rmd daemon [ARGS..]`         -> `reminder::run_service` (the background reminder service)
//! - `rmd __setup-config [ARGS..]` -> `install_config_wizard::run`

use std::ffi::OsString;
use std::io::{IsTerminal, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

use clap::builder::{PossibleValuesParser, Styles};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{Arg, ArgAction, Args, ColorChoice, CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::commands;
use crate::i18n::{t, tf};

// =============================================================================
// Command tree
// =============================================================================

/// The `rmd` command line.
#[derive(Parser, Debug)]
#[command(name = "rmd", bin_name = "rmd", disable_help_subcommand = true, disable_version_flag = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// `rmd` subcommands, in the order the help lists them.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the local browser workspace (reuses an existing tab)
    Web(WebArgs),

    /// Add a new task with description and priority level (1-10)
    #[command(long_about = "Add a task to your task list with a priority level.", allow_negative_numbers = true)]
    Add(AddArgs),

    /// Delete one or more tasks by description or ID
    #[command(long_about = "Remove tasks from your task list.", allow_negative_numbers = true)]
    Rm(TaskIdsArgs),

    /// Cancel tasks added by mistake (not counted as done)
    #[command(
        long_about = "Remove tasks that were added by mistake. Like 'rm' but recorded as 'cancelled' so it does not count as a completion in history/reports.",
        allow_negative_numbers = true
    )]
    Cancel(TaskIdsArgs),

    /// Give up on tasks (not counted as done)
    #[command(
        long_about = "Abandon tasks you no longer intend to do. Like 'rm' but recorded as 'dropped' so it does not count as a completion in history/reports.",
        allow_negative_numbers = true
    )]
    Drop(TaskIdsArgs),

    /// Show all tasks sorted by importance
    #[command(long_about = "Display your task list sorted by priority.")]
    Ls(LsArgs),

    /// Show recent completed task activities
    #[command(
        long_about = "Display the most recent completed tasks with lifecycle details.",
        allow_negative_numbers = true
    )]
    History(HistoryArgs),

    /// Manage deadlines (use 'ddl add' or just 'ddl' to list)
    #[command(
        long_about = "Deadline management. Without subcommand, shows all deadlines.",
        subcommand_help_heading = "Deadline commands"
    )]
    Ddl {
        #[command(subcommand)]
        command: Option<DdlCommand>,
    },

    /// Track completed habits for today
    #[command(
        long_about = "Mark habits as completed. Opens interactive prompt if no IDs given.",
        allow_negative_numbers = true
    )]
    Track(TrackArgs),

    /// Show current status and next events
    #[command(long_about = "Display current event, next scheduled event, and optionally full schedule.")]
    Status(StatusArgs),

    /// Generate schedule visualizations
    #[command(long_about = "Create a multi-page PDF visualization of your schedules.")]
    View,

    /// Show, save, or clear today's plan for pomodoro/potato blocks
    #[command(
        long_about = "Show today's focus blocks and their planned tasks. AI agents with the Schedule Everything plugin read --json and save plans with --apply."
    )]
    Sync(SyncArgs),

    /// Tasks suggested by AI agents, waiting for your review
    #[command(
        long_about = "Tasks an AI agent suggests. They wait on the browser task board, marked as suggestions, until you accept or reject them there. Without subcommand, lists the suggestions waiting for review.",
        subcommand_help_heading = "Suggestion commands"
    )]
    Suggest {
        #[command(subcommand)]
        command: Option<SuggestCommand>,
    },

    /// Validate the schedule configuration files
    #[command(
        long_about = "Validate settings.toml, the week schedules, habits.toml, and ddl.json in the active config directory (or --dir), including that every schedule label is defined in settings.toml."
    )]
    Check(CheckArgs),

    /// Reload config and pull from git when available
    #[command(
        long_about = "Reload the reminder service, pulling latest schedule files first when the config directory is git-managed."
    )]
    Update,

    /// Stop the reminder service
    #[command(long_about = "Stop the running reminder-runner background service.")]
    Stop,

    /// Switch to a different versioned config set and reload the service
    #[command(long_about = "Activate a different user_config_n directory and reload the reminder service.")]
    Switch(SwitchArgs),

    /// Generate weekly or monthly PDF reports
    #[command(long_about = "Generate a productivity report for a specified time period.")]
    Report(ReportArgs),

    /// Edit schedule configuration files
    #[command(long_about = "Open a configuration file in your editor.")]
    Edit(EditArgs),

    /// Print a shell completion script for bash, zsh, tcsh, or fish
    #[command(long_about = "Generate a shell completion script for the rmd CLI.")]
    Completion(CompletionArgs),

    /// Interactive TUI editor for settings.toml
    #[command(long_about = "Browse and edit settings.toml interactively with keyboard navigation.")]
    Settings,

    /// Switch or display the current mode (j mode or p mode)
    #[command(
        long_about = "Switch or display the current mode. j mode allows all notifications, p mode cancels specific event notifications."
    )]
    Mode(ModeArgs),

    /// Browser API request (JSON from the argument or standard input).
    #[command(name = "__api", hide = true, disable_help_flag = true)]
    Api(PassthroughArgs),

    /// Local browser workspace server.
    #[command(name = "__web-server", hide = true)]
    WebServer(WebServerArgs),

    /// The background reminder service.
    #[command(name = "daemon", hide = true, disable_help_flag = true)]
    Daemon(PassthroughArgs),

    /// First-run config wizard used by the installer.
    #[command(name = "__setup-config", hide = true, disable_help_flag = true)]
    SetupConfig(PassthroughArgs),
}

/// `rmd ddl` subcommands.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum DdlCommand {
    /// Add a new deadline event
    #[command(long_about = "Add a deadline with name and due date.")]
    Add(DdlAddArgs),

    /// Delete one or more deadline events
    #[command(long_about = "Remove deadlines by their event names.")]
    Rm(DdlRmArgs),
}

/// `rmd suggest` subcommands.
#[derive(Subcommand, Debug, PartialEq, Eq)]
pub enum SuggestCommand {
    /// List the suggestions waiting for review
    #[command(
        long_about = "List the suggestions waiting for review. With --json, also print the task types, the open tasks, and the user's recent decisions, so an agent can avoid suggesting a task twice."
    )]
    Ls(SuggestLsArgs),

    /// Suggest a task for the user to accept or reject
    #[command(
        long_about = "Suggest a task. It waits on the browser task board, marked as a suggestion, until the user accepts it (it becomes a normal task) or rejects it. Suggesting the same description again updates the waiting suggestion.",
        allow_negative_numbers = true
    )]
    Add(SuggestAddArgs),

    /// Withdraw suggestions that are still waiting for review
    #[command(long_about = "Withdraw suggestions that are still waiting for review, by ID or description.")]
    Rm(SuggestRmArgs),
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct WebArgs {
    /// Start the local server without opening a browser
    #[arg(long)]
    pub no_open: bool,
    /// Local port for a new server (default: automatically chosen)
    #[arg(long, value_name = "PORT", default_value_t = 0, value_parser = parse_port)]
    pub port: u16,
    /// Only restart a running server that is on outdated code; never start one or open a tab
    #[arg(long, conflicts_with_all = ["phone", "no_phone"])]
    pub if_running: bool,
    /// Let your phone use the workspace over Wi-Fi or Tailscale, and show its pairing code
    #[arg(long, conflicts_with = "no_phone")]
    pub phone: bool,
    /// Turn phone access off and sign every phone out
    #[arg(long)]
    pub no_phone: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct AddArgs {
    /// Description of the task (e.g., 'biology homework')
    #[arg(value_name = "task")]
    pub task: Option<String>,
    /// Priority level 1-10 (higher = more important)
    #[arg(value_name = "PRIORITY", value_parser = parse_priority)]
    pub priority: Option<i64>,
    /// Task type ID (e.g., 1 for 'read papers')
    #[arg(value_name = "task_type", value_parser = parse_int)]
    pub task_type: Option<i64>,
    /// Optional days to postpone daily urgent alarms (e.g., 1 for tomorrow, 2 for two days later)
    #[arg(value_name = "postpone", value_parser = parse_int)]
    pub postpone: Option<i64>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct TaskIdsArgs {
    /// Task descriptions or ID numbers from 'rmd ls'
    #[arg(value_name = "tasks", required = true, num_args = 1..)]
    pub tasks: Vec<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct LsArgs {
    #[arg(
        short = 't',
        long = "type",
        value_name = "TASK_TYPE",
        help = "Only show tasks of this type. Accepts a task type ID (e.g. 1) or name (e.g. 'coding')."
    )]
    pub task_type: Option<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct HistoryArgs {
    /// Number of recent activities to show (default: 5)
    #[arg(value_name = "count", default_value_t = 5, value_parser = parse_int)]
    pub count: i64,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct DdlAddArgs {
    /// Name of the event (e.g., 'homework2')
    #[arg(value_name = "event")]
    pub event: String,
    /// Due date in M.D or MM.DD format (e.g., '7.4' for July 4th)
    #[arg(value_name = "date")]
    pub date: String,
    /// Repeat the deadline weekly, every two weeks, or monthly
    #[arg(long, value_parser = PossibleValuesParser::new(["weekly", "biweekly", "monthly"]))]
    pub repeat: Option<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct DdlRmArgs {
    /// Event names to delete (e.g., 'homework2' 'project')
    #[arg(value_name = "events", required = true, num_args = 1..)]
    pub events: Vec<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct SuggestLsArgs {
    /// Print the suggestions, task types, open tasks, and recent decisions as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct SuggestAddArgs {
    /// Description of the suggested task
    #[arg(value_name = "task")]
    pub task: String,
    /// Priority level 1-10 (higher = more important)
    #[arg(value_name = "PRIORITY", value_parser = parse_priority)]
    pub priority: i64,
    /// Task type ID or name (default: the first type)
    #[arg(short = 't', long = "type", value_name = "TASK_TYPE")]
    pub task_type: Option<String>,
    /// Finish-by time for a timed task, e.g. 2026-11-12T17:00
    #[arg(long, value_name = "WHEN")]
    pub due: Option<String>,
    /// Days to postpone the daily urgent alarm
    #[arg(long, value_name = "DAYS", value_parser = parse_int)]
    pub postpone: Option<i64>,
    /// Why you suggest it, in one sentence the user will read
    #[arg(long, value_name = "TEXT")]
    pub reason: Option<String>,
    /// Where you found it, e.g. Gmail or Calendar
    #[arg(long, value_name = "TEXT")]
    pub source: Option<String>,
    /// Who suggests it, e.g. Claude
    #[arg(long, value_name = "NAME")]
    pub agent: Option<String>,
    /// Suggest it even though the user rejected it before
    #[arg(long)]
    pub again: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct SuggestRmArgs {
    /// Suggestion IDs or descriptions from 'rmd suggest ls'
    #[arg(value_name = "suggestions", required = true, num_args = 1..)]
    pub suggestions: Vec<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct TrackArgs {
    /// Optional habit IDs to mark complete (e.g., '1 2 3')
    #[arg(value_name = "habit_ids", num_args = 0..)]
    pub habit_ids: Vec<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct StatusArgs {
    /// Show detailed schedule for today
    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
#[group(id = "sync_action", multiple = false)]
pub struct SyncArgs {
    /// Print today's focus slots, tasks, deadlines, and plan as JSON
    #[arg(long)]
    pub json: bool,
    /// Save a plan for today from a JSON file ("-" reads standard input)
    #[arg(long, value_name = "FILE", allow_hyphen_values = true)]
    pub apply: Option<String>,
    /// Remove today's plan
    #[arg(long)]
    pub clear: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct CheckArgs {
    /// Config directory to check instead of the active one
    #[arg(long, value_name = "DIR")]
    pub dir: Option<String>,
    /// Print the result and config paths as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct SwitchArgs {
    /// Numeric config id to activate (for example: 0, 1, 2)
    #[arg(value_name = "config_id")]
    pub config_id: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ReportArgs {
    /// Type of report to generate
    #[arg(value_name = "type", value_parser = PossibleValuesParser::new(["weekly", "monthly"]))]
    pub report_type: String,
    /// Target date in YYYY-MM-DD format (default: today)
    #[arg(short, long, value_name = "DATE")]
    pub date: Option<String>,
    /// Compatibility flag; only '--days 7' is accepted for weekly reports
    #[arg(long, value_name = "DAYS", value_parser = parse_int, allow_negative_numbers = true)]
    pub days: Option<i64>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct EditArgs {
    /// File to edit (default: settings)
    #[arg(
        value_name = "file",
        default_value = "settings",
        value_parser = PossibleValuesParser::new(["settings", "odd", "even", "deadlines", "ddl", "habits"])
    )]
    pub file: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct CompletionArgs {
    /// Shell type to target (default: bash)
    #[arg(
        value_name = "shell",
        default_value = "bash",
        value_parser = PossibleValuesParser::new(commands::completion::SUPPORTED_COMPLETION_SHELLS)
    )]
    pub shell: String,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct ModeArgs {
    /// Mode to switch to ('j' or 'p')
    #[arg(value_name = "mode", value_parser = PossibleValuesParser::new(["j", "p"]))]
    pub mode: Option<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct PassthroughArgs {
    /// Arguments passed through unchanged.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
    pub args: Vec<String>,
}

#[derive(Args, Debug, PartialEq, Eq)]
pub struct WebServerArgs {
    /// Config root to serve.
    #[arg(long)]
    pub root: String,
    /// Port to listen on (0 picks a free port).
    #[arg(long, default_value_t = 0, value_parser = parse_port)]
    pub port: u16,
}

/// argparse `type=int`: Python `int()` semantics, argparse's message.
fn parse_int(value: &str) -> Result<i64, String> {
    crate::py::parse_int_str(value).ok_or_else(|| format!("invalid int value: {}", crate::py::repr_str(value)))
}

fn parse_port(value: &str) -> Result<u16, String> {
    let number = parse_int(value)?;
    u16::try_from(number).map_err(|_| format!("invalid port: {number}"))
}

/// `type=int, choices=range(1, 11)`.
fn parse_priority(value: &str) -> Result<i64, String> {
    let number = parse_int(value)?;
    if (1..=10).contains(&number) {
        Ok(number)
    } else {
        Err(format!(
            "invalid choice: {} (choose from 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)",
            crate::py::repr_str(&number.to_string())
        ))
    }
}

// =============================================================================
// argparse-style help, usage and errors
// =============================================================================

/// Root description and epilog (coloured like the Python CLI; the colours
/// are dropped when stdout is not a terminal).
fn root_description() -> String {
    format!("\x1b[1m\x1b[96m{}\x1b[0m - \x1b[92m{}\x1b[0m", t("rmd CLI"), t("Manage your schedule management system"))
}

fn root_epilog() -> String {
    let root = crate::paths::resolve_config_root_dir();
    let active = crate::paths::preview_active_config_dir(Some(&root));
    format!(
        "\x1b[4m\x1b[93m{}\x1b[0m \x1b[94m{}\x1b[0m\n\x1b[4m\x1b[93m{}\x1b[0m \x1b[94m{}\x1b[0m\n    ",
        t("Configuration root:"),
        root.display(),
        t("Active config:"),
        active.display()
    )
}

/// argparse's formatter width: `shutil.get_terminal_size().columns - 2`.
fn help_width() -> usize {
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.trim().parse::<usize>().ok())
        .filter(|c| *c > 0)
        .or_else(|| {
            std::io::stdout()
                .is_terminal()
                .then(crossterm::terminal::size)
                .and_then(Result::ok)
                .map(|(w, _)| usize::from(w))
                .filter(|w| *w > 0)
        })
        .unwrap_or(80);
    columns.saturating_sub(2).max(1)
}

/// `{a,b,c}` for an argument's choices.
fn choices_metavar(arg: &Arg) -> Option<String> {
    let values: Vec<String> = arg.get_possible_values().iter().map(|v| v.get_name().to_string()).collect();
    (!values.is_empty()).then(|| format!("{{{}}}", values.join(",")))
}

/// The argparse metavar of an argument: its choices, else its value name.
fn metavar(arg: &Arg) -> String {
    if let Some(choices) = choices_metavar(arg) {
        return choices;
    }
    arg.get_value_names()
        .and_then(|names| names.first())
        .map(|n| n.to_string())
        .unwrap_or_else(|| arg.get_id().as_str().to_uppercase())
}

fn takes_value(arg: &Arg) -> bool {
    !matches!(arg.get_action(), ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count | ArgAction::Help)
}

fn is_multiple(arg: &Arg) -> bool {
    arg.get_num_args().is_some_and(|n| n.max_values() > 1)
}

/// `%(prog)s`-relative usage of a positional (`_format_args`).
fn positional_usage(arg: &Arg) -> String {
    let name = metavar(arg);
    match (arg.is_required_set(), is_multiple(arg)) {
        (true, true) => format!("{name} [{name} ...]"),
        (false, true) => format!("[{name} ...]"),
        (true, false) => name,
        (false, false) => format!("[{name}]"),
    }
}

/// First option string with its value, as in the usage line (`-t TASK_TYPE`).
fn option_usage(arg: &Arg) -> String {
    let flag = match (arg.get_short(), arg.get_long()) {
        (Some(short), _) => format!("-{short}"),
        (None, Some(long)) => format!("--{long}"),
        (None, None) => arg.get_id().to_string(),
    };
    if takes_value(arg) { format!("{flag} {}", metavar(arg)) } else { flag }
}

/// Help invocation of an option (`-t TASK_TYPE, --type TASK_TYPE`).
fn option_invocation(arg: &Arg) -> String {
    let mut parts = Vec::new();
    if let Some(short) = arg.get_short() {
        parts.push(format!("-{short}"));
    }
    if let Some(long) = arg.get_long() {
        parts.push(format!("--{long}"));
    }
    if takes_value(arg) {
        let value = metavar(arg);
        parts = parts.into_iter().map(|p| format!("{p} {value}")).collect();
    }
    parts.join(", ")
}

fn visible_options(cmd: &clap::Command) -> Vec<&Arg> {
    cmd.get_arguments().filter(|a| !a.is_positional() && !a.is_hide_set() && a.get_id() != "help").collect()
}

fn visible_positionals(cmd: &clap::Command) -> Vec<&Arg> {
    cmd.get_positionals().filter(|a| !a.is_hide_set()).collect()
}

fn visible_subcommands(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands().filter(|s| !s.is_hide_set()).collect()
}

/// The subcommand placeholder (`<command>` at the top level, `{add,rm}` below).
fn subcommand_metavar(cmd: &clap::Command, is_root: bool) -> String {
    if is_root {
        "<command>".to_string()
    } else {
        format!("{{{}}}", visible_subcommands(cmd).iter().map(|s| s.get_name()).collect::<Vec<_>>().join(","))
    }
}

/// argparse's usage line (without the `usage: ` prefix).
fn usage_line(cmd: &clap::Command, prog: &str, is_root: bool) -> String {
    let mut parts = vec![prog.to_string(), "[-h]".to_string()];
    let mut grouped: Vec<String> = Vec::new();
    for group in cmd.get_groups().filter(|g| !(*g).clone().is_multiple()) {
        let members: Vec<String> = group.get_args().map(|id| id.to_string()).collect();
        let rendered: Vec<String> = members
            .iter()
            .filter_map(|id| cmd.get_arguments().find(|a| a.get_id() == id.as_str()))
            .map(option_usage)
            .collect();
        if !rendered.is_empty() {
            parts.push(format!("[{}]", rendered.join(" | ")));
            grouped.extend(members);
        }
    }
    for arg in visible_options(cmd) {
        if !grouped.iter().any(|id| arg.get_id() == id.as_str()) {
            parts.push(format!("[{}]", option_usage(arg)));
        }
    }
    for arg in visible_positionals(cmd) {
        parts.push(positional_usage(arg));
    }
    if !visible_subcommands(cmd).is_empty() {
        parts.push(format!("{} ...", subcommand_metavar(cmd, is_root)));
    }
    parts.join(" ")
}

/// `textwrap.wrap` after collapsing whitespace.
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let candidate =
            if current.is_empty() { word.len() } else { current.chars().count() + 1 + word.chars().count() };
        if !current.is_empty() && candidate > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

/// One help entry: invocation, help text, indent.
struct HelpItem {
    invocation: String,
    help: Option<String>,
    indent: usize,
}

fn format_items(items: &[HelpItem], help_position: usize, width: usize) -> String {
    let mut out = String::new();
    for item in items {
        let action_width = help_position.saturating_sub(item.indent + 2);
        let indent = " ".repeat(item.indent);
        let help = item.help.as_deref().filter(|h| !h.trim().is_empty());
        let Some(help) = help else {
            out.push_str(&format!("{indent}{}\n", item.invocation));
            continue;
        };
        let help_lines = wrap_words(help, width.saturating_sub(help_position).max(11));
        if item.invocation.chars().count() <= action_width {
            out.push_str(&format!("{indent}{:<action_width$}  ", item.invocation));
            out.push_str(help_lines.first().map(String::as_str).unwrap_or(""));
            out.push('\n');
        } else {
            out.push_str(&format!("{indent}{}\n", item.invocation));
            out.push_str(&format!(
                "{}{}\n",
                " ".repeat(help_position),
                help_lines.first().cloned().unwrap_or_default()
            ));
        }
        for line in help_lines.iter().skip(1) {
            out.push_str(&format!("{}{}\n", " ".repeat(help_position), line));
        }
    }
    out
}

/// Full argparse-style help for a command.
fn argparse_help(cmd: &clap::Command, prog: &str, is_root: bool, width: usize) -> String {
    let positionals: Vec<HelpItem> = visible_positionals(cmd)
        .into_iter()
        .map(|a| HelpItem { invocation: metavar(a), help: a.get_help().map(|h| h.to_string()), indent: 2 })
        .collect();
    let mut options = vec![HelpItem {
        invocation: "-h, --help".to_string(),
        help: Some("show this help message and exit".to_string()),
        indent: 2,
    }];
    options.extend(visible_options(cmd).into_iter().map(|a| HelpItem {
        invocation: option_invocation(a),
        help: a.get_help().map(|h| h.to_string()),
        indent: 2,
    }));
    let subcommands = visible_subcommands(cmd);
    let mut commands: Vec<HelpItem> = Vec::new();
    if !subcommands.is_empty() {
        commands.push(HelpItem { invocation: subcommand_metavar(cmd, is_root), help: None, indent: 2 });
        commands.extend(subcommands.iter().map(|s| HelpItem {
            invocation: s.get_name().to_string(),
            help: s.get_about().map(|h| h.to_string()),
            indent: 4,
        }));
    }
    let longest = positionals.iter().chain(&options).chain(&commands).map(|i| i.indent + i.invocation.chars().count());
    let max_help_position = 24.min(width.saturating_sub(20).max(4));
    let help_position = (longest.max().unwrap_or(0) + 2).min(max_help_position);

    let mut out = format!("usage: {}\n", usage_line(cmd, prog, is_root));
    if is_root {
        out.push_str(&format!("\n{}\n", root_description()));
    } else if let Some(description) = cmd.get_long_about() {
        out.push('\n');
        for line in wrap_words(&description.to_string(), width) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    if !positionals.is_empty() {
        out.push_str("\npositional arguments:\n");
        out.push_str(&format_items(&positionals, help_position, width));
    }
    out.push_str("\noptions:\n");
    out.push_str(&format_items(&options, help_position, width));
    if !commands.is_empty() {
        let title = cmd.get_subcommand_help_heading().map(str::to_string).unwrap_or_else(|| t("Available commands"));
        out.push_str(&format!("\n{title}:\n"));
        out.push_str(&format_items(&commands, help_position, width));
    }
    if is_root {
        out.push_str(&format!("\n{}\n", root_epilog()));
    }
    out
}

/// Attach argparse-style help and usage to a command and its subcommands.
fn with_argparse_help(cmd: clap::Command, prog: &str, is_root: bool, width: usize) -> clap::Command {
    let names: Vec<String> = cmd.get_subcommands().map(|s| s.get_name().to_string()).collect();
    let mut cmd = cmd;
    for name in names {
        let sub_prog = format!("{prog} {name}");
        cmd = cmd.mut_subcommand(&name, |sub| with_argparse_help(sub, &sub_prog, false, width));
    }
    let help = argparse_help(&cmd, prog, is_root, width);
    let usage = usage_line(&cmd, prog, is_root);
    cmd.help_template("{before-help}").before_help(help).override_usage(usage)
}

/// The `rmd` clap command with argparse-style help (also used for completions).
pub fn build_command() -> clap::Command {
    let cmd = Cli::command().styles(Styles::plain()).color(ColorChoice::Auto).subcommand_value_name("<command>");
    with_argparse_help(cmd, "rmd", true, help_width())
}

fn context_string(error: &clap::Error, kind: ContextKind) -> Option<String> {
    match error.get(kind)? {
        ContextValue::String(s) => Some(s.clone()),
        ContextValue::Strings(v) => Some(v.join(", ")),
        ContextValue::StyledStr(s) => Some(s.to_string()),
        ContextValue::StyledStrs(v) => Some(v.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", ")),
        ContextValue::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// argparse's name for an argument as clap renders it (`[PRIORITY]`,
/// `--repeat <REPEAT>`, `<tasks>...`).
fn argparse_arg_name(rendered: &str, cmd: &clap::Command) -> String {
    let rendered = rendered.trim();
    if rendered.starts_with('-') {
        let flag = rendered.split_whitespace().next().unwrap_or(rendered);
        let found = cmd.get_arguments().find(|a| {
            a.get_long().is_some_and(|l| flag == format!("--{l}"))
                || a.get_short().is_some_and(|s| flag == format!("-{s}"))
        });
        return match found {
            Some(arg) => {
                let mut names = Vec::new();
                if let Some(short) = arg.get_short() {
                    names.push(format!("-{short}"));
                }
                if let Some(long) = arg.get_long() {
                    names.push(format!("--{long}"));
                }
                names.join("/")
            }
            None => flag.to_string(),
        };
    }
    rendered.trim_end_matches("...").trim_matches(|c| matches!(c, '<' | '>' | '[' | ']')).to_string()
}

/// The subcommand path an argument vector selects (`["ddl", "add"]`).
fn command_path(cmd: &clap::Command, argv: &[OsString]) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = cmd;
    for token in argv.iter().skip(1) {
        let Some(token) = token.to_str() else { break };
        if token == "--" || visible_subcommands(current).is_empty() && current.get_subcommands().next().is_none() {
            break;
        }
        if token.starts_with('-') {
            continue;
        }
        match current.find_subcommand(token) {
            Some(sub) => {
                path.push(token.to_string());
                current = sub;
            }
            None => break,
        }
    }
    path
}

/// argparse's `(usage, message)` for a clap usage error raised while
/// parsing `path`.
fn argparse_error(error: &clap::Error, cmd: &clap::Command, path: &[String]) -> (String, String) {
    let root_usage = usage_line(cmd, "rmd", true);
    let target = path.iter().try_fold(cmd, |c, name| c.find_subcommand(name)).unwrap_or(cmd);
    let prog = std::iter::once("rmd").chain(path.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    let usage = usage_line(target, &prog, path.is_empty());
    let arg = context_string(error, ContextKind::InvalidArg).map(|a| argparse_arg_name(&a, target)).unwrap_or_default();
    let value = context_string(error, ContextKind::InvalidValue).unwrap_or_default();
    let message = match error.kind() {
        ErrorKind::InvalidValue => {
            let choices = match error.get(ContextKind::ValidValue) {
                Some(ContextValue::Strings(values)) => values.join(", "),
                _ => String::new(),
            };
            if value.is_empty() && choices.is_empty() {
                format!("argument {arg}: expected one argument")
            } else {
                format!("argument {arg}: invalid choice: {} (choose from {choices})", crate::py::repr_str(&value))
            }
        }
        ErrorKind::ValueValidation => {
            let detail = std::error::Error::source(error).map(|e| e.to_string()).unwrap_or_default();
            format!("argument {arg}: {detail}")
        }
        ErrorKind::MissingRequiredArgument => {
            let missing: Vec<String> = match error.get(ContextKind::InvalidArg) {
                Some(ContextValue::Strings(values)) => values.iter().map(|v| argparse_arg_name(v, target)).collect(),
                _ => vec![arg],
            };
            format!("the following arguments are required: {}", missing.join(", "))
        }
        ErrorKind::ArgumentConflict => {
            let prior =
                context_string(error, ContextKind::PriorArg).map(|a| argparse_arg_name(&a, target)).unwrap_or_default();
            format!("argument {prior}: not allowed with argument {arg}")
        }
        ErrorKind::InvalidSubcommand => {
            let name = context_string(error, ContextKind::InvalidSubcommand).unwrap_or_default();
            let label = match path.last() {
                None => "<command>".to_string(),
                Some(last) => format!("{last}_command"),
            };
            format!(
                "argument {label}: invalid choice: {} (choose from {})",
                crate::py::repr_str(&name),
                commands_list(target)
            )
        }
        ErrorKind::UnknownArgument | ErrorKind::TooManyValues => {
            let value = if value.is_empty() { arg.clone() } else { value };
            return (root_usage, format!("unrecognized arguments: {value}"));
        }
        ErrorKind::NoEquals | ErrorKind::WrongNumberOfValues | ErrorKind::TooFewValues => {
            format!("argument {arg}: expected one argument")
        }
        _ => {
            let rendered = strip_ansi(&error.to_string());
            rendered.lines().next().unwrap_or("").trim_start_matches("error:").trim().to_string()
        }
    };
    (usage, message)
}

fn commands_list(cmd: &clap::Command) -> String {
    visible_subcommands(cmd).iter().map(|s| s.get_name()).collect::<Vec<_>>().join(", ")
}

/// The argparse-style text for a usage error.
fn usage_error_text(error: &clap::Error, cmd: &clap::Command, argv: &[OsString]) -> String {
    let (usage, message) = argparse_error(error, cmd, &command_path(cmd, argv));
    let prog = usage.split(" [-h]").next().unwrap_or("rmd").to_string();
    format!("usage: {usage}\n{prog}: error: {message}")
}

/// Print a clap error the argparse way; returns the exit code.
fn report_parse_error(error: clap::Error, cmd: &clap::Command, argv: &[OsString]) -> i32 {
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            // Print the argparse-style text as is (clap would re-wrap it).
            let path = command_path(cmd, argv);
            let target = path.iter().try_fold(cmd, |c, name| c.find_subcommand(name)).unwrap_or(cmd);
            let rendered = target.get_before_help().map(|h| h.to_string()).unwrap_or_default();
            let mut stdout = std::io::stdout().lock();
            let text = if crate::ui::detect_color_system().is_some() { rendered } else { strip_ansi(&rendered) };
            let _ = stdout.write_all(text.as_bytes());
            let _ = stdout.flush();
            0
        }
        _ => {
            eprintln!("{}", usage_error_text(&error, cmd, argv));
            2
        }
    }
}

fn strip_ansi(text: &str) -> String {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new("\x1b\\[[0-9;]*m").expect("valid ANSI regex"));
    re.replace_all(text, "").into_owned()
}

/// Help text of a command path (`[]` is the top level), as printed by `-h`.
pub fn help_text(path: &[&str]) -> String {
    let cmd = build_command();
    let target = path.iter().try_fold(&cmd, |c, name| c.find_subcommand(name)).unwrap_or(&cmd);
    target.get_before_help().map(|h| h.to_string()).unwrap_or_default()
}

// =============================================================================
// Dispatch
// =============================================================================

/// Parse `std::env::args()` and run the command; returns the exit code.
pub fn main() -> i32 {
    let code = run(std::env::args_os().collect());
    let _ = std::io::stdout().flush();
    code
}

/// Parse an argument vector (including the program name) and run it.
pub fn run(argv: Vec<OsString>) -> i32 {
    let mut cmd = build_command();
    let matches = match cmd.try_get_matches_from_mut(argv.clone()) {
        Ok(matches) => matches,
        Err(error) => return report_parse_error(error, &cmd, &argv),
    };
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(error) => return report_parse_error(error, &cmd, &argv),
    };
    let Some(command) = cli.command else {
        let help = cmd.get_before_help().map(|h| h.to_string()).unwrap_or_default();
        let text = if crate::ui::detect_color_system().is_some() { help } else { strip_ansi(&help) };
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(text.as_bytes());
        let _ = stdout.flush();
        return 1;
    };
    if is_internal(&command) {
        return dispatch(command);
    }
    run_user_command(command)
}

fn is_internal(command: &Command) -> bool {
    matches!(command, Command::Api(_) | Command::WebServer(_) | Command::Daemon(_) | Command::SetupConfig(_))
}

/// Run a user-facing command with the Python CLI's error handling: Ctrl+C
/// prints "Operation cancelled" and any unexpected failure prints
/// "❌ Unexpected error: ..."; both exit with status 1.
fn run_user_command(command: Command) -> i32 {
    crate::ui::install_cancel_handler(&format!("\n{}\n", t("❌ Operation cancelled by user")));
    let verbose_panics = std::env::var_os("RUST_BACKTRACE").is_some_and(|v| !v.is_empty() && v != "0");
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if verbose_panics {
            previous_hook(info);
        }
    }));
    match catch_unwind(AssertUnwindSafe(|| dispatch(command))) {
        Ok(code) => code,
        Err(payload) => {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "unknown error".to_string());
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", tf("❌ Unexpected error: {e}", &[("e", &message)]));
            let _ = stdout.flush();
            1
        }
    }
}

fn dispatch(command: Command) -> i32 {
    use commands::{completion, deadlines, habits, history, service, status, suggest, sync, tasks};
    match command {
        Command::Web(a) => commands::browser::browser_command(commands::browser::WebRequest {
            no_open: a.no_open,
            port: a.port,
            if_running: a.if_running,
            phone: if a.phone {
                Some(true)
            } else if a.no_phone {
                Some(false)
            } else {
                None
            },
        }),
        Command::Add(a) => tasks::add_task(a.task.as_deref(), a.priority, a.task_type, a.postpone),
        Command::Rm(a) => tasks::delete_task(&a.tasks),
        Command::Cancel(a) => tasks::cancel_task(&a.tasks),
        Command::Drop(a) => tasks::drop_task(&a.tasks),
        Command::Ls(a) => tasks::show_tasks(a.task_type.as_deref()),
        Command::History(a) => history::history_command(a.count),
        Command::Ddl { command } => match command {
            None => deadlines::show_deadlines(),
            Some(DdlCommand::Add(a)) => deadlines::add_deadline(&a.event, &a.date, a.repeat.as_deref()),
            Some(DdlCommand::Rm(a)) => deadlines::delete_deadline(&a.events),
        },
        Command::Track(a) => habits::track_habits(&a.habit_ids),
        Command::Status(a) => status::status_command(a.verbose),
        Command::View => status::view_command(),
        Command::Sync(a) => sync::sync_command(a.json, a.apply.as_deref(), a.clear),
        Command::Suggest { command } => match command {
            None => suggest::list_command(false),
            Some(SuggestCommand::Ls(a)) => suggest::list_command(a.json),
            Some(SuggestCommand::Add(a)) => suggest::add_command(&crate::suggestions::NewSuggestion {
                description: a.task,
                priority: a.priority,
                task_type: a.task_type,
                due: a.due,
                postpone: a.postpone,
                reason: a.reason,
                source: a.source,
                agent: a.agent,
                again: a.again,
            }),
            Some(SuggestCommand::Rm(a)) => suggest::withdraw_command(&a.suggestions),
        },
        Command::Check(a) => crate::config_check::check_command(a.dir.as_deref(), a.json),
        Command::Update => service::update_command(),
        Command::Stop => service::stop_command(),
        Command::Switch(a) => service::switch_command(&a.config_id),
        Command::Report(a) => service::report_command(&a.report_type, a.date.as_deref(), a.days),
        Command::Edit(a) => service::edit_schedule_command(&a.file),
        Command::Completion(a) => completion::completion_command(&a.shell),
        Command::Settings => commands::settings_tui::settings_command(),
        Command::Mode(a) => service::mode_command(a.mode.as_deref()),
        Command::Api(a) => crate::web::api::cli_main(a.args),
        Command::WebServer(a) => {
            let root = crate::paths::resolve(&crate::paths::expand_user(&a.root));
            crate::web::server::run(&root, a.port)
        }
        Command::Daemon(a) => crate::reminder::run_service(a.args),
        Command::SetupConfig(a) => crate::install_config_wizard::run(a.args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    fn parse(args: &[&str]) -> Result<Option<Command>, clap::Error> {
        let mut argv = vec!["rmd"];
        argv.extend_from_slice(args);
        let matches = build_command().try_get_matches_from(argv)?;
        Ok(Cli::from_arg_matches(&matches)?.command)
    }

    fn parse_ok(args: &[&str]) -> Command {
        parse(args).expect("parses").expect("has a command")
    }

    fn error_message(args: &[&str]) -> String {
        let error = parse(args).expect_err("usage error");
        let argv: Vec<OsString> = std::iter::once("rmd").chain(args.iter().copied()).map(OsString::from).collect();
        usage_error_text(&error, &build_command(), &argv)
    }

    #[test]
    fn parses_task_commands() {
        let _env = TestEnv::new();
        assert_eq!(
            parse_ok(&["add", "Write slides", "7"]),
            Command::Add(AddArgs {
                task: Some("Write slides".into()),
                priority: Some(7),
                task_type: None,
                postpone: None
            })
        );
        assert_eq!(
            parse_ok(&["add", "x", "5", "4", "-2"]),
            Command::Add(AddArgs { task: Some("x".into()), priority: Some(5), task_type: Some(4), postpone: Some(-2) })
        );
        assert_eq!(
            parse_ok(&["add"]),
            Command::Add(AddArgs { task: None, priority: None, task_type: None, postpone: None })
        );
        assert_eq!(
            parse_ok(&["rm", "1", "-1", "Call"]),
            Command::Rm(TaskIdsArgs { tasks: vec!["1".into(), "-1".into(), "Call".into()] })
        );
        assert_eq!(
            parse_ok(&["cancel", "some task"]),
            Command::Cancel(TaskIdsArgs { tasks: vec!["some task".into()] })
        );
        assert_eq!(parse_ok(&["drop", "1", "2"]), Command::Drop(TaskIdsArgs { tasks: vec!["1".into(), "2".into()] }));
        assert_eq!(parse_ok(&["ls", "-t", "1"]), Command::Ls(LsArgs { task_type: Some("1".into()) }));
        assert_eq!(parse_ok(&["ls", "--type", "coding"]), Command::Ls(LsArgs { task_type: Some("coding".into()) }));
        assert_eq!(parse_ok(&["ls"]), Command::Ls(LsArgs { task_type: None }));
        assert_eq!(parse_ok(&["history"]), Command::History(HistoryArgs { count: 5 }));
        assert_eq!(parse_ok(&["history", "10"]), Command::History(HistoryArgs { count: 10 }));
    }

    #[test]
    fn parses_other_commands_and_defaults() {
        let _env = TestEnv::new();
        assert_eq!(parse_ok(&["ddl"]), Command::Ddl { command: None });
        assert_eq!(
            parse_ok(&["ddl", "add", "rent", "12.1", "--repeat", "monthly"]),
            Command::Ddl {
                command: Some(DdlCommand::Add(DdlAddArgs {
                    event: "rent".into(),
                    date: "12.1".into(),
                    repeat: Some("monthly".into())
                }))
            }
        );
        assert_eq!(
            parse_ok(&["ddl", "rm", "a", "b"]),
            Command::Ddl { command: Some(DdlCommand::Rm(DdlRmArgs { events: vec!["a".into(), "b".into()] })) }
        );
        assert_eq!(parse_ok(&["track"]), Command::Track(TrackArgs { habit_ids: vec![] }));
        assert_eq!(parse_ok(&["status", "-v"]), Command::Status(StatusArgs { verbose: true }));
        assert_eq!(parse_ok(&["completion"]), Command::Completion(CompletionArgs { shell: "bash".into() }));
        assert_eq!(parse_ok(&["completion", "zsh"]), Command::Completion(CompletionArgs { shell: "zsh".into() }));
        assert_eq!(parse_ok(&["switch", "2"]), Command::Switch(SwitchArgs { config_id: "2".into() }));
        assert_eq!(parse_ok(&["edit"]), Command::Edit(EditArgs { file: "settings".into() }));
        assert_eq!(parse_ok(&["mode"]), Command::Mode(ModeArgs { mode: None }));
        assert_eq!(
            parse_ok(&["report", "weekly", "-d", "2026-11-01", "--days", "7"]),
            Command::Report(ReportArgs {
                report_type: "weekly".into(),
                date: Some("2026-11-01".into()),
                days: Some(7)
            })
        );
        assert_eq!(
            parse_ok(&["web", "--no-open", "--port", "8765"]),
            Command::Web(WebArgs { no_open: true, port: 8765, if_running: false, phone: false, no_phone: false })
        );
        assert_eq!(
            parse_ok(&["web", "--phone"]),
            Command::Web(WebArgs { no_open: false, port: 0, if_running: false, phone: true, no_phone: false })
        );
        assert_eq!(
            parse_ok(&["sync", "--apply", "-"]),
            Command::Sync(SyncArgs { json: false, apply: Some("-".into()), clear: false })
        );
        assert_eq!(
            parse_ok(&["check", "--dir", "x", "--json"]),
            Command::Check(CheckArgs { dir: Some("x".into()), json: true })
        );
        assert_eq!(parse(&[]).unwrap(), None);
    }

    #[test]
    fn parses_suggest_commands() {
        let _env = TestEnv::new();
        assert_eq!(parse_ok(&["suggest"]), Command::Suggest { command: None });
        assert_eq!(
            parse_ok(&["suggest", "ls", "--json"]),
            Command::Suggest { command: Some(SuggestCommand::Ls(SuggestLsArgs { json: true })) }
        );
        assert_eq!(
            parse_ok(&[
                "suggest",
                "add",
                "Reply to Lee",
                "8",
                "-t",
                "work",
                "--due",
                "2026-11-12T17:00",
                "--reason",
                "Asked by Thursday",
                "--source",
                "Gmail",
                "--agent",
                "Claude",
                "--again",
            ]),
            Command::Suggest {
                command: Some(SuggestCommand::Add(SuggestAddArgs {
                    task: "Reply to Lee".into(),
                    priority: 8,
                    task_type: Some("work".into()),
                    due: Some("2026-11-12T17:00".into()),
                    postpone: None,
                    reason: Some("Asked by Thursday".into()),
                    source: Some("Gmail".into()),
                    agent: Some("Claude".into()),
                    again: true,
                }))
            }
        );
        assert_eq!(
            parse_ok(&["suggest", "rm", "3", "Buy stamps"]),
            Command::Suggest {
                command: Some(SuggestCommand::Rm(SuggestRmArgs { suggestions: vec!["3".into(), "Buy stamps".into()] }))
            }
        );
        assert_eq!(
            error_message(&["suggest", "add", "x", "0"]),
            "usage: rmd suggest add [-h] [-t TASK_TYPE] [--due WHEN] [--postpone DAYS] [--reason TEXT] [--source TEXT] [--agent NAME] [--again] task PRIORITY\nrmd suggest add: error: argument PRIORITY: invalid choice: '0' (choose from 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)"
        );
    }

    #[test]
    fn parses_internal_commands_verbatim() {
        let _env = TestEnv::new();
        assert_eq!(
            parse_ok(&["__api", "{\"a\": 1}"]),
            Command::Api(PassthroughArgs { args: vec!["{\"a\": 1}".into()] })
        );
        assert_eq!(parse_ok(&["__api"]), Command::Api(PassthroughArgs { args: vec![] }));
        assert_eq!(
            parse_ok(&["__setup-config", "--config-dir", "x", "--yes", "--help"]),
            Command::SetupConfig(PassthroughArgs {
                args: vec!["--config-dir".into(), "x".into(), "--yes".into(), "--help".into()]
            })
        );
        assert_eq!(
            parse_ok(&["daemon", "--visualize"]),
            Command::Daemon(PassthroughArgs { args: vec!["--visualize".into()] })
        );
        assert_eq!(
            parse_ok(&["__web-server", "--root", "/tmp/x", "--port", "0"]),
            Command::WebServer(WebServerArgs { root: "/tmp/x".into(), port: 0 })
        );
    }

    #[test]
    fn usage_errors_read_like_argparse() {
        let _env = TestEnv::new();
        assert_eq!(
            error_message(&["add", "x", "11"]),
            "usage: rmd add [-h] [task] [PRIORITY] [task_type] [postpone]\nrmd add: error: argument PRIORITY: invalid choice: '11' (choose from 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)"
        );
        assert_eq!(
            error_message(&["add", "x", "abc"]),
            "usage: rmd add [-h] [task] [PRIORITY] [task_type] [postpone]\nrmd add: error: argument PRIORITY: invalid int value: 'abc'"
        );
        assert_eq!(
            error_message(&["sync", "--json", "--clear"]),
            "usage: rmd sync [-h] [--json | --apply FILE | --clear]\nrmd sync: error: argument --clear: not allowed with argument --json"
        );
        assert_eq!(
            error_message(&["web", "--phone", "--no-phone"]),
            "usage: rmd web [-h] [--no-open] [--port PORT] [--if-running] [--phone] [--no-phone]\nrmd web: error: argument --no-phone: not allowed with argument --phone"
        );
        assert_eq!(
            error_message(&["frobnicate"]),
            "usage: rmd [-h] <command> ...\nrmd: error: argument <command>: invalid choice: 'frobnicate' (choose from web, add, rm, cancel, drop, ls, history, ddl, track, status, view, sync, suggest, check, update, stop, switch, report, edit, completion, settings, mode)"
        );
        assert_eq!(
            error_message(&["ddl", "add", "x"]),
            "usage: rmd ddl add [-h] [--repeat {weekly,biweekly,monthly}] event date\nrmd ddl add: error: the following arguments are required: date"
        );
        assert_eq!(
            error_message(&["ddl", "add", "x", "7.4", "--repeat", "daily"]),
            "usage: rmd ddl add [-h] [--repeat {weekly,biweekly,monthly}] event date\nrmd ddl add: error: argument --repeat: invalid choice: 'daily' (choose from weekly, biweekly, monthly)"
        );
        assert_eq!(
            error_message(&["history", "abc"]),
            "usage: rmd history [-h] [count]\nrmd history: error: argument count: invalid int value: 'abc'"
        );
        assert_eq!(
            error_message(&["rm"]),
            "usage: rmd rm [-h] tasks [tasks ...]\nrmd rm: error: the following arguments are required: tasks"
        );
        assert_eq!(
            error_message(&["completion", "ksh"]),
            "usage: rmd completion [-h] [{bash,zsh,tcsh,fish}]\nrmd completion: error: argument shell: invalid choice: 'ksh' (choose from bash, zsh, tcsh, fish)"
        );
        assert_eq!(
            error_message(&["mode", "x"]),
            "usage: rmd mode [-h] [{j,p}]\nrmd mode: error: argument mode: invalid choice: 'x' (choose from j, p)"
        );
        assert_eq!(
            error_message(&["report"]),
            "usage: rmd report [-h] [-d DATE] [--days DAYS] {weekly,monthly}\nrmd report: error: the following arguments are required: type"
        );
        assert_eq!(
            error_message(&["ddl", "foo"]),
            "usage: rmd ddl [-h] {add,rm} ...\nrmd ddl: error: argument ddl_command: invalid choice: 'foo' (choose from add, rm)"
        );
        assert_eq!(
            error_message(&["switch", "1", "2"]),
            "usage: rmd [-h] <command> ...\nrmd: error: unrecognized arguments: 2"
        );
    }

    #[test]
    fn help_reads_like_argparse() {
        let mut env = TestEnv::new();
        env.set_var("COLUMNS", "80");
        let add = help_text(&["add"]);
        assert_eq!(
            add,
            "usage: rmd add [-h] [task] [PRIORITY] [task_type] [postpone]\n\nAdd a task to your task list with a priority level.\n\npositional arguments:\n  task        Description of the task (e.g., 'biology homework')\n  PRIORITY    Priority level 1-10 (higher = more important)\n  task_type   Task type ID (e.g., 1 for 'read papers')\n  postpone    Optional days to postpone daily urgent alarms (e.g., 1 for\n              tomorrow, 2 for two days later)\n\noptions:\n  -h, --help  show this help message and exit\n"
        );
        assert_eq!(
            help_text(&["ddl", "add"]),
            "usage: rmd ddl add [-h] [--repeat {weekly,biweekly,monthly}] event date\n\nAdd a deadline with name and due date.\n\npositional arguments:\n  event                 Name of the event (e.g., 'homework2')\n  date                  Due date in M.D or MM.DD format (e.g., '7.4' for July\n                        4th)\n\noptions:\n  -h, --help            show this help message and exit\n  --repeat {weekly,biweekly,monthly}\n                        Repeat the deadline weekly, every two weeks, or\n                        monthly\n"
        );
        assert_eq!(
            help_text(&["ddl"]),
            "usage: rmd ddl [-h] {add,rm} ...\n\nDeadline management. Without subcommand, shows all deadlines.\n\noptions:\n  -h, --help  show this help message and exit\n\nDeadline commands:\n  {add,rm}\n    add       Add a new deadline event\n    rm        Delete one or more deadline events\n"
        );
        assert_eq!(
            help_text(&["sync"]),
            "usage: rmd sync [-h] [--json | --apply FILE | --clear]\n\nShow today's focus blocks and their planned tasks. AI agents with the Schedule\nEverything plugin read --json and save plans with --apply.\n\noptions:\n  -h, --help    show this help message and exit\n  --json        Print today's focus slots, tasks, deadlines, and plan as JSON\n  --apply FILE  Save a plan for today from a JSON file (\"-\" reads standard\n                input)\n  --clear       Remove today's plan\n"
        );
        assert_eq!(
            help_text(&["ls"]),
            "usage: rmd ls [-h] [-t TASK_TYPE]\n\nDisplay your task list sorted by priority.\n\noptions:\n  -h, --help            show this help message and exit\n  -t TASK_TYPE, --type TASK_TYPE\n                        Only show tasks of this type. Accepts a task type ID\n                        (e.g. 1) or name (e.g. 'coding').\n"
        );
        assert_eq!(
            help_text(&["web"]),
            "usage: rmd web [-h] [--no-open] [--port PORT] [--if-running] [--phone] [--no-phone]\n\noptions:\n  -h, --help    show this help message and exit\n  --no-open     Start the local server without opening a browser\n  --port PORT   Local port for a new server (default: automatically chosen)\n  --if-running  Only restart a running server that is on outdated code; never\n                start one or open a tab\n  --phone       Let your phone use the workspace over Wi-Fi or Tailscale, and\n                show its pairing code\n  --no-phone    Turn phone access off and sign every phone out\n"
        );
        let root = strip_ansi(&help_text(&[]));
        assert!(root.starts_with("usage: rmd [-h] <command> ...\n\nrmd CLI - Manage your schedule management system\n\noptions:\n  -h, --help    show this help message and exit\n\nAvailable commands:\n  <command>\n    web         Open the local browser workspace (reuses an existing tab)\n"));
        assert!(root.contains(
            "    switch      Switch to a different versioned config set and reload the\n                service\n"
        ));
        assert!(!root.contains("__api") && !root.contains("daemon"));
        assert!(root.contains(&format!("Configuration root: {}", crate::paths::resolve(env.root()).display())));
        assert!(root.ends_with("/user_config_0\n    \n"));
    }

    #[test]
    fn root_help_follows_the_language() {
        let mut env = TestEnv::new();
        env.set_var("REMINDER_LANG", "zh");
        let root = strip_ansi(&help_text(&[]));
        assert!(root.contains("\n\nrmd 命令行工具 - "));
        assert!(root.contains("\n可用命令:\n  <command>\n"));
        assert!(help_text(&["ddl"]).contains("\nDeadline commands:\n"));
    }

    #[test]
    fn bare_rmd_prints_help_and_exits_1() {
        let _env = TestEnv::new();
        assert_eq!(run(vec!["rmd".into()]), 1);
        assert_eq!(run(vec!["rmd".into(), "--help".into()]), 0);
        assert_eq!(run(vec!["rmd".into(), "frobnicate".into()]), 2);
        assert_eq!(run(vec!["rmd".into(), "add".into(), "x".into(), "11".into()]), 2);
    }
}
