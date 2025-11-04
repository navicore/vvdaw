#!/usr/bin/env bash
#
# Setup script to download the MIT-licensed VST3 SDK
#
# This script clones the VST3 SDK into the vendor directory so that
# the vvdaw-vst3 crate can generate Rust bindings from the C++ headers.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
VENDOR_DIR="$WORKSPACE_ROOT/vendor"
VST3_SDK_DIR="$VENDOR_DIR/vst3sdk"

echo "==> Setting up VST3 SDK for vvdaw"
echo

# Check if VST3 SDK already exists
if [ -d "$VST3_SDK_DIR" ]; then
    echo "VST3 SDK already exists at: $VST3_SDK_DIR"
    echo
    read -p "Do you want to update it? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "==> Updating VST3 SDK..."
        cd "$VST3_SDK_DIR"
        git pull
        echo "✓ VST3 SDK updated"
    else
        echo "Skipping update"
    fi
else
    # Create vendor directory if it doesn't exist
    mkdir -p "$VENDOR_DIR"

    echo "==> Cloning VST3 SDK from GitHub..."
    echo "    Repository: https://github.com/steinbergmedia/vst3sdk"
    echo "    Destination: $VST3_SDK_DIR"
    echo

    # Clone the VST3 SDK
    git clone --depth=1 --recursive https://github.com/steinbergmedia/vst3sdk.git "$VST3_SDK_DIR"

    echo
    echo "✓ VST3 SDK cloned successfully"
fi

echo
echo "==> VST3 SDK setup complete!"
echo
echo "The SDK is located at: $VST3_SDK_DIR"
echo
echo "You can now build vvdaw-vst3:"
echo "    cargo build -p vvdaw-vst3"
echo
