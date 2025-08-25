# Exit on first error
$ErrorActionPreference = "Stop"

$OWNER = "omni-oss"
$REPO = "omni"
$TARGET = "windows-latest"  # Adjust if you need linux/macos builds

$BinDir = "$HOME\AppData\Local\omni\bin"
$OmniPath = Join-Path $BinDir "omni.exe"


$update_url = "https://api.github.com/repos/$OWNER/$REPO/releases/latest"

Write-Output "Checking latest version..."

# Get the latest release tag from GitHub
$LATEST_RELEASE = (Invoke-RestMethod "$update_url").tag_name

Write-Output "Latest version: $LATEST_RELEASE"

# Check if omni is already installed
if (Test-Path $OmniPath) {
    $INSTALLED_VERSION = & $OmniPath --version | ForEach-Object { ($_ -split ' ')[1] }
    Write-Output "Found installed version: v$INSTALLED_VERSION"

    if ($LATEST_RELEASE -eq "v$INSTALLED_VERSION") {
        Write-Output "omni is already up to date ($LATEST_RELEASE)."
        exit 0
    }
}

Write-Output "Downloading omni $LATEST_RELEASE..."

# Construct download URL
$DOWNLOAD_URL = "https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/omni-$LATEST_RELEASE-$TARGET.zip"
$ZipFile = Join-Path $BinDir "omni.zip"

# Ensure directory exists
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

# Download the release
Invoke-WebRequest -Uri $DOWNLOAD_URL -OutFile $ZipFile

# Extract zip
Expand-Archive -Path $ZipFile -DestinationPath $BinDir -Force
Remove-Item $ZipFile

# Add to PATH (User environment variable)
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not ($CurrentPath -split ";" | Where-Object { $_ -eq $BinDir })) {
    [Environment]::SetEnvironmentVariable("Path", "$BinDir;$CurrentPath", "User")
    Write-Output "Added $BinDir to PATH. Restart your shell or run `$env:Path = '$BinDir;' + $env:Path` to use immediately."
}

Write-Output "omni $LATEST_RELEASE has been installed successfully."