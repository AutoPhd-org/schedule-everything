#!/bin/bash

# This script sets up the schedule management reminder system for macOS and Linux

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_URL="https://github.com/PhDeasy-org/schedule-everything"
TEMP_DIR=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script configuration
SCRIPT_NAME="Schedule Management Installer"
PYTHON_VERSION="3.12"
# VENV_NAME="schedule_management_env"
INSTALL_DIR="$HOME/SCHEDULE_MANAGEMENT"
INSTALL_CONFIG_DIR="$HOME/.schedule_management/config"
INSTALL_ACTIVE_CONFIG_ID="0"
INSTALL_ACTIVE_CONFIG_DIR="$INSTALL_CONFIG_DIR/user_config_${INSTALL_ACTIVE_CONFIG_ID}"
INSTALL_ACTIVE_CONFIG_FILE="$INSTALL_CONFIG_DIR/.active_config"
LAUNCH_AGENT_NAME="com.sergiudm.schedule.management.reminder"
LAUNCH_AGENT_PLIST="$HOME/Library/LaunchAgents/${LAUNCH_AGENT_NAME}.plist"
SYSTEMD_SERVICE_NAME="schedule-management.service"
SYSTEMD_SERVICE_PATH="$HOME/.config/systemd/user/${SYSTEMD_SERVICE_NAME}"

# Detect OS
detect_os() {
    if [[ "$OSTYPE" == "darwin"* ]]; then
        echo "macos"
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
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
            if [[ "$base_name" == "llm.toml" ]]; then
                continue
            fi
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
        log_info "Supported systems: macOS (darwin) and Linux (linux-gnu)"
        exit 1
    fi
    log_success "$OS_TYPE detected"
}

# Get Linux distribution info
get_linux_distro() {
    if [[ -f /etc/os-release ]]; then
        . /etc/os-release
        echo "$ID"
    else
        echo "unknown"
    fi
}

# Install package manager dependencies for Linux
install_linux_dependencies() {
    log_info "Installing Linux dependencies..."
    
    DISTRO=$(get_linux_distro)
    log_info "Detected Linux distribution: $DISTRO"
    
    case "$DISTRO" in
        ubuntu|debian)
            sudo apt update
            sudo apt install -y curl build-essential libssl-dev zlib1g-dev \
                libbz2-dev libreadline-dev libsqlite3-dev libncursesw5-dev \
                xz-utils tk-dev libxml2-dev libxmlsec1-dev libffi-dev liblzma-dev
            ;;
        fedora)
            sudo dnf install -y curl gcc zlib-devel bzip2 bzip2-devel \
                readline-devel sqlite sqlite-devel openssl-devel xz xz-devel \
                libffi-devel findutils
            ;;
        arch|manjaro)
            sudo pacman -S --needed curl base-devel openssl zlib xz
            ;;
        *)
            log_warning "Unknown distribution: $DISTRO. Please install dependencies manually:"
            log_info "Required: curl, build tools, Python development headers, OpenSSL"
            ;;
    esac
    
    log_success "Linux dependencies installed"
}

# Check if Homebrew is installed (macOS only)
check_homebrew() {
    if [[ "$OS_TYPE" != "macos" ]]; then
        return 0
    fi
    
    if ! command -v brew &> /dev/null; then
        log_warning "Homebrew not found. Installing Homebrew..."
        /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        if [[ ":$PATH:" != *":/opt/homebrew/bin:"* ]]; then
            echo 'eval "$(/opt/homebrew/bin/brew shellenv)"' >> ~/.zprofile
            eval "$(/opt/homebrew/bin/brew shellenv)"
        fi
    else
        log_success "Homebrew is already installed"
    fi
}

# Install uv package manager
install_uv() {
    log_info "Checking uv installation..."
    if ! command -v uv &> /dev/null; then
        log_info "uv not found. Installing uv..."
        
        if [[ "$OS_TYPE" == "macos" ]]; then
            brew install uv
        elif [[ "$OS_TYPE" == "linux" ]]; then
            # Install uv using the official installer
            curl -LsSf https://astral.sh/uv/install.sh | sh
            # Add to PATH
            export PATH="$HOME/.cargo/bin:$PATH"
            echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
        fi
        log_success "uv installed successfully"
    else
        log_success "uv is already installed"
    fi
    
    if ! uv --version &> /dev/null; then
        log_error "uv installation verification failed"
        exit 1
    fi
}

