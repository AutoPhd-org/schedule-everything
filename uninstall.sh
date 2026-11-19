#!/bin/bash

# Removes what install.sh installed: the reminder service, ~/SCHEDULE_MANAGEMENT,
# the shell completion lines, and the LaunchX registration. Your schedules,
# tasks and history in ~/.schedule_management are kept.
#
#   ./uninstall.sh
#
# `lx uninstall schedule-everything` runs this for you when LaunchX installed
# Schedule Everything.

set -euo pipefail

INSTALL_DIR="$HOME/SCHEDULE_MANAGEMENT"
RMD_BIN="$INSTALL_DIR/rmd"
CONFIG_ROOT="$HOME/.schedule_management"
LAUNCH_AGENT_NAME="com.sergiudm.schedule.management.reminder"
LAUNCH_AGENT_PLIST="$HOME/Library/LaunchAgents/${LAUNCH_AGENT_NAME}.plist"
SYSTEMD_SERVICE_NAME="schedule-management.service"
SYSTEMD_SERVICE_PATH="$HOME/.config/systemd/user/${SYSTEMD_SERVICE_NAME}"

log() { printf '%s\n' "$1"; }

# The reminder service.
if [[ "$OSTYPE" == "darwin"* ]]; then
    if [[ -f "$LAUNCH_AGENT_PLIST" ]]; then
        launchctl unload "$LAUNCH_AGENT_PLIST" 2>/dev/null || true
        rm -f "$LAUNCH_AGENT_PLIST"
        log "Removed the reminder service ($LAUNCH_AGENT_NAME)"
    fi
elif [[ -f "$SYSTEMD_SERVICE_PATH" ]]; then
    systemctl --user stop "$SYSTEMD_SERVICE_NAME" 2>/dev/null || true
    systemctl --user disable "$SYSTEMD_SERVICE_NAME" 2>/dev/null || true
    rm -f "$SYSTEMD_SERVICE_PATH"
    systemctl --user daemon-reload 2>/dev/null || true
    log "Removed the reminder service ($SYSTEMD_SERVICE_NAME)"
fi
pkill -f "$RMD_BIN daemon" 2>/dev/null || true
pkill -f "$RMD_BIN __web-server" 2>/dev/null || true

# LaunchX, when it runs the workspace from this installation. `lx uninstall`
# also does this; running it here covers an uninstall without LaunchX's help.
lx="$(command -v lx || true)"
if [[ -n "$lx" ]]; then
    for entry in "${LAUNCHX_HOME:-$HOME/.launchx}"/services/*.toml; do
        if [[ -e "$entry" && "$(readlink "$entry" || true)" == "$INSTALL_DIR/launchx.toml" ]]; then
            "$lx" rm rmd >/dev/null 2>&1 && log "Unregistered rmd from LaunchX" || true
        fi
    done
fi

# Shell completion lines install.sh added.
for rc in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile"; do
    [[ -f "$rc" ]] || continue
    for shell in zsh bash; do
        line="eval \"\$(\"$RMD_BIN\" completion $shell)\""
        if grep -qxF "$line" "$rc"; then
            cp "$rc" "$rc.before-rmd-uninstall"
            grep -vxF "$line" "$rc.before-rmd-uninstall" > "$rc" || true
            log "Removed rmd completion from $rc (backup: $rc.before-rmd-uninstall)"
        fi
    done
done

# The installation.
if [[ -d "$INSTALL_DIR" ]]; then
    rm -rf "$INSTALL_DIR"
    log "Removed $INSTALL_DIR"
fi

log "Schedule Everything is uninstalled. Your schedules, tasks and history are still in $CONFIG_ROOT."
shopt -s nullglob
backups=("$INSTALL_DIR".backup.*)
if (( ${#backups[@]} )); then
    log "Earlier installations backed up by install.sh are still there: ${backups[*]}"
fi
