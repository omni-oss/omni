#!/bin/sh
set -eu

OWNER="omni-oss"
REPO="omni"
UNAME_S=$(uname -s)
UNAME_M=$(uname -m)
VERSION=${1:-latest}

case "$UNAME_M" in
    x86_64) ARCH="x86_64" ;;
    arm64|aarch64) ARCH="aarch64" ;;
    *) ARCH="$UNAME_M" ;;
esac

case "$UNAME_S" in
    Darwin)
        TARGET="${ARCH}-apple-darwin"
        ;;
    Linux)
        # Only check ldd if it exists (avoids errors on macOS)
        if command -v ldd >/dev/null && ldd /bin/ls | grep -q 'musl'; then
            TARGET="${ARCH}-unknown-linux-musl"
        else
            TARGET="${ARCH}-unknown-linux-gnu"
        fi
        ;;
    *)
        TARGET="${ARCH}-unknown-linux-gnu"
        ;;
esac

echo "Target: $TARGET"

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

    # Remove the omni- prefix from the version if it exists
    TO_INSTALL_VERSION=${TO_INSTALL_VERSION#omni-}

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
OMNI_BIN_DIR="$HOME/.omni/bin"
OMNI_BIN="$OMNI_BIN_DIR/omni"
if [ -f $OMNI_BIN ]; then
    INSTALLED_VERSION=$($OMNI_BIN --version | cut -d' ' -f2)
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

DOWNLOAD_URL="https://github.com/$OWNER/$REPO/releases/download/omni-$TO_INSTALL_VERSION/omni-$TO_INSTALL_VERSION-$TARGET.zip"
mkdir -p $OMNI_BIN_DIR

FILENAME="omni-$TO_INSTALL_VERSION-$TARGET.zip"

TMP="$HOME/.omni/.tmp"

mkdir -p $TMP

# Retry function for downloads
i=1
while [ $i -le 3 ]; do
    if curl -L -o "$TMP/$FILENAME" "$DOWNLOAD_URL"; then
        break
    fi
    echo "Attempt ${i}: Download failed. Retrying in 2 seconds..."
    i=$((i + 1))
    sleep 2
done

# Final validation of download
if [ ! -f "$TMP/$FILENAME" ]; then
    echo "❌ Failed to download file after 3 attempts: $DOWNLOAD_URL"
    exit 1
fi

EXTRACTED_PATH="$TMP/extracted"
unzip -o "$TMP/$FILENAME" -d "$EXTRACTED_PATH"

longest_path=""
longest_len=0

# Use find to locate all files named 'omni'
# We use a while loop to read paths safely line-by-line
find $EXTRACTED_PATH -type f -name omni | while read -r file; do
    # Count characters in the path using wc
    current_len=$(echo "$file" | wc -c)

    # Check if this path is longer than the current maximum
    if [ "$current_len" -gt "$longest_len" ]; then
        longest_len=$current_len
        longest_path=$file

        # Write the current winner to a temporary file because
        # changes inside a while-pipe loop are lost to the parent shell
        echo "$longest_path" > /tmp/omni_longest_path.tmp
    fi
done

# Read the result from the temporary file
if [ -f /tmp/omni_longest_path.tmp ]; then
    final_path=$(cat /tmp/omni_longest_path.tmp)
    rm /tmp/omni_longest_path.tmp

    echo "Found longest path: $final_path"
    echo "Copying to $OMNI_BIN_DIR..."
    cp "$final_path" "$OMNI_BIN_DIR/"
else
    echo "No files matching 'omni' were found."
fi

chmod +x $OMNI_BIN
rm -rf "$TMP"

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
