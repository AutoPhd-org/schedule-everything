//! Schedule Everything: a local-first weekly routine, task list, and
//! focus-block reminder app. The crate replaced the original Python
//! implementation and keeps its behaviour (see `PORTING.md`).

pub mod clock;
pub mod py;
#[cfg(test)]
pub mod test_support;

// Core data and configuration.
pub mod config;
pub mod config_check;
pub mod data;
pub mod i18n;
mod i18n_table;
mod i18n_table_es;
mod i18n_table_ko;
pub mod paths;
pub mod recurrence;
pub mod synced_schedule;
pub mod time_utils;
pub mod toml_writer;

// Desktop integration.
pub mod dialogs;
pub mod platform;

// Features.
pub mod cli;
pub mod commands;
pub mod conferences;
pub mod install_config_wizard;
pub mod popups;
pub mod reminder;
pub mod report;
pub mod runner;
pub mod suggestions;
pub mod ui;
pub mod visualizer;
pub mod web;

/// Run the `rmd` command line with the process arguments; returns the exit code.
pub fn run() -> i32 {
    cli::main()
}
