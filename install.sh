#!/bin/bash

# Installs Schedule Everything on macOS or Linux: the `rmd` binary, the config
# root, and the background reminder service. No Python is needed.
#
#   curl -fsSL https://raw.githubusercontent.com/AutoPhd-org/schedule-everything/main/install.sh | bash
#   ./install.sh [-y|--yes]
#
# `rmd` is built with cargo from the source (this checkout, or a copy of the
# repository the script downloads), so Rust is required: https://rustup.rs.
# Existing configuration is never overwritten.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_URL="https://github.com/AutoPhd-org/schedule-everything"
TEMP_DIR=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script configuration (paths and service names are unchanged from the
# Python-based releases so upgrades keep PATH entries and service settings).
SCRIPT_NAME="Schedule Everything Installer"
INSTALL_DIR="$HOME/SCHEDULE_MANAGEMENT"
RMD_BIN="$INSTALL_DIR/rmd"
INSTALL_CONFIG_DIR="$HOME/.schedule_management/config"
INSTALL_ACTIVE_CONFIG_ID="0"
INSTALL_ACTIVE_CONFIG_DIR="$INSTALL_CONFIG_DIR/user_config_${INSTALL_ACTIVE_CONFIG_ID}"
INSTALL_ACTIVE_CONFIG_FILE="$INSTALL_CONFIG_DIR/.active_config"
LAUNCH_AGENT_NAME="com.sergiudm.schedule.management.reminder"
LAUNCH_AGENT_PLIST="$HOME/Library/LaunchAgents/${LAUNCH_AGENT_NAME}.plist"
SYSTEMD_SERVICE_NAME="schedule-management.service"
SYSTEMD_SERVICE_PATH="$HOME/.config/systemd/user/${SYSTEMD_SERVICE_NAME}"
SERVICE_WAS_RUNNING="false"

# Detect OS
detect_os() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    elif [[ "$OSTYPE" == "linux-gnu"* || "$OSTYPE" == "linux-musl"* ]]; then
        echo "linux"
    else
        echo "unknown"
    fi
}

OS_TYPE=$(detect_os)

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

has_legacy_config_files() {
    local legacy_files=(
        "ddl.json"
        "even_weeks.toml"
        "habits.toml"
        "habits_template.toml"
        "odd_weeks.toml"
        "profile.md"
        "settings.toml"
        "settings_template.toml"
        "week_schedule_template.toml"
    )
    local file_name
    for file_name in "${legacy_files[@]}"; do
        if [[ -e "$INSTALL_CONFIG_DIR/$file_name" ]]; then
            return 0
        fi
    done
    return 1
}

has_versioned_config_dirs() {
    compgen -G "$INSTALL_CONFIG_DIR/user_config_*" > /dev/null
}

resolve_install_target_config_dir() {
    if [[ -f "$INSTALL_ACTIVE_CONFIG_FILE" ]]; then
        local active_id
        active_id="$(tr -d '[:space:]' < "$INSTALL_ACTIVE_CONFIG_FILE")"
        if [[ "$active_id" =~ ^[0-9]+$ ]] && [[ -d "$INSTALL_CONFIG_DIR/user_config_${active_id}" ]]; then
            echo "$INSTALL_CONFIG_DIR/user_config_${active_id}"
            return 0
        fi
    fi

    if has_legacy_config_files; then
        echo "$INSTALL_CONFIG_DIR"
        return 0
    fi

    echo "$INSTALL_ACTIVE_CONFIG_DIR"
}

