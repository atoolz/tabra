#!/bin/sh
# Tabra installer
# Usage: curl -fsSL https://raw.githubusercontent.com/atoolz/tabra/main/install.sh | sh

set -e

REPO="atoolz/tabra"
INSTALL_DIR="${TABRA_INSTALL_DIR:-$HOME/.local/bin}"

# Detect OS and architecture
detect_platform() {
    OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    ARCH=$(uname -m)

    case "$OS" in
        linux) OS="linux" ;;
        darwin) OS="darwin" ;;
        *)
            echo "Unsupported OS: $OS"
            exit 1
            ;;
    esac

    case "$ARCH" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *)
            echo "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    ARTIFACT="tabra-${OS}-${ARCH}"
    echo "Detected platform: ${OS}/${ARCH}"
}

# Get latest release tag
get_latest_version() {
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    if [ -z "$VERSION" ]; then
        echo "Failed to get latest version"
        exit 1
    fi
    echo "Latest version: ${VERSION}"
}

# Download and install binary
install_binary() {
    URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARTIFACT}.tar.gz"
    echo "Downloading ${URL}..."

    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    curl -fsSL "$URL" -o "${TMP_DIR}/${ARTIFACT}.tar.gz"
    tar xzf "${TMP_DIR}/${ARTIFACT}.tar.gz" -C "$TMP_DIR"

    mkdir -p "$INSTALL_DIR"
    mv "${TMP_DIR}/tabra" "${INSTALL_DIR}/tabra"
    chmod +x "${INSTALL_DIR}/tabra"

    echo "Installed tabra to ${INSTALL_DIR}/tabra"
}

# Verify installation
verify() {
    if command -v tabra >/dev/null 2>&1; then
        echo ""
        echo "tabra $(tabra --version 2>/dev/null || echo 'installed')"
    elif [ -x "${INSTALL_DIR}/tabra" ]; then
        echo ""
        echo "Installed successfully, but ${INSTALL_DIR} is not in your PATH."
        echo "Add it:"
        echo "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    fi
}

# Print setup instructions
print_setup() {
    echo ""
    echo "Setup:"
    echo ""

    SHELL_NAME=$(basename "${SHELL:-/bin/sh}")
    case "$SHELL_NAME" in
        zsh)
            echo "  Add to ~/.zshrc:"
            echo "    eval \"\$(tabra init zsh)\""
            ;;
        bash)
            echo "  Add to ~/.bashrc:"
            echo "    eval \"\$(tabra init bash)\""
            ;;
        fish)
            echo "  Add to ~/.config/fish/config.fish:"
            echo "    tabra init fish | source"
            ;;
        *)
            echo "  Add to your shell config:"
            echo "    eval \"\$(tabra init zsh)\"   # for zsh"
            echo "    eval \"\$(tabra init bash)\"  # for bash"
            echo "    tabra init fish | source    # for fish"
            ;;
    esac

    echo ""
    echo "Then restart your shell or run: exec $SHELL_NAME"
    echo ""
    echo "Tab. Complete. Ship."
}

main() {
    echo "Installing Tabra..."
    echo ""
    detect_platform
    get_latest_version
    install_binary
    verify
    print_setup
}

main
