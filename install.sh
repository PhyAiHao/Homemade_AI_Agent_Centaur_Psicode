#!/usr/bin/env bash
# Centaur Psicode Agent — Quick Install Script
# Usage: curl -fsSL https://raw.githubusercontent.com/centaur-psicode/agent/main/install.sh | bash

set -euo pipefail

REPO="centaur-psicode/agent"
BINARY_NAME="agent"

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "${OS}" in
    darwin)
        TARGET="universal-apple-darwin"
        ;;
    linux)
        case "${ARCH}" in
            x86_64|amd64)  TARGET="x86_64-unknown-linux-gnu" ;;
            aarch64|arm64) TARGET="aarch64-unknown-linux-gnu" ;;
            *) echo "Unsupported architecture: ${ARCH}"; exit 1 ;;
        esac
        ;;
    *) echo "Unsupported OS: ${OS}"; exit 1 ;;
esac

echo "Detected platform: ${TARGET}"

# Get latest release
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep tag_name | cut -d'"' -f4)
echo "Latest version: ${LATEST}"

# Download
URL="https://github.com/${REPO}/releases/download/${LATEST}/${BINARY_NAME}-${TARGET}.tar.gz"
echo "Downloading from ${URL}..."
curl -fsSL "${URL}" -o "/tmp/${BINARY_NAME}.tar.gz"

# Install
INSTALL_DIR="${HOME}/.local/bin"
mkdir -p "${INSTALL_DIR}"
tar xzf "/tmp/${BINARY_NAME}.tar.gz" -C "${INSTALL_DIR}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
rm "/tmp/${BINARY_NAME}.tar.gz"

echo ""
echo "Installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"
echo ""
echo "Make sure ${INSTALL_DIR} is in your PATH:"
echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
echo ""
echo "Run 'agent login' to authenticate, then 'agent' to start."
