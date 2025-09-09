#!/usr/bin/env bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

REPO="fayazexo/dicom-json"
INSTALL_DIR="$HOME/.local/bin"

echo -e "${BLUE}Installing DICOM-JSON...${NC}"

# Create install directory
mkdir -p "$INSTALL_DIR"

# Detect platform
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

case "$OS" in
    darwin) PLATFORM="macos" ;;
    linux) PLATFORM="linux" ;;
    *) echo -e "${RED}Unsupported OS: $OS${NC}"; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64) ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *) echo -e "${RED}Unsupported architecture: $ARCH${NC}"; exit 1 ;;
esac

# Get latest version
echo "Getting latest version..."
VERSION=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | cut -d'"' -f4)

if [ -z "$VERSION" ]; then
    echo -e "${RED}Failed to get latest version${NC}"
    exit 1
fi

echo "Latest version: $VERSION"

# Download URL
FILENAME="dicom-json-${PLATFORM}-${ARCH}.tar.gz"
URL="https://github.com/$REPO/releases/download/$VERSION/$FILENAME"

echo "Downloading $FILENAME..."

# Download and extract
curl -L "$URL" | tar -xz -C "$INSTALL_DIR"

# Make executable
chmod +x "$INSTALL_DIR/dicom-json"

# Check if it's in PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    echo ""
    echo -e "${GREEN}dicom-json installed to $INSTALL_DIR/dicom-json${NC}"
    echo ""
    echo -e "${BLUE}To use from anywhere, add this to your shell profile:${NC}"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
    echo ""
    echo -e "${BLUE}Or run directly:${NC}"
    echo "  $INSTALL_DIR/dicom-json --help"
else
    echo ""
    echo -e "${GREEN}dicom-json installed and ready!${NC}"
    echo ""
    echo "Try it: dicom-json --help"
fi