scaffold_versioned_config_root() {
    log_info "Scaffolding versioned config layout in $INSTALL_CONFIG_DIR"

    mkdir -p "$INSTALL_ACTIVE_CONFIG_DIR"
    mkdir -p "$INSTALL_CONFIG_DIR/tasks"
    printf '%s\n' "$INSTALL_ACTIVE_CONFIG_ID" > "$INSTALL_ACTIVE_CONFIG_FILE"

    if [[ -d "$SCRIPT_DIR/config" ]]; then
        local source_path
        for source_path in "$SCRIPT_DIR"/config/*; do
            local base_name
            base_name="$(basename "$source_path")"
            if [[ -f "$source_path" ]]; then
                cp "$source_path" "$INSTALL_ACTIVE_CONFIG_DIR/$base_name"
            fi
        done
        log_success "Initial config files copied to $INSTALL_ACTIVE_CONFIG_DIR"
    else
        log_warning "config/ directory not found in project. Created empty versioned config layout at $INSTALL_CONFIG_DIR."
    fi

    log_success "Active config marker created at $INSTALL_ACTIVE_CONFIG_FILE"
}

# Check OS compatibility
check_os() {
    if [[ "$OS_TYPE" == "unknown" ]]; then
        log_error "Unsupported operating system: $OSTYPE"
        log_info "Supported systems: macOS (darwin) and Linux"
        exit 1
    fi
    log_success "$OS_TYPE detected"
}

# Stop a running reminder service (from a Python-based or an earlier Rust
# release) so the new binary takes over; it is started again at the end.
stop_existing_service() {
    if [[ "$OS_TYPE" == "macos" ]]; then
        if launchctl list 2>/dev/null | grep -q "$LAUNCH_AGENT_NAME"; then
            SERVICE_WAS_RUNNING="true"
            log_info "Stopping the running reminder service..."
            launchctl unload "$LAUNCH_AGENT_PLIST" 2>/dev/null || true
        fi
    elif [[ "$OS_TYPE" == "linux" ]]; then
        if systemctl --user is-active --quiet "$SYSTEMD_SERVICE_NAME" 2>/dev/null; then
            SERVICE_WAS_RUNNING="true"
            log_info "Stopping the running reminder service..."
            systemctl --user stop "$SYSTEMD_SERVICE_NAME" 2>/dev/null || true
        fi
    fi
    pkill -f "python.*reminder_macos.py" 2>/dev/null || true
    pkill -f "$RMD_BIN daemon" 2>/dev/null || true
}

# Prepare the installation directory, keeping a backup of an earlier one.
setup_install_dir() {
    log_info "Setting up $INSTALL_DIR..."

    if [[ -d "$INSTALL_DIR" ]]; then
        log_warning "Installation directory exists at $INSTALL_DIR"
        if [[ "$AUTO_YES" == "true" ]]; then
            backup_path="${INSTALL_DIR}.backup.$(date +%Y%m%d_%H%M%S)"
            log_info "Backing up existing installation to $backup_path (non-interactive)"
            mv "$INSTALL_DIR" "$backup_path"
        else
            while true; do
                read -r -p "Replace existing installation with new files? [y/N]: " replace_existing
                case "$replace_existing" in
                    [yY]|[yY][eE][sS])
                        log_info "Replacing existing installation directory..."
                        rm -rf "$INSTALL_DIR"
                        break
                        ;;
                    [nN]|[nN][oO]|"")
                        backup_path="${INSTALL_DIR}.backup.$(date +%Y%m%d_%H%M%S)"
                        log_info "Backing up existing installation to $backup_path"
                        mv "$INSTALL_DIR" "$backup_path"
                        break
                        ;;
                    *)
                        log_warning "Invalid input. Please answer with y or n."
                        ;;
                esac
            done
        fi
    fi

    mkdir -p "$INSTALL_DIR/logs"

    # Agent plugins (Claude Code, Codex, Antigravity) for building and planning schedules.
    if [[ -d "$SCRIPT_DIR/plugins" ]]; then
        cp -r "$SCRIPT_DIR/plugins" "$INSTALL_DIR/"
        log_success "plugins/ directory copied"
    fi
    if [[ -f "$SCRIPT_DIR/LICENSE" ]]; then
        cp "$SCRIPT_DIR/LICENSE" "$INSTALL_DIR/"
    fi

    # Config directory handling
    mkdir -p "$(dirname "$INSTALL_CONFIG_DIR")"
    if [[ -d "$INSTALL_CONFIG_DIR" ]]; then
        # If config root already exists, preserve it unless it is still empty.
        if has_legacy_config_files || has_versioned_config_dirs || [[ -f "$INSTALL_ACTIVE_CONFIG_FILE" ]]; then
            log_warning "Config directory $INSTALL_CONFIG_DIR already exists; skipping config copy."
        else
            scaffold_versioned_config_root
        fi
    else
        mkdir -p "$INSTALL_CONFIG_DIR"
        scaffold_versioned_config_root
    fi

    log_success "Installation directory ready"
}

# Check for cargo before anything is changed, so a missing toolchain never
# leaves the service stopped or the installation half replaced.
require_cargo() {
    if ! command -v cargo &>/dev/null; then
        log_error "Building rmd needs cargo, which was not found."
        log_info "Install Rust from https://rustup.rs (it provides cargo), then run this installer again."
        exit 1
    fi
}

# Build rmd with cargo from the source in $SCRIPT_DIR. This runs before the
# service is stopped, so reminders keep working during the build.
build_rmd() {
    if [[ ! -f "$SCRIPT_DIR/Cargo.toml" || ! -d "$SCRIPT_DIR/crates/rmd" ]]; then
        log_error "No Rust source found in $SCRIPT_DIR."
        exit 1
    fi
    log_info "Building rmd with cargo (the first build takes a few minutes)..."
    (cd "$SCRIPT_DIR" && cargo build --release --locked -p rmd)
    BUILT_RMD="${CARGO_TARGET_DIR:-$SCRIPT_DIR/target}/release/rmd"
    log_success "Built rmd"
}

install_binary() {
    install -m 755 "$BUILT_RMD" "$RMD_BIN"
    ln -sf "rmd" "$INSTALL_DIR/reminder"
    log_success "Installed rmd in $INSTALL_DIR"
}

# Validate and complete missing configuration values
configure_configs() {
    log_info "Checking configuration files and required values..."

    local template_dir="$SCRIPT_DIR/config/user_config_0"
    local target_config_dir
    target_config_dir="$(resolve_install_target_config_dir)"

    local wizard_invocation=("$RMD_BIN" __setup-config --config-dir "$target_config_dir")
    if [[ -d "$template_dir" ]]; then
        wizard_invocation+=(--template-dir "$template_dir")
    else
        log_warning "Template directory not found at $template_dir; using config directory as template source."
    fi
    if [[ "$AUTO_YES" == "true" ]]; then
        wizard_invocation+=(--yes)
    fi

    REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR" "${wizard_invocation[@]}"

    log_info "Validated active config directory: $target_config_dir"
    log_success "Configuration checks complete"
}

# Create systemd service (Linux only)
create_systemd_service() {
    if [[ "$OS_TYPE" != "linux" ]]; then
        return 0
    fi

    log_info "Creating systemd service..."

    mkdir -p "$HOME/.config/systemd/user"

    cat > "$SYSTEMD_SERVICE_PATH" << EOF
[Unit]
Description=Schedule Management Reminder
After=graphical-session.target

[Service]
Type=simple
ExecStart=$RMD_BIN daemon
Restart=always
RestartSec=10
Environment=DISPLAY=:0
Environment=REMINDER_CONFIG_DIR=$INSTALL_CONFIG_DIR
StandardOutput=append:$INSTALL_DIR/logs/schedule_management.out
StandardError=append:$INSTALL_DIR/logs/schedule_management.err

[Install]
WantedBy=default.target
EOF

    systemctl --user daemon-reload
    systemctl --user enable "$SYSTEMD_SERVICE_NAME"
    log_success "systemd service created and enabled"
}

# Create LaunchAgent plist (macOS only)
create_launch_agent() {
    if [[ "$OS_TYPE" != "macos" ]]; then
        return 0
    fi

    log_info "Creating LaunchAgent..."

    mkdir -p "$HOME/Library/LaunchAgents"

    cat > "$LAUNCH_AGENT_PLIST" << EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${LAUNCH_AGENT_NAME}</string>
    <key>ProgramArguments</key>
    <array>
        <string>$RMD_BIN</string>
        <string>daemon</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$INSTALL_DIR/logs/schedule_management.out</string>
    <key>StandardErrorPath</key>
    <string>$INSTALL_DIR/logs/schedule_management.err</string>
    <key>WorkingDirectory</key>
    <string>$INSTALL_DIR</string>
    <key>EnvironmentVariables</key>
    <dict>
        <key>REMINDER_CONFIG_DIR</key>
        <string>$INSTALL_CONFIG_DIR</string>
    </dict>
</dict>
</plist>
EOF

    log_success "LaunchAgent created at $LAUNCH_AGENT_PLIST"
}

# Start the service again if it was running before the upgrade.
restart_service_if_needed() {
    if [[ "$SERVICE_WAS_RUNNING" != "true" ]]; then
        return 0
    fi
    log_info "Starting the reminder service on the new binary..."
    if [[ "$OS_TYPE" == "macos" ]]; then
        launchctl load "$LAUNCH_AGENT_PLIST" || log_warning "Could not load $LAUNCH_AGENT_PLIST; run: launchctl load $LAUNCH_AGENT_PLIST"
    elif [[ "$OS_TYPE" == "linux" ]]; then
        systemctl --user start "$SYSTEMD_SERVICE_NAME" || log_warning "Could not start $SYSTEMD_SERVICE_NAME"
    fi
}

# A browser workspace left running from the previous version would pair the
# new frontend with old code, so restart it on the new binary.
refresh_browser_workspace() {
    REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR" "$RMD_BIN" web --if-running \
        || log_warning "Could not restart the running browser workspace. Run 'rmd web' to restart it."
}

# Request permissions (platform-specific)
request_permissions() {
    if [[ "$OS_TYPE" == "macos" ]]; then
        log_info "The app will request Accessibility & Notification permissions on first run."
    elif [[ "$OS_TYPE" == "linux" ]]; then
        log_info "Dialogs use zenity or kdialog (or notify-send); sounds use paplay, aplay or play."
        log_info "Install the ones your desktop lacks, for example: sudo apt install zenity libnotify-bin pulseaudio-utils"
    fi
}

# Create convenience scripts (platform-specific)
create_scripts() {
    log_info "Creating convenience scripts..."

    cat > "$INSTALL_DIR/start_reminders.sh" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
exec "$RMD_BIN" daemon
EOF

    if [[ "$OS_TYPE" == "macos" ]]; then
        cat > "$INSTALL_DIR/stop_reminders.sh" << EOF
#!/bin/bash
launchctl unload "\$HOME/Library/LaunchAgents/${LAUNCH_AGENT_NAME}.plist" 2>/dev/null || true
pkill -f "$RMD_BIN daemon" 2>/dev/null || true
pkill -f "python.*reminder_macos.py" 2>/dev/null || true
EOF
        cat > "$INSTALL_DIR/restart_reminders.sh" << EOF
#!/bin/bash
"$INSTALL_DIR/stop_reminders.sh"
sleep 2
launchctl load "\$HOME/Library/LaunchAgents/${LAUNCH_AGENT_NAME}.plist"
EOF
    elif [[ "$OS_TYPE" == "linux" ]]; then
        cat > "$INSTALL_DIR/stop_reminders.sh" << EOF
#!/bin/bash
systemctl --user stop ${SYSTEMD_SERVICE_NAME} 2>/dev/null || true
systemctl --user disable ${SYSTEMD_SERVICE_NAME} 2>/dev/null || true
pkill -f "$RMD_BIN daemon" 2>/dev/null || true
pkill -f "python.*reminder_macos.py" 2>/dev/null || true
EOF
        cat > "$INSTALL_DIR/restart_reminders.sh" << EOF
#!/bin/bash
"$INSTALL_DIR/stop_reminders.sh"
sleep 2
systemctl --user enable ${SYSTEMD_SERVICE_NAME}
systemctl --user start ${SYSTEMD_SERVICE_NAME}
EOF
    fi

    cat > "$INSTALL_DIR/visualize_schedule.sh" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
exec "$RMD_BIN" view
EOF

    chmod +x "$INSTALL_DIR"/*.sh
    log_success "Convenience scripts created"
}

# Test installation
test_installation() {
    log_info "Testing installation..."

    if "$RMD_BIN" --help &>/dev/null; then
        log_success "CLI 'rmd' works correctly"
    else
        log_error "CLI 'rmd' failed"
        exit 1
    fi

    if REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR" "$RMD_BIN" check &>/dev/null; then
        log_success "Configuration is valid"
    else
        log_warning "'rmd check' reported problems; run it to see them."
    fi
}

# Setup shell autocompletion
setup_autocompletion() {
    log_info "Setting up shell autocompletion..."

    local zsh_rc="$HOME/.zshrc"
    local bash_rc="$HOME/.bashrc"
    if [[ "$OS_TYPE" == "macos" ]]; then
        bash_rc="$HOME/.bash_profile"
    fi

    if [[ -f "$zsh_rc" ]]; then
        if ! grep -q "rmd\"\{0,1\} completion zsh" "$zsh_rc"; then
            echo "eval \"\$(\"$RMD_BIN\" completion zsh)\"" >> "$zsh_rc"
            log_success "Added zsh autocompletion to $zsh_rc"
        else
            log_info "zsh autocompletion already configured in $zsh_rc"
        fi
    fi
    if [[ -f "$bash_rc" ]]; then
        if ! grep -q "rmd\"\{0,1\} completion bash" "$bash_rc"; then
            echo "eval \"\$(\"$RMD_BIN\" completion bash)\"" >> "$bash_rc"
            log_success "Added bash autocompletion to $bash_rc"
        else
            log_info "bash autocompletion already configured in $bash_rc"
        fi
    fi
}

# Display usage (platform-specific)
display_usage() {
    log_info "Installation completed successfully!"
    echo
    echo "=== Usage ==="
    echo "Add to PATH (optional):"
    if [[ "$OS_TYPE" == "macos" ]]; then
        echo "  echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.zshrc"
    elif [[ "$OS_TYPE" == "linux" ]]; then
        echo "  echo 'export PATH=\"\$PATH:$INSTALL_DIR\"' >> ~/.bashrc"
    fi
    echo
    echo "Manual control:"
    echo "  $INSTALL_DIR/start_reminders.sh"
    echo "  $INSTALL_DIR/stop_reminders.sh"
    echo
    echo "Build and plan your schedule with an AI coding agent (optional):"
    echo "  Claude Code: claude plugin marketplace add AutoPhd-org/schedule-everything"
    echo "               claude plugin install schedule-everything@schedule-everything"
    echo "  Codex:       codex plugin marketplace add AutoPhd-org/schedule-everything"
    echo "               codex plugin add schedule-everything@schedule-everything"
    echo "  Antigravity: agy plugin install $INSTALL_DIR/plugins/schedule-everything"
    echo
    if [[ "$OS_TYPE" == "macos" ]]; then
        echo "LaunchAgent:"
        echo "  launchctl load $LAUNCH_AGENT_PLIST   # Enable auto-start"
        echo "  launchctl unload $LAUNCH_AGENT_PLIST # Disable"
    elif [[ "$OS_TYPE" == "linux" ]]; then
        echo "systemd service:"
        echo "  systemctl --user start $SYSTEMD_SERVICE_NAME   # Start service"
        echo "  systemctl --user stop $SYSTEMD_SERVICE_NAME    # Stop service"
        echo "  systemctl --user enable $SYSTEMD_SERVICE_NAME  # Enable auto-start"
        echo "  systemctl --user disable $SYSTEMD_SERVICE_NAME # Disable auto-start"
        echo "  systemctl --user status $SYSTEMD_SERVICE_NAME  # Check status"
    fi
    echo
    echo "Logs: $INSTALL_DIR/logs/"
    echo
    if [[ "$OS_TYPE" == "macos" ]]; then
        log_warning "First run will prompt for Accessibility permissions in System Settings."
    elif [[ "$OS_TYPE" == "linux" ]]; then
        log_info "First run will use desktop notifications. Ensure notification daemon is running."
    fi
}

# Cleanup
cleanup() {
    if [[ -n "${TEMP_DIR:-}" && -d "${TEMP_DIR}" ]]; then
        rm -rf "${TEMP_DIR}"
    fi
}

# Main
main() {
    echo "=== $SCRIPT_NAME ==="
    trap cleanup EXIT

    check_os
    require_cargo

    # Outside a checkout, fetch the repository: the source, config templates, and plugins.
    if [[ ! -f "$SCRIPT_DIR/Cargo.toml" || ! -d "$SCRIPT_DIR/config" || ! -d "$SCRIPT_DIR/plugins" ]]; then
        log_info "Downloading the repository..."
        TEMP_DIR=$(mktemp -d -t schedule-installer-XXXXXX)
        if command -v git &>/dev/null; then
            git clone --depth 1 "${REPO_URL}.git" "${TEMP_DIR}"
        elif command -v curl &>/dev/null && command -v tar &>/dev/null; then
            curl -sSL "${REPO_URL}/archive/refs/heads/main.tar.gz" | tar -xz -C "${TEMP_DIR}" --strip-components=1
        else
            log_error "Required tools missing. Please install curl/tar or git."
            exit 1
        fi
        cd "${TEMP_DIR}"
        SCRIPT_DIR="${TEMP_DIR}"
    fi

    build_rmd
    stop_existing_service
    setup_install_dir
    install_binary
    configure_configs

    if [[ "$OS_TYPE" == "macos" ]]; then
        create_launch_agent
    elif [[ "$OS_TYPE" == "linux" ]]; then
        create_systemd_service
    fi

    request_permissions
    create_scripts
    refresh_browser_workspace
    test_installation
    setup_autocompletion
    restart_service_if_needed
    display_usage

    log_success "Installation complete! 🎉"
}

# Parse args
AUTO_YES="false"
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            echo "Usage: $0 [-y|--yes]"
            echo "  -y, --yes       Accept defaults without prompting"
            echo
            echo "Builds rmd with cargo (install Rust from https://rustup.rs)."
            exit 0
            ;;
        -y|--yes)
            AUTO_YES="true"
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Redirect stdin to controlling terminal if piped, unless running in non-interactive mode
if [[ "$AUTO_YES" == "false" ]] && [[ ! -t 0 ]] && [[ -c /dev/tty ]]; then
    exec < /dev/tty
fi

main