# Install Python (platform-specific)
install_python() {
    log_info "Setting up Python $PYTHON_VERSION..."
    
    if [[ "$OS_TYPE" == "macos" ]]; then
        # macOS: use pyenv
        if ! command -v pyenv &> /dev/null; then
            log_info "Installing pyenv..."
            brew install pyenv
            echo 'eval "$(pyenv init -)"' >> ~/.zshrc
            eval "$(pyenv init -)"
        fi
        if ! pyenv versions --bare | grep -q "^$PYTHON_VERSION$"; then
            log_info "Installing Python $PYTHON_VERSION..."
            pyenv install "$PYTHON_VERSION"
        fi
        pyenv local "$PYTHON_VERSION"
    elif [[ "$OS_TYPE" == "linux" ]]; then
        # Linux: use system Python or pyenv
        if command -v "python${PYTHON_VERSION}" &> /dev/null; then
            log_success "Python $PYTHON_VERSION found in system"
        else
            log_info "Installing Python $PYTHON_VERSION using pyenv..."
            if ! command -v pyenv &> /dev/null; then
                curl https://pyenv.run | bash
                export PATH="$HOME/.pyenv/bin:$PATH"
                eval "$(pyenv init -)"
                echo 'export PATH="$HOME/.pyenv/bin:$PATH"' >> ~/.bashrc
                echo 'eval "$(pyenv init -)"' >> ~/.bashrc
            fi
            if ! pyenv versions --bare | grep -q "^$PYTHON_VERSION$"; then
                pyenv install "$PYTHON_VERSION"
            fi
            pyenv local "$PYTHON_VERSION"
        fi
    fi
    
    log_success "Python $PYTHON_VERSION is ready"
}

# Setup project directory (preserve src layout!)
setup_project() {
    log_info "Setting up project directory..."

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

    mkdir -p "$INSTALL_DIR"

    if [[ -d "src" ]]; then
        cp -r src "$INSTALL_DIR/"
        log_success "src/ directory copied (preserving layout)"
    else
        log_error "src/ directory not found in current working directory!"
        exit 1
    fi

    if [[ -d "third_party" ]]; then
        cp -r third_party "$INSTALL_DIR/"
        log_success "third_party/ directory copied"
    fi

    # Copy build metadata files needed by editable installs.
    for file in LICENSE pyproject.toml README.md uv.lock requirements.txt requirements-test.txt; do
        if [[ -f "$file" ]]; then
            cp "$file" "$INSTALL_DIR/"
            log_success "$file copied"
        fi
    done

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

    # Create logs directory
    mkdir -p "$INSTALL_DIR/logs"
    log_success "Project directory setup complete"
}

# Create virtual environment
create_venv() {
    log_info "Creating virtual environment with uv..."
    uv venv --python=3.12 --clear "$INSTALL_DIR/.venv"
    if [[ -f "$INSTALL_DIR/.venv/bin/activate" ]]; then
        source "$INSTALL_DIR/.venv/bin/activate"
        log_success "Virtual environment created and activated with uv"
    else
        log_error "Virtual environment creation failed"
        exit 1
    fi
}

# Install dependencies and package
install_dependencies() {
    log_info "Installing dependencies and package..."

    cd "$INSTALL_DIR"

    # Upgrade pip
    uv pip install --upgrade pip

    # Install from requirements if available
    if [[ -f "requirements.txt" ]]; then
        uv pip install -r requirements.txt
    fi
    if [[ -f "requirements-test.txt" ]]; then
        uv pip install -r requirements-test.txt
    fi

    # Install editable package (supports src layout)
    uv pip install -e .
    if ! command -v rmd &> /dev/null; then
        log_warning "CLI 'rmd' not in PATH, but should work via venv"
    else
        log_success "CLI tool 'rmd' is available"
    fi

    cd - > /dev/null
}

