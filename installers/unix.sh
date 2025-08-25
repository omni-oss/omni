#!/usr/bin/env bash
# Exit immediately if a command exits with a non-zero status
set -e

OWNER="omni-oss"
REPO="omni"

# check if linux or macos
case "$(uname)" in
  Linux*)   TARGET="ubuntu-latest";;
  Darwin*)  TARGET="macos-latest";;
  *)        echo "Unsupported OS. Please install omni manually."; exit 1;;
esac

latest_url="https://api.github.com/repos/$OWNER/$REPO/releases/latest"

echo "Checking for updates... $latest_url"

# Get the latest release version from the GitHub API
LATEST_RELEASE=$(curl -sL "$latest_url" | jq -r '.tag_name')

echo "Latest release: $LATEST_RELEASE"

# Check if the latest release version is already installed
if [ -f ~/.omni/bin/omni ]; then
    # Compare the installed version with the latest release version
    INSTALLED_VERSION=$(~/.omni/bin/omni --version | cut -d' ' -f2)

    echo "Found installed version: $INSTALLED_VERSION"
    # Remove the v prefix from the version string
    if [ "$LATEST_RELEASE" == "v$INSTALLED_VERSION" ]; then
        echo "omni is already up to date ($LATEST_VERSION)."
        exit 0
    fi
fi

echo "Downloading omni $LATEST_RELEASE..."

# Download the latest release asset
DOWNLOAD_URL="https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/omni-$LATEST_RELEASE-$TARGET.zip"
mkdir -p ~/.omni/bin
curl -L -o ~/.omni/bin/omni $DOWNLOAD_URL

# Extract the downloaded asset
unzip -o ~/.omni/bin/omni -d ~/.omni/bin

# Make the binary executable
chmod +x ~/.omni/bin/omni

# Add environment variables to the appropriate files
# check which shell is running
if [ -n "$ZSH_VERSION" ] || [ -n "$BASH_VERSION" ]; then
    # Don't overwrite existing environment variables if env file already exists
    if [ ! -f ~/.omni/env ]; then
        if [ -n "$ZSH_VERSION" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> ~/.zshrc
        fi
        
        if [ -n "$BASH_VERSION" ]; then
            echo "[[ -s \"\$HOME/.omni/env\" ]] && . \"\$HOME/.omni/env\"" >> ~/.bashrc
        fi
    fi

    echo "export PATH=\$HOME/.omni/bin:\$PATH" >| $HOME/.omni/env
else
    echo "Unsupported shell. Please add the following line to your shell's configuration file:"
    echo "export PATH=\$HOME/.omni/bin:\$PATH"
fi

echo "omni $LATEST_RELEASE has been installed successfully."