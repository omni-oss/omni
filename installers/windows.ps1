# Parameter to accept an optional version argument
# parameter should be first in the script
param(
    [string]$Version = "latest"
)

# Exit on first error
$ErrorActionPreference = "Stop"

$OWNER = "omni-oss"
$REPO = "omni"
# Get architecture
$RawArch = $Env:PROCESSOR_ARCHITECTURE

# Map to Rust standards
switch -Wildcard ($RawArch) {
    "AMD64"   { $ARCH = "x86_64" }
    "ARM64"   { $ARCH = "aarch64" }
    "IA64"    { $ARCH = "x86_64" } # Older Itanium systems
    "x86"     { $ARCH = "i686" }
    default   { $ARCH = "x86_64" } # Fallback
}

# Set the TARGET variable
$TARGET = "$ARCH-pc-windows-msvc"

# Export to environment for the current session
$env:TARGET = $TARGET

Write-Host "Target: $env:TARGET"

$BinDir = "$HOME\AppData\Local\omni\bin"
$OmniPath = Join-Path $BinDir "omni.exe"
$UpdateUrl = "https://api.github.com/repos/$OWNER/$REPO/releases/latest"

# Optional GitHub token for higher rate limit
$GitHubToken = $env:GITHUB_TOKEN


# Retry function for API requests
function Get-LatestRelease {
    param(
        [int]$Retries = 3,
        [int]$DelaySeconds = 2
    )

    $Headers = @{ "User-Agent" = "PowerShell" }
    if ($GitHubToken) {
        $Headers["Authorization"] = "Bearer $GitHubToken"
    }

    for ($i = 1; $i -le $Retries; $i++) {
        try {
            $Response = Invoke-RestMethod -Uri $UpdateUrl -Headers $Headers
            if ($Response -and $Response.tag_name) {
                return $Response.tag_name
            }
        } catch {
            Write-Warning "Attempt ${i}: Failed to fetch latest release."
        }
        Start-Sleep -Seconds $DelaySeconds
    }
    throw "Failed to fetch latest release after $Retries attempts."
}

# Determine the version to install
if ($Version -eq "latest") {
    # Fetch the latest version from the GitHub API.
    Write-Output "Checking for latest version..."
    $TO_INSTALL_VERSION = Get-LatestRelease
    Write-Output "Latest version: $TO_INSTALL_VERSION"
} else {
    # Use the specified version.
    # We prepend 'v' if it's not already there to match release tag format.
    if ($Version.StartsWith("v")) {
        $TO_INSTALL_VERSION = $Version
    } else {
        $TO_INSTALL_VERSION = "v$Version"
    }
    Write-Output "Installing specified version: $TO_INSTALL_VERSION"
}

# Check if omni is already installed and matches the target version
if (Test-Path $OmniPath) {
    $INSTALLED_VERSION = & $OmniPath --version | ForEach-Object { ($_ -split ' ')[1] }
    Write-Output "Found installed version: v$INSTALLED_VERSION"

    if ($TO_INSTALL_VERSION -eq "v$INSTALLED_VERSION") {
        if ($VERSION -eq "latest") {
            Write-Output "omni is already update to with the latest version ($TO_INSTALL_VERSION)."
        } else {
            Write-Output "omni is already installed at the specified version ($TO_INSTALL_VERSION)."
        }
        exit 0
    }
}

Write-Output "Downloading omni $TO_INSTALL_VERSION..."

$DOWNLOAD_URL = "https://github.com/$OWNER/$REPO/releases/download/omni-$TO_INSTALL_VERSION/omni-$TO_INSTALL_VERSION-$TARGET.zip"
$FILENAME = "omni-$TO_INSTALL_VERSION-$TARGET.zip"
$ZipFile = Join-Path $BinDir $FILENAME

# Ensure directory exists
New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

# Retry function for downloads
function Invoke-Download {
    param(
        [string]$Uri,
        [string]$OutFile,
        [int]$Retries = 3,
        [int]$DelaySeconds = 2
    )

    for ($i = 1; $i -le $Retries; $i++) {
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing
            return
        } catch {
            Write-Warning "Attempt ${i}: Download failed. Retrying in $DelaySeconds seconds..."
            Start-Sleep -Seconds $DelaySeconds
        }
    }
    throw "Failed to download file after $Retries attempts: $Uri"
}

Invoke-Download -Uri $DOWNLOAD_URL -OutFile $ZipFile

# Extract zip
Expand-Archive -Path $ZipFile -DestinationPath $BinDir -Force
Rename-Item -Path (Join-Path $BinDir "omni-$TO_INSTALL_VERSION-$TARGET") -NewName "omni"
Remove-Item $ZipFile

# Add to PATH (User environment variable)
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not ($CurrentPath -split ";" | Where-Object { $_ -eq $BinDir })) {
    [Environment]::SetEnvironmentVariable("Path", "$BinDir;$CurrentPath", "User")
    Write-Output "Added $BinDir to PATH. Restart your shell or run `$env:Path = '$BinDir;' + $env:Path` to use immediately."
}

Write-Output "✅ omni $TO_INSTALL_VERSION has been installed successfully."
