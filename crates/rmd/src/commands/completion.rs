//! Shell completion scripts (`rmd completion [SHELL]`; port of
//! `commands/completion.py`, which used shtab). OWNER: CLI agent.
//!
//! bash, zsh and fish scripts come from clap_complete; tcsh gets a small
//! `complete` rule covering the subcommands and their fixed choices.

use std::io::Write;

/// Shells `rmd completion` can target (the Python version had bash, zsh, tcsh).
pub const SUPPORTED_COMPLETION_SHELLS: [&str; 4] = ["bash", "zsh", "tcsh", "fish"];

/// The `rmd` command tree without the hidden internal subcommands.
fn public_command() -> clap::Command {
    let full = crate::cli::build_command();
    full.get_subcommands().filter(|s| !s.is_hide_set()).fold(
        clap::Command::new("rmd").bin_name("rmd").about(full.get_about().cloned().unwrap_or_default()),
        |cmd, sub| cmd.subcommand(sub.clone()),
    )
}

/// The completion script for `shell`.
pub fn completion_script(shell: &str) -> Result<String, String> {
    let mut cmd = public_command();
    let generator = match shell {
        "bash" => clap_complete::Shell::Bash,
        "zsh" => clap_complete::Shell::Zsh,
        "fish" => clap_complete::Shell::Fish,
        "tcsh" => return Ok(tcsh_script(&cmd)),
        other => return Err(format!("unsupported shell: {other}")),
    };
    let mut buffer = Vec::new();
    clap_complete::generate(generator, &mut cmd, "rmd", &mut buffer);
    String::from_utf8(buffer).map_err(|e| e.to_string())
}

/// `complete rmd ...` rules: subcommand names, then each (nested)
/// subcommand's subcommands and fixed choices, and options' choices.
fn tcsh_script(cmd: &clap::Command) -> String {
    fn visible(c: &clap::Command) -> Vec<&clap::Command> {
        c.get_subcommands().filter(|s| !s.is_hide_set()).collect()
    }
    fn add_rules(cmd: &clap::Command, rules: &mut Vec<String>) {
        for sub in visible(cmd) {
            let mut words: Vec<String> = visible(sub).iter().map(|s| s.get_name().to_string()).collect();
            for arg in sub.get_positionals() {
                words.extend(arg.get_possible_values().iter().map(|v| v.get_name().to_string()));
            }
            if !words.is_empty() {
                rules.push(format!("'n/{}/({})/'", sub.get_name(), words.join(" ")));
            }
            for arg in sub.get_arguments().filter(|a| a.get_long().is_some() && a.get_action().takes_values()) {
                let values: Vec<String> = arg.get_possible_values().iter().map(|v| v.get_name().to_string()).collect();
                if !values.is_empty() {
                    rules.push(format!("'n/--{}/({})/'", arg.get_long().unwrap_or_default(), values.join(" ")));
                }
            }
            add_rules(sub, rules);
        }
    }
    let names: Vec<&str> = visible(cmd).iter().map(|s| s.get_name()).collect();
    let mut rules = vec![format!("'p/1/({})/'", names.join(" "))];
    add_rules(cmd, &mut rules);
    format!("# tcsh completion for rmd\ncomplete rmd \\\n    {}\n", rules.join(" \\\n    "))
}

/// `rmd completion [bash|zsh|tcsh|fish]`: print the script.
pub fn completion_command(shell: &str) -> i32 {
    match completion_script(shell) {
        Ok(script) => {
            let mut stdout = std::io::stdout().lock();
            let _ = stdout.write_all(script.as_bytes());
            let _ = stdout.flush();
            0
        }
        Err(e) => {
            println!("❌ Error generating completion script: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestEnv;

    #[test]
    fn bash_zsh_and_fish_scripts_cover_the_commands() {
        let _env = TestEnv::new();
        let bash = completion_script("bash").unwrap();
        assert!(bash.contains("complete "));
        assert!(bash.contains("rmd"));
        for shell in ["bash", "zsh", "fish"] {
            let script = completion_script(shell).unwrap();
            for command in ["add", "ddl", "sync", "completion"] {
                assert!(script.contains(command), "{shell} script lacks {command}");
            }
            assert!(
                !script.contains("__api") && !script.contains("__web-server"),
                "{shell} script lists hidden commands"
            );
        }
    }

    #[test]
    fn tcsh_script_lists_subcommands_and_choices() {
        let _env = TestEnv::new();
        let script = completion_script("tcsh").unwrap();
        assert!(script.starts_with(
            "# tcsh completion for rmd\ncomplete rmd \\\n    'p/1/(web add rm cancel drop ls history ddl track status"
        ));
        assert!(script.contains("'n/ddl/(add rm)/'"));
        assert!(script.contains("'n/completion/(bash zsh tcsh fish)/'"));
        assert!(script.contains("'n/mode/(j p)/'"));
        assert!(script.contains("'n/--repeat/(weekly biweekly monthly)/'"));
        assert!(!script.contains("true false"));
        assert!(!script.contains("daemon"));
        assert!(completion_script("ksh").is_err());
    }
}
