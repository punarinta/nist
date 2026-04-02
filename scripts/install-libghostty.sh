#!/usr/bin/env bash
# install-libghostty.sh - Build libghostty-vt from source and update the prebuilt artifacts.
#
# Run this when you need to rebuild the vendored library (e.g. after bumping
# GHOSTTY_COMMIT in build.rs or switching to a different CPU architecture).
#
# Zig is required. If not in PATH, this script will download it into
# third-party/zig/ automatically (no snap, no sudo required).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
PREBUILT_DIR="$PROJECT_ROOT/third-party/libghostty-prebuilt"
GHOSTTY_COMMIT="debcffbadb75221a030319c075fae12cfe114176"
GHOSTTY_REPO="https://github.com/ghostty-org/ghostty.git"
ZIG_VERSION="0.15.2"
ZIG_DIR="$PROJECT_ROOT/third-party/zig"
WORK_DIR="$(mktemp -d)"

cleanup() { rm -rf "$WORK_DIR"; }
trap cleanup EXIT

# Locate or install zig — prefer local third-party/zig over system zig
if [ -x "$ZIG_DIR/zig" ]; then
    ZIG="$ZIG_DIR/zig"
    echo "Using local zig: $($ZIG version)"
else
    echo "zig not found — downloading zig $ZIG_VERSION into third-party/zig/ ..."
    ARCH="$(uname -m)"
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    case "$ARCH" in
        x86_64)  ZIG_ARCH="x86_64" ;;
        aarch64) ZIG_ARCH="aarch64" ;;
        arm64)   ZIG_ARCH="aarch64" ;;
        *)       echo "Unsupported arch: $ARCH"; exit 1 ;;
    esac
    case "$OS" in
        linux)  ZIG_OS="linux" ;;
        darwin) ZIG_OS="macos" ;;
        *)      echo "Unsupported OS: $OS"; exit 1 ;;
    esac
    ZIG_TARBALL="zig-$ZIG_ARCH-$ZIG_OS-$ZIG_VERSION.tar.xz"
    ZIG_URL="https://ziglang.org/download/$ZIG_VERSION/$ZIG_TARBALL"
    echo "Downloading $ZIG_URL ..."
    curl -fL "$ZIG_URL" -o "$WORK_DIR/$ZIG_TARBALL"
    mkdir -p "$ZIG_DIR"
    tar -xJf "$WORK_DIR/$ZIG_TARBALL" -C "$ZIG_DIR" --strip-components=1
    ZIG="$ZIG_DIR/zig"
    echo "Installed zig $($ZIG version) in $ZIG_DIR"
fi

# Clone ghostty at the pinned commit
echo "Cloning ghostty $GHOSTTY_COMMIT ..."
git clone --filter=blob:none --no-checkout "$GHOSTTY_REPO" "$WORK_DIR/ghostty"
git -C "$WORK_DIR/ghostty" checkout "$GHOSTTY_COMMIT"

# Build
echo "Building libghostty-vt ..."
(cd "$WORK_DIR/ghostty" && "$ZIG" build -Demit-lib-vt --prefix "$WORK_DIR/install")

# Copy artifacts into prebuilt/
echo "Updating prebuilt artifacts ..."
rm -rf "$PREBUILT_DIR"
mkdir -p "$PREBUILT_DIR/lib" "$PREBUILT_DIR/include/ghostty"

# Copy static library (same name on all platforms)
cp "$WORK_DIR/install/lib/libghostty-vt.a" "$PREBUILT_DIR/lib/"

# Copy shared library (platform-specific)
OS="$(uname -s)"
if [ "$OS" = "Darwin" ]; then
    # macOS: look for .dylib
    DYLIB=$(find "$WORK_DIR/install/lib" -name "libghostty-vt.*.dylib" | head -1)
    if [ -n "$DYLIB" ]; then
        cp "$DYLIB" "$PREBUILT_DIR/lib/"
        DYLIB_NAME="$(basename "$DYLIB")"
        (cd "$PREBUILT_DIR/lib" && ln -sf "$DYLIB_NAME" libghostty-vt.dylib)
    fi
else
    # Linux: look for .so
    if [ -f "$WORK_DIR/install/lib/libghostty-vt.so.0.1.0" ]; then
        cp "$WORK_DIR/install/lib/libghostty-vt.so.0.1.0" "$PREBUILT_DIR/lib/"
        (cd "$PREBUILT_DIR/lib" \
            && ln -sf libghostty-vt.so.0.1.0 libghostty-vt.so.0 \
            && ln -sf libghostty-vt.so.0.1.0 libghostty-vt.so)
    fi
fi

# Copy headers
cp -r "$WORK_DIR/install/include/ghostty/." "$PREBUILT_DIR/include/ghostty/"

echo "Done. Prebuilt artifacts updated in $PREBUILT_DIR"
echo "Run 'cargo build' to rebuild the project."
