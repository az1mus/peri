#!/bin/bash
set -euo pipefail
export LC_ALL=C

# Peri Install Script (Local Build Edition)
# Usage: cd peri && bash scripts/install.sh
#
# This fork builds Peri from source instead of downloading pre-built binaries.
# Prerequisites: Rust toolchain (cargo), Git
#
# Options:
#   PERI_INSTALL_DIR       Install directory (default: $HOME/.peri)
#   PERI_NO_PATH_HINT      Set to 1 to skip PATH hint
#
# Example:
#   cd peri && bash scripts/install.sh
#   PERI_INSTALL_DIR=/opt/peri bash scripts/install.sh

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()    { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
step()    { echo -e "${CYAN}[STEP]${NC}  $*"; }

# --- Cleanup Old Versions ---
cleanup_old_versions() {
    local install_dir="$1"
    local current_version="$2"

    # Collect agent-v* directories, excluding current version
    local old_dirs=()
    for d in "${install_dir}"/agent-v*; do
        [[ -d "$d" ]] || continue
        local base
        base=$(basename "$d")
        [[ "$base" == "$current_version" ]] && continue
        old_dirs+=("$d")
    done

    if [[ ${#old_dirs[@]} -eq 0 ]]; then
        info "No old versions to clean up."
        return
    fi

    echo ""
    warn "Found ${#old_dirs[@]} old version(s):"
    for d in "${old_dirs[@]}"; do
        local size
        size=$(du -sh "$d" 2>/dev/null | cut -f1)
        echo "  $(basename "$d")  (${size})"
    done
    local total_human
    total_human=$(du -sh "${old_dirs[@]}" 2>/dev/null | tail -1 | cut -f1)
    echo "  Total: ${total_human}"
    echo ""

    # Read from /dev/tty to work with curl | bash pipe
    if ! [[ -t 0 ]] && [[ -e /dev/tty ]]; then
        exec 3< /dev/tty
    else
        exec 3<&0
    fi

    echo -e "${YELLOW}[WARN]${NC}  Delete old versions? [y/N] " >/dev/tty
    local answer
    read -r answer <&3
    exec 3<&-

    case "${answer}" in
        [yY]|[yY][eE][sS])
            for d in "${old_dirs[@]}"; do
                rm -rf "$d"
                info "Removed: $(basename "$d")"
            done
            info "Cleaned up ${#old_dirs[@]} old version(s)."
            ;;
        *)
            info "Skipped cleanup."
            ;;
    esac
}

# --- Main ---
main() {
    INSTALL_DIR="${PERI_INSTALL_DIR:-${HOME}/.peri}"

    echo ""
    info "Peri Agent Installer (Local Build)"
    info "-------------------------------"

    # Check prerequisites
    if ! command -v cargo &>/dev/null; then
        error "cargo not found. Please install Rust: https://rustup.rs"
        exit 1
    fi

    # Determine version from git
    if command -v git &>/dev/null && git rev-parse --git-dir >/dev/null 2>&1; then
        VERSION_TAG=$(git describe --tags --always 2>/dev/null || echo "local-$(date +%Y%m%d)")
    else
        VERSION_TAG="local-$(date +%Y%m%d)"
    fi
    info "Build version: ${VERSION_TAG}"

    # Build
    step "Building peri from source..."
    cargo build -p peri-tui --release || {
        error "Build failed."
        exit 1
    }

    # Detect binary name
    local BINARY_NAME
    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) BINARY_NAME="peri.exe" ;;
        *)                      BINARY_NAME="peri" ;;
    esac

    BINARY_PATH="target/release/${BINARY_NAME}"
    if [[ ! -f "${BINARY_PATH}" ]]; then
        error "Binary not found at ${BINARY_PATH}"
        exit 1
    fi

    # Create install directory
    VERSION_DIR="${INSTALL_DIR}/${VERSION_TAG}"
    mkdir -p "${VERSION_DIR}"

    TARGET="${VERSION_DIR}/${BINARY_NAME}"

    # Copy binary
    step "Installing..."
    cp -f "${BINARY_PATH}" "${TARGET}" || {
        error "Copy failed."
        exit 1
    }

    # Make executable
    chmod +x "${TARGET}"
    info "Installed to: ${TARGET}"

    # Create symlink for convenience
    LINK="${INSTALL_DIR}/peri"
    rm -f "${LINK}"
    ln -sf "${TARGET}" "${LINK}"

    # Write current version
    echo "${VERSION_TAG}" > "${INSTALL_DIR}/current-version.txt"

    # --- PATH Setup ---
    if [[ "${PERI_NO_PATH_HINT:-}" != "1" ]]; then
        BIN_LINK="${INSTALL_DIR}/peri"
        SHELL_PROFILE=""
        case "${SHELL:-}" in
            */zsh)  SHELL_PROFILE="${HOME}/.zshrc" ;;
            */bash) SHELL_PROFILE="${HOME}/.bashrc" ;;
            */fish) SHELL_PROFILE="${HOME}/.config/fish/config.fish" ;;
        esac

        if [[ -n "${SHELL_PROFILE}" ]]; then
            # Check for exact PATH entry (not substring: avoid .peri matching .perihelion)
            INSTALL_DIR_ESC="${INSTALL_DIR//\./\\.}"
            if ! grep -qE "(^|[:\" ])${INSTALL_DIR_ESC}([:\"\$ ]|$)" "${SHELL_PROFILE}" 2>/dev/null; then
                if [[ "${SHELL}" == */fish ]]; then
                    echo "set -gx PATH ${INSTALL_DIR} \$PATH" >> "${SHELL_PROFILE}"
                else
                    echo "export PATH=\"${INSTALL_DIR}:\$PATH\"" >> "${SHELL_PROFILE}"
                fi
                info "Added ${INSTALL_DIR} to PATH in ${SHELL_PROFILE}"
            fi
        else
            echo ""
            warn "Unknown shell. Add this directory to your PATH manually:"
            echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
            echo ""
        fi
    fi

    # Offer to clean up old versions
    cleanup_old_versions "${INSTALL_DIR}" "${VERSION_TAG}"

    echo ""
    info "Installation complete! Version: ${VERSION_TAG}"
    echo ""

    if command -v "${BIN_LINK}" &>/dev/null || [[ -x "${BIN_LINK}" ]]; then
        info "Run 'peri' to start."
    else
        info "Run: ${BIN_LINK}"
    fi
    echo ""
}

main
