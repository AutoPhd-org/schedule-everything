//! `rmd` argument parsing and dispatch (port of `cli.py`). OWNER: CLI agent.
//!
//! Hidden subcommands used internally (not shown in help):
//! - `rmd __api [JSON]`            -> `web::api::cli_main` (JSON request from argv or stdin)
//! - `rmd __web-server --root R --port P` -> `web::server::run`
//! - `rmd daemon`                  -> `reminder::run_service` (the background reminder service)
//! - `rmd __setup-config [ARGS..]` -> `install_config_wizard::run`

/// Parse `std::env::args()` and run the command; returns the exit code.
pub fn main() -> i32 {
    todo!("CLI agent: port cli.py with clap")
}
