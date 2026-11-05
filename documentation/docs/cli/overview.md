---
sidebar_position: 1
---

# CLI Overview

Schedule Everything provides a comprehensive command-line interface (CLI) for managing your schedules, tasks, and system configuration. The CLI tool is automatically installed when you run the installation script.

## Accessing the CLI

After installation, the `rmd` command should be available in your terminal. `reminder` still works as a compatibility alias. If `rmd` is not on your PATH, ensure you've added the following to your shell profile:

```bash
export PATH="$HOME/SCHEDULE_MANAGEMENT:$PATH"
export REMINDER_CONFIG_DIR="$HOME/.schedule_management/config"
alias rmd="$HOME/SCHEDULE_MANAGEMENT/rmd"
```

Then reload your shell:
```bash
source ~/.zshrc  # or source ~/.bash_profile
```

## In the terminal

<FigureRow columns={2}>
  <Figure src="img/rmd_add.gif" variant="plain" alt="Terminal recording: rmd add adds a task with priority 8, then rmd ls shows the task table" caption="rmd add, then rmd ls" />
  <Figure src="img/rmd_view.gif" variant="plain" alt="Terminal recording: rmd status and rmd status -v, then rmd view opens the schedule PDF with odd and even week statistics" caption="rmd status, then rmd view" />
</FigureRow>

## Command Categories

The CLI commands are organized into these main categories:

### Schedule Management
Commands for managing your schedule and service:
- [`rmd update`](schedule-management.md#update) - Reload config and restart service
- [`rmd switch`](schedule-management.md#switch) - Change the active `user_config_n` snapshot and reload the service
- [`rmd check`](schedule-management.md#check) - Validate a config directory (the active one by default)
- [`rmd sync`](schedule-management.md#sync) - Show today's pomodoro/potato plan; print today's context as JSON for AI agents, save a plan, or clear it
- [`rmd view`](schedule-management.md#view) - Generate schedule visualization
- [`rmd status`](schedule-management.md#status) - Show upcoming events, including planned task titles when present
- [`rmd stop`](schedule-management.md#stop) - Stop the alarm service
- `rmd mode [j|p]` - Show or switch between J mode and P mode
- `rmd web` - Open the [browser workspace](../browser.md)

Building a schedule and planning a day with AI happen in your own coding agent.
See [AI agent plugins](../ai-agents.md).

### Task Management
Commands for managing your task list:
- [`rmd add`](task-management.md#add) - Add or update tasks
- [`rmd rm`](task-management.md#rm) - Remove tasks
- [`rmd ls`](task-management.md#ls) - List all tasks
- [`rmd history`](task-management.md#history) - Show recent completed task activities

### Deadline Management
Commands for managing event deadlines:
- [`rmd ddl add`](deadline-management.md#ddl-add) - Add or update deadlines
- [`rmd ddl rm`](deadline-management.md#ddl-rm) - Remove deadlines
- [`rmd ddl`](deadline-management.md#ddl) - List all deadlines with urgency status

### Habits
Commands for tracking daily habits:
- [`rmd track [ids...]`](task-management.md#track) - Log completed habits for today (opens a prompt if no IDs given)

### Reports
Commands for generating productivity reports:
- [`rmd report <type>`](schedule-management.md#report) - Generate weekly or monthly PDF reports

### System Commands
- [`rmd settings`](schedule-management.md#settings) - Interactive TUI for editing `settings.toml` with contextual actions (↑/↓ to move, Enter to open/apply, Esc to back/cancel, Space to toggle, `s` to save, `q` to quit)
- [`rmd edit <file>`](schedule-management.md#edit) - Open configuration files (`settings`, `odd`, `even`, `deadlines`, `habits`) in your default editor
- `rmd --help` - Show help information

## Configuration Directory

By default, the CLI looks for its config root in:
- `~/.schedule_management/config/` (macOS/Linux)

Inside that root, schedule files live under versioned `user_config_n`
directories and shared task data stays under `tasks/`.

You can override this by setting the `REMINDER_CONFIG_DIR` environment variable.

## Error Handling

The CLI provides clear error messages for common issues:

```bash
# Configuration file not found
Error: settings.toml not found in /Users/username/.schedule_management/config/

# Invalid TOML syntax
Error: Invalid TOML syntax in odd_weeks.toml: Expected '=' at line 5

# Service not running
Warning: Schedule management service is not running. Run 'rmd update' to start it.
```

## Exit Codes

The CLI returns standard exit codes:
- `0` - Success
- `1` - General error
- `2` - Configuration error
- `3` - Service error

## Examples

### Basic Usage
```bash
# Check service status
rmd status

# Add a high-priority task
rmd add "Complete project proposal" 9

# Show today's focus blocks and their planned tasks
rmd sync

# Validate the active configuration
rmd check

# View your schedule
rmd view

# Stop the service
rmd stop
```

### Advanced Usage
```bash
# Update with custom config directory
REMINDER_CONFIG_DIR=/custom/path/config rmd update

# Switch to config snapshot 2 under a custom config root
REMINDER_CONFIG_DIR=/custom/path/config rmd switch 2

# List tasks with verbose output (if supported)
rmd status -v

# Remove multiple tasks
rmd rm "Task 1" "Task 2" "Task 3"
```

## Integration with Other Tools

The CLI can be integrated with other tools and scripts:

```bash
# Use in shell scripts
#!/bin/bash
if rmd status | grep -q "No upcoming events"; then
    echo "Schedule is clear"
fi

# Pipe to other commands
rmd ls | grep "urgent" | wc -l

# Use with cron for automated tasks
0 9 * * * /Users/username/SCHEDULE_MANAGEMENT/rmd status >> ~/schedule.log
```

## Troubleshooting CLI Issues

### Command Not Found
If you get `command not found: rmd`:
1. Check that the installation completed successfully
2. Verify the PATH includes the installation directory
3. Ensure your shell profile is sourced

### Permission Denied
If you get `Permission denied` errors:
```bash
chmod +x ~/SCHEDULE_MANAGEMENT/rmd
```

### Configuration Errors
If commands fail with configuration errors:
1. Run `rmd check` to see missing files, TOML syntax errors, unknown blocks, and likely label typos
2. Ensure file permissions are correct

## Next Steps

- Learn about [Schedule Management Commands](schedule-management.md)
- Explore [Task Management Commands](task-management.md) and [Deadline Management Commands](deadline-management.md)
- See [Configuration](../configuration/overview.md) and [Settings](../configuration/settings.md) for detailed syntax


## Browser workspace

Run `rmd web` to manage the same local data through the browser. The task board
groups all tasks by type and retains importance and reminder details. Existing
CLI syntax and task numbering are unchanged. See [Browser workspace](../browser.md).
