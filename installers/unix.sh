#!/bin/sh
set -eu

OWNER="omni-oss"
REPO="omni"

# Parameter to accept an optional version argument, defaults to 'latest'
VERSION="${1:-latest}"

# check if linux or macos
case "$(uname)" in
Linux*) TARGET="ubuntu-latest" ;;
Darwin*) TARGET="macos-latest" ;;
*)
    echo "Unsupported OS. Please install omni manually."
    exit 1
    ;;
esac

latest_url="https://api.github.com/repos/$OWNER/$REPO/releases/latest"

# Determine the version to install
if [ "$VERSION" = "latest" ]; then
    echo "Checking for latest version..."
    # Try up to 3 times to fetch the latest release
    TO_INSTALL_VERSION=""
    i=1
    while [ $i -le 3 ]; do
        if [ -n "${GITHUB_TOKEN:-}" ]; then
            response=$(curl -sL -H "Authorization: Bearer $GITHUB_TOKEN" "$latest_url") || response=""
        else
            response=$(curl -sL "$latest_url") || response=""
        fi

        if [ -n "$response" ]; then
            TO_INSTALL_VERSION=$(printf '%s' "$response" | jq -r '.tag_name') || TO_INSTALL_VERSION=""
        fi

        if [ -n "$TO_INSTALL_VERSION" ] && [ "$TO_INSTALL_VERSION" != "null" ]; then
            break
        fi

        echo "Failed to fetch latest release (attempt $i). Retrying..."
        i=$((i + 1))
        sleep 2
    done

    # Final validation
    if [ -z "$TO_INSTALL_VERSION" ] || [ "$TO_INSTALL_VERSION" = "null" ]; then
        echo "❌ Could not fetch latest release. Full response:"
        echo "$response"
        exit 1
    fi

    echo "Latest version: $TO_INSTALL_VERSION"
else
    # Use the specified version. Prepend 'v' if it's not already there.
    case "$VERSION" in
    v*) TO_INSTALL_VERSION=$VERSION ;;
    *) TO_INSTALL_VERSION="v$VERSION" ;;
    esac
    echo "Installing specified version: $TO_INSTALL_VERSION"
fi

# Check if omni is already installed and matches the target version
if [ -f "$HOME/.omni/bin/omni" ]; then
    INSTALLED_VERSION=$("$HOME/.omni/bin/omni" --version | cut -d' ' -f2)
    echo "Found installed version: v$INSTALLED_VERSION"

    if [ "$TO_INSTALL_VERSION" = "v$INSTALLED_VERSION" ]; then
        if [ "$VERSION" = "latest" ]; then
            echo "omni is already up to date ($TO_INSTALL_VERSION)."
        else
            echo "omni is already installed at the specified version ($TO_INSTALL_VERSION)."
        fi
        exit 0
    fi
fi

echo "Downloading omni $TO_INSTALL_VERSION..."

DOWNLOAD_URL="https://github.com/$OWNER/$REPO/releases/download/$TO_INSTALL_VERSION/omni-$TO_INSTALL_VERSION-$TARGET.zip"
mkdir -p "$HOME/.omni/bin"

# Retry function for downloads
i=1
while [ $i -le 3 ]; do
    if curl -L -o "$HOME/.omni/bin/omni.zip" "$DOWNLOAD_URL"; then
        break
    fi
    echo "Attempt ${i}: Download failed. Retrying in 2 seconds..."
    i=$((i + 1))
    sleep 2
done

# Final validation of download
if [ ! -f "$HOME/.omni/bin/omni.zip" ]; then
    echo "❌ Failed to download file after 3 attempts: $DOWNLOAD_URL"
    exit 1
fi

unzip -o "$HOME/.omni/bin/omni.zip" -d "$HOME/.omni/bin"
chmod +x "$HOME/.omni/bin/omni"
rm "$HOME/.omni/bin/omni.zip"

# Setup PATH env
if [ -n "${ZSH_VERSION:-}" ] || [ -n "${BASH_VERSION:-}" ]; then
    if [ ! -f "$HOME/.omni/env" ]; then
        if [ -n "${ZSH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >>"$HOME/.zshrc"
        fi
        if [ -n "${BASH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >>"$HOME/.bashrc"
        fi
    fi
    echo "export PATH=\$HOME/.omni/bin:\$PATH" >"$HOME/.omni/env"
else
    echo "Unsupported shell. Please add manually:"
    echo "export PATH=\$HOME/.omni/bin:\$PATH"
fi

echo "✅ omni $TO_INSTALL_VERSION has been installed successfully."