# Validate and complete missing configuration values
configure_configs() {
    log_info "Checking configuration files and required values..."

    local wizard_script="$INSTALL_DIR/src/schedule_management/install_config_wizard.py"
    local template_dir="$SCRIPT_DIR/config/user_config_0"
    local target_config_dir
    target_config_dir="$(resolve_install_target_config_dir)"

    if [[ ! -f "$wizard_script" ]]; then
        log_error "Config wizard not found: $wizard_script"
        exit 1
    fi

    local extra_args=()
    if [[ "$AUTO_YES" == "true" ]]; then
        extra_args+=("--yes")
    fi

    local wizard_invocation=(
        "$INSTALL_DIR/.venv/bin/python" "$wizard_script"
        --config-dir "$target_config_dir"
    )
    if [[ -d "$template_dir" ]]; then
        wizard_invocation+=(--template-dir "$template_dir")
    else
        log_warning "Template directory not found at $template_dir; using config directory as template source."
    fi
    if [[ ${#extra_args[@]} -gt 0 ]]; then
        wizard_invocation+=("${extra_args[@]}")
    fi

    "${wizard_invocation[@]}"

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
ExecStart=$INSTALL_DIR/.venv/bin/python $INSTALL_DIR/src/schedule_management/reminder_macos.py
Restart=always
RestartSec=10
Environment=DISPLAY=:0
Environment=REMINDER_CONFIG_DIR=$INSTALL_CONFIG_DIR
StandardOutput=$INSTALL_DIR/logs/schedule_management.out
StandardError=$INSTALL_DIR/logs/schedule_management.err

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
        <string>$INSTALL_DIR/.venv/bin/python</string>
        <string>$INSTALL_DIR/src/schedule_management/reminder_macos.py</string>
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

# Request permissions (platform-specific)
request_permissions() {
    if [[ "$OS_TYPE" == "macos" ]]; then
        log_info "The app will request Accessibility & Notification permissions on first run."
    elif [[ "$OS_TYPE" == "linux" ]]; then
        log_info "The app will use desktop notifications. Ensure notification daemon is running."
    fi
}

# Create convenience scripts (platform-specific)
create_scripts() {
    log_info "Creating convenience scripts..."

    # Start script (common)
    cat > "$INSTALL_DIR/start_reminders.sh" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
source "$INSTALL_DIR/.venv/bin/activate"
cd "$INSTALL_DIR/src/schedule_management"
exec python reminder_macos.py
EOF

    # Stop script (platform-specific)
    if [[ "$OS_TYPE" == "macos" ]]; then
        cat > "$INSTALL_DIR/stop_reminders.sh" << EOF
#!/bin/bash
launchctl unload "\$HOME/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist" 2>/dev/null || true
pkill -f "python.*reminder_macos.py" 2>/dev/null || true
EOF
    elif [[ "$OS_TYPE" == "linux" ]]; then
        cat > "$INSTALL_DIR/stop_reminders.sh" << EOF
#!/bin/bash
systemctl --user stop schedule-management.service 2>/dev/null || true
systemctl --user disable schedule-management.service 2>/dev/null || true
pkill -f "python.*reminder_macos.py" 2>/dev/null || true
EOF
    fi

    # Restart script (platform-specific)
    if [[ "$OS_TYPE" == "macos" ]]; then
        cat > "$INSTALL_DIR/restart_reminders.sh" << EOF
#!/bin/bash
"$INSTALL_DIR/stop_reminders.sh"
sleep 2
launchctl load "\$HOME/Library/LaunchAgents/com.sergiudm.schedule.management.reminder.plist"
EOF
    elif [[ "$OS_TYPE" == "linux" ]]; then
        cat > "$INSTALL_DIR/restart_reminders.sh" << EOF
#!/bin/bash
"$INSTALL_DIR/stop_reminders.sh"
sleep 2
systemctl --user start schedule-management.service
EOF
    fi

    # Visualize script (common)
    cat > "$INSTALL_DIR/visualize_schedule.sh" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
source "$INSTALL_DIR/.venv/bin/activate"
cd "$INSTALL_DIR/src/schedule_management"
exec python reminder_macos.py --visualize
EOF

    # Primary CLI wrapper (common)
    cat > "$INSTALL_DIR/rmd" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
source "$INSTALL_DIR/.venv/bin/activate"
exec rmd "\$@"
EOF

    # Legacy compatibility wrapper
    cat > "$INSTALL_DIR/reminder" << EOF
#!/bin/bash
export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
source "$INSTALL_DIR/.venv/bin/activate"
exec rmd "\$@"
EOF

    chmod +x "$INSTALL_DIR"/*.sh "$INSTALL_DIR/rmd" "$INSTALL_DIR/reminder"
    log_success "Convenience scripts created"
}

# Test installation
test_installation() {
    log_info "Testing installation..."

    source "$INSTALL_DIR/.venv/bin/activate"
    export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"

    if python -c "import schedule_management; print('OK')" &>/dev/null; then
        log_success "schedule_management import test passed"
    else
        log_error "Failed to import schedule_management"
        exit 1
    fi

    if rmd --help &>/dev/null; then
        log_success "CLI 'rmd' works correctly"
    else
        log_error "CLI 'rmd' failed"
        exit 1
    fi

    log_success "All tests passed"
}

# Install desktop widget (macOS only)
install_desktop_widget() {
    if [[ "$OS_TYPE" != "macos" ]]; then
        return 0
    fi

    local widget_dir="$HOME/Library/Application Support/Übersicht/widgets/rmd-tasks.widget"
    if [[ -d "$widget_dir" ]]; then
        log_info "Desktop widget is already installed"
        return 0
    fi

    if [[ "$AUTO_YES" == "true" ]]; then
        log_info "Skipping desktop widget installation in non-interactive mode"
        return 0
    fi

    while true; do
        read -r -p "Install desktop widget to show tasks on your desktop? (requires Übersicht) [Y/n]: " install_widget
        case "$install_widget" in
            [yY]|[yY][eE][sS]|"")
                break
                ;;
            [nN]|[nN][oO])
                log_info "Skipping desktop widget installation"
                return 0
                ;;
            *)
                log_warning "Invalid input. Please answer with y or n."
                ;;
        esac
    done

    # Install Übersicht if not present
    if ! mdfind "kMDItemCFBundleIdentifier == 'tracesOf.Uebersicht'" | grep -q .; then
        if ! command -v brew &> /dev/null; then
            log_warning "Homebrew not found. Cannot install Übersicht automatically."
            log_info "Install Übersicht from https://tracesof.net/uebersicht/ and re-run the installer."
            return 0
        fi
        log_info "Installing Übersicht via Homebrew..."
        if ! brew install --cask ubersicht; then
            log_error "Failed to install Übersicht"
            return 0
        fi
        log_success "Übersicht installed"
    else
        log_success "Übersicht is already installed"
    fi

    # Install widget using the Python module
    source "$INSTALL_DIR/.venv/bin/activate"
    export REMINDER_CONFIG_DIR="$INSTALL_CONFIG_DIR"
    if python -c "from schedule_management.desktop_widget import install_widget; install_widget('$INSTALL_DIR/rmd')" 2>/dev/null; then
        log_success "Desktop widget installed at $widget_dir"
    else
        log_error "Failed to install desktop widget"
    fi
}

# Setup shell autocompletion
setup_autocompletion() {
    log_info "Setting up shell autocompletion..."
    
    local rmd_cmd="$INSTALL_DIR/rmd"

    if [[ "$OS_TYPE" == "macos" ]]; then
        # zsh is default on macOS
        if [[ -f "$HOME/.zshrc" ]]; then
            if ! grep -q "rmd completion zsh" "$HOME/.zshrc"; then
                echo "eval \"\$(\"$rmd_cmd\" completion zsh)\"" >> "$HOME/.zshrc"
                log_success "Added zsh autocompletion to ~/.zshrc"
            else
                log_info "zsh autocompletion already configured in ~/.zshrc"
            fi
        fi
        # fallback to bash
        if [[ -f "$HOME/.bash_profile" ]]; then
            if ! grep -q "rmd completion bash" "$HOME/.bash_profile"; then
                echo "eval \"\$(\"$rmd_cmd\" completion bash)\"" >> "$HOME/.bash_profile"
                log_success "Added bash autocompletion to ~/.bash_profile"
            fi
        fi
    elif [[ "$OS_TYPE" == "linux" ]]; then
        if [[ -f "$HOME/.bashrc" ]]; then
            if ! grep -q "rmd completion bash" "$HOME/.bashrc"; then
                echo "eval \"\$(\"$rmd_cmd\" completion bash)\"" >> "$HOME/.bashrc"
                log_success "Added bash autocompletion to ~/.bashrc"
            else
                log_info "bash autocompletion already configured in ~/.bashrc"
            fi
        fi
        if [[ -f "$HOME/.zshrc" ]]; then
            if ! grep -q "rmd completion zsh" "$HOME/.zshrc"; then
                echo "eval \"\$(\"$rmd_cmd\" completion zsh)\"" >> "$HOME/.zshrc"
                log_success "Added zsh autocompletion to ~/.zshrc"
            fi
        fi
    fi
}

# Install pi CLI if missing and npm is available
install_pi() {
    if command -v pi &> /dev/null; then
        return 0
    fi

    if ! command -v npm &> /dev/null; then
        return 0
    fi

    if [[ "$AUTO_YES" == "true" ]]; then
        log_info "Skipping pi CLI installation in non-interactive mode."
        return 0
    fi
    echo
    while true; do
        read -r -p "Install pi CLI? (required for AI-assisted commands) [Y/n]: " install_pi_answer
        case "$install_pi_answer" in
            [yY]|[yY][eE][sS]|"")
                log_info "Installing pi CLI..."
                npm install -g @earendil-works/pi-coding-agent
                log_success "pi CLI installed"
                break
                ;;
            [nN]|[nN][oO])
                log_info "Skipping pi CLI installation"
                break
                ;;
            *)
                log_warning "Invalid input. Please answer with y or n."
                ;;
        esac
    done
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
    if ! command -v pi &> /dev/null; then
        echo "pi CLI installation (required for AI-assisted commands):"
        echo "  npm install -g @earendil-works/pi-coding-agent"
        echo "  To use a non-PATH binary, run:"
        echo "    export REMINDER_PI_BIN=/path/to/pi"
        echo
    fi
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
    if [[ -n "${VIRTUAL_ENV:-}" ]]; then
        deactivate 2>/dev/null || true
    fi
    if [[ -n "${TEMP_DIR:-}" && -d "${TEMP_DIR}" ]]; then
        rm -rf "${TEMP_DIR}"
    fi
}

# Main
main() {
    echo "=== $SCRIPT_NAME ==="
    trap cleanup EXIT

    # If not running from within the repository (src/ is missing), download the repo
    if [[ ! -d "src" ]]; then
        log_info "Source code not found in current directory. Downloading repository..."
        TEMP_DIR=$(mktemp -d -t schedule-installer-XXXXXX)
        
        # Prefer git clone with recursive submodules to get everything cleanly
        if command -v git &>/dev/null; then
            log_info "Cloning repository recursively..."
            git clone --recursive --depth 1 "${REPO_URL}.git" "${TEMP_DIR}"
        elif command -v curl &>/dev/null && command -v tar &>/dev/null; then
            log_info "Downloading repository tarball..."
            curl -sSL "${REPO_URL}/archive/refs/heads/main.tar.gz" | tar -xz -C "${TEMP_DIR}" --strip-components=1
        else
            log_error "Required tools missing. Please install curl/tar or git."
            exit 1
        fi
        
        cd "${TEMP_DIR}"
        SCRIPT_DIR="${TEMP_DIR}"
    fi

    check_os
    
    if [[ "$OS_TYPE" == "linux" ]]; then
        install_linux_dependencies
    fi
    
    check_homebrew
    install_uv
    # install_python
    setup_project
    create_venv
    configure_configs
    install_dependencies
    install_pi
    
    if [[ "$OS_TYPE" == "macos" ]]; then
        create_launch_agent
    elif [[ "$OS_TYPE" == "linux" ]]; then
        create_systemd_service
    fi
    
    request_permissions
    create_scripts
    test_installation
    install_desktop_widget
    setup_autocompletion
    display_usage

    log_success "Installation complete! 🎉"
}

# Parse args (minimal)
AUTO_YES="false"
while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            echo "Usage: $0 [-y|--yes]"
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
