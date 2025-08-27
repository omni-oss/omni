#!/bin/sh
set -eu

OWNER="omni-oss"
REPO="omni"

# check if linux or macos
case "$(uname)" in
  Linux*)   TARGET="ubuntu-latest";;
  Darwin*)  TARGET="macos-latest";;
  *)        echo "Unsupported OS. Please install omni manually."; exit 1;;
esac

latest_url="https://api.github.com/repos/$OWNER/$REPO/releases/latest"

echo "Checking latest version..."

# Try up to 3 times to fetch the latest release
LATEST_RELEASE=""
i=1
while [ $i -le 3 ]; do
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        response=$(curl -sL -H "Authorization: Bearer $GITHUB_TOKEN" "$latest_url") || response=""
    else
        response=$(curl -sL "$latest_url") || response=""
    fi

    if [ -n "$response" ]; then
        LATEST_RELEASE=$(printf '%s' "$response" | jq -r '.tag_name') || LATEST_RELEASE=""
    fi

    if [ -n "$LATEST_RELEASE" ] && [ "$LATEST_RELEASE" != "null" ]; then
        break
    fi

    echo "Failed to fetch latest release (attempt $i). Retrying..."
    i=$((i+1))
    sleep 2
done

# Final validation
if [ -z "$LATEST_RELEASE" ] || [ "$LATEST_RELEASE" = "null" ]; then
    echo "❌ Could not fetch latest release. Full response:"
    echo "$response"
    exit 1
fi

echo "Latest version: $LATEST_RELEASE"

# Check if the latest release version is already installed
if [ -f "$HOME/.omni/bin/omni" ]; then
    INSTALLED_VERSION=$("$HOME/.omni/bin/omni" --version | cut -d' ' -f2)
    echo "Found installed version: v$INSTALLED_VERSION"
    if [ "$LATEST_RELEASE" = "v$INSTALLED_VERSION" ]; then
        echo "omni is already up to date ($LATEST_RELEASE)."
        exit 0
    fi
fi

echo "Downloading omni $LATEST_RELEASE..."

DOWNLOAD_URL="https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/omni-$LATEST_RELEASE-$TARGET.zip"
mkdir -p "$HOME/.omni/bin"
curl -L -o "$HOME/.omni/bin/omni" "$DOWNLOAD_URL"

unzip -o "$HOME/.omni/bin/omni" -d "$HOME/.omni/bin"
chmod +x "$HOME/.omni/bin/omni"

# Setup PATH env
if [ -n "${ZSH_VERSION:-}" ] || [ -n "${BASH_VERSION:-}" ]; then
    if [ ! -f "$HOME/.omni/env" ]; then
        if [ -n "${ZSH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> "$HOME/.zshrc"
        fi
        if [ -n "${BASH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> "$HOME/.bashrc"
        fi
    fi
    echo "export PATH=\$HOME/.omni/bin:\$PATH" > "$HOME/.omni/env"
else
    echo "Unsupported shell. Please add manually:"
    echo "export PATH=\$HOME/.omni/bin:\$PATH"
fi

echo "✅ omni $LATEST_RELEASE has been installed successfully."