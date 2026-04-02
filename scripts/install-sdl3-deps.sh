#!/bin/bash
# SDL3 Build Dependencies Installation Script
# This script installs the necessary system dependencies to build SDL3 and SDL3_ttf from source

set -e

echo "Installing SDL3 build dependencies..."

OS="$(uname -s)"

if [ "$OS" = "Darwin" ]; then
    # macOS: use Homebrew
    if ! command -v brew &>/dev/null; then
        echo "Homebrew is required on macOS. Install it from https://brew.sh"
        exit 1
    fi
    echo "Detected macOS"
    brew install cmake pkg-config freetype harfbuzz
    echo "SDL3 build dependencies installed successfully!"
    exit 0
fi

# Linux: detect distribution
if [ ! -f /etc/os-release ]; then
    echo "Cannot detect Linux distribution"
    exit 1
fi

. /etc/os-release
DISTRO=$ID

case $DISTRO in
    ubuntu|debian|pop|linuxmint)
        echo "Detected Debian/Ubuntu-based system"
        sudo apt-get update
        sudo apt-get install -y \
            build-essential \
            cmake \
            pkg-config \
            libx11-dev \
            libxext-dev \
            libxrandr-dev \
            libxcursor-dev \
            libxi-dev \
            libxfixes-dev \
            libxrender-dev \
            libxss-dev \
            libxtst-dev \
            libasound2-dev \
            libpulse-dev \
            libjack-dev \
            libsndio-dev \
            libdbus-1-dev \
            libibus-1.0-dev \
            libwayland-dev \
            libxkbcommon-dev \
            libegl1-mesa-dev \
            libgles2-mesa-dev \
            libdrm-dev \
            libgbm-dev \
            libfreetype6-dev \
            libharfbuzz-dev
        echo "SDL3 and SDL3_ttf dependencies installed successfully!"
        ;;

    fedora|rhel|centos|rocky|almalinux)
        echo "Detected Fedora/RHEL-based system"
        sudo dnf install -y \
            gcc \
            gcc-c++ \
            cmake \
            pkgconfig \
            libX11-devel \
            libXext-devel \
            libXrandr-devel \
            libXcursor-devel \
            libXi-devel \
            libXfixes-devel \
            libXrender-devel \
            libXScrnSaver-devel \
            libXtst-devel \
            alsa-lib-devel \
            pulseaudio-libs-devel \
            jack-audio-connection-kit-devel \
            sndio-devel \
            dbus-devel \
            ibus-devel \
            wayland-devel \
            libxkbcommon-devel \
            mesa-libEGL-devel \
            mesa-libGLES-devel \
            libdrm-devel \
            mesa-libgbm-devel \
            freetype-devel \
            harfbuzz-devel
        echo "SDL3 and SDL3_ttf dependencies installed successfully!"
        ;;

    arch|manjaro)
        echo "Detected Arch-based system"
        sudo pacman -S --needed --noconfirm \
            base-devel \
            cmake \
            libx11 \
            libxext \
            libxrandr \
            libxcursor \
            libxi \
            libxfixes \
            libxrender \
            libxss \
            libxtst \
            alsa-lib \
            libpulse \
            jack \
            sndio \
            dbus \
            ibus \
            wayland \
            libxkbcommon \
            mesa \
            freetype2 \
            harfbuzz
        echo "SDL3 and SDL3_ttf dependencies installed successfully!"
        ;;

    opensuse*|sles)
        echo "Detected openSUSE/SLES system"
        sudo zypper install -y \
            gcc \
            gcc-c++ \
            cmake \
            pkg-config \
            libX11-devel \
            libXext-devel \
            libXrandr-devel \
            libXcursor-devel \
            libXi-devel \
            libXfixes-devel \
            libXrender-devel \
            libXss-devel \
            libXtst-devel \
            alsa-devel \
            libpulse-devel \
            libjack-devel \
            dbus-1-devel \
            ibus-devel \
            wayland-devel \
            libxkbcommon-devel \
            Mesa-libEGL-devel \
            Mesa-libGLESv2-devel \
            libdrm-devel \
            libgbm-devel \
            freetype2-devel \
            harfbuzz-devel
        echo "SDL3 and SDL3_ttf dependencies installed successfully!"
        ;;

    *)
        echo "Unsupported distribution: $DISTRO"
        echo "Please install the following packages manually:"
        echo "  - Build tools (gcc, cmake, pkg-config)"
        echo "  - X11 development libraries (including libxtst-dev)"
        echo "  - Audio libraries (ALSA, PulseAudio, JACK)"
        echo "  - Wayland development libraries"
        echo "  - FreeType and HarfBuzz development libraries (for SDL3_ttf)"
        echo ""
        echo "See: https://wiki.libsdl.org/SDL3/README-linux#build-dependencies"
        exit 1
        ;;
esac

echo ""
echo "All dependencies installed! You can now run: cargo build"
