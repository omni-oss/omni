#!/usr/bin/env bash
set -eu
(set -o pipefail 2>/dev/null) || true

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
for i in {1..3}; do
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        response=$(curl -sL -H "Authorization: Bearer $GITHUB_TOKEN" "$latest_url")
    else
        response=$(curl -sL "$latest_url")
    fi

    LATEST_RELEASE=$(echo "$response" | jq -r '.tag_name')

    if [ -n "$LATEST_RELEASE" ] && [ "$LATEST_RELEASE" != "null" ]; then
        break
    fi

    echo "Failed to fetch latest release (attempt $i). Retrying..."
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
if [ -f ~/.omni/bin/omni ]; then
    INSTALLED_VERSION=$(~/.omni/bin/omni --version | cut -d' ' -f2)
    echo "Found installed version: v$INSTALLED_VERSION"
    if [ "$LATEST_RELEASE" == "v$INSTALLED_VERSION" ]; then
        echo "omni is already up to date ($LATEST_RELEASE)."
        exit 0
    fi
fi

echo "Downloading omni $LATEST_RELEASE..."

DOWNLOAD_URL="https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/omni-$LATEST_RELEASE-$TARGET.zip"
mkdir -p ~/.omni/bin
curl -L -o ~/.omni/bin/omni "$DOWNLOAD_URL"

unzip -o ~/.omni/bin/omni -d ~/.omni/bin
chmod +x ~/.omni/bin/omni

# Setup PATH env
if [ -n "${ZSH_VERSION:-}" ] || [ -n "${BASH_VERSION:-}" ]; then
    if [ ! -f ~/.omni/env ]; then
        if [ -n "${ZSH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> ~/.zshrc
        fi
        if [ -n "${BASH_VERSION:-}" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> ~/.bashrc
        fi
    fi
    echo "export PATH=\$HOME/.omni/bin:\$PATH" >| "$HOME/.omni/env"
else
    echo "Unsupported shell. Please add manually:"
    echo "export PATH=\$HOME/.omni/bin:\$PATH"
fi

echo "✅ omni $LATEST_RELEASE has been installed successfully."