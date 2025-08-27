# Exit on first error
$ErrorActionPreference = "Stop"

$OWNER = "omni-oss"
$REPO = "omni"
$TARGET = "windows-latest"

$BinDir = "$HOME\AppData\Local\omni\bin"
$OmniPath = Join-Path $BinDir "omni.exe"
$UpdateUrl = "https://api.github.com/repos/$OWNER/$REPO/releases/latest"

Write-Output "Checking latest version..."

# Retry function for API requests
function Get-LatestRelease {
    param(
        [int]$Retries = 3,
        [int]$DelaySeconds = 2
    )

    for ($i = 1; $i -le $Retries; $i++) {
        try {
            $Response = Invoke-RestMethod -Uri $UpdateUrl -Headers @{ "User-Agent" = "PowerShell" }
            if ($Response -and $Response.tag_name) {
                return $Response.tag_name
            }
        } catch {
            Write-Warning "Attempt $i: Failed to fetch latest release."
        }
        Start-Sleep -Seconds $DelaySeconds
    }
    throw "Failed to fetch latest release after $Retries attempts."
}

$LATEST_RELEASE = Get-LatestRelease
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

$DOWNLOAD_URL = "https://github.com/$OWNER/$REPO/releases/download/$LATEST_RELEASE/omni-$LATEST_RELEASE-$TARGET.zip"
$ZipFile = Join-Path $BinDir "omni.zip"

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
            Write-Warning "Attempt $i: Download failed. Retrying in $DelaySeconds seconds..."
            Start-Sleep -Seconds $DelaySeconds
        }
    }
    throw "Failed to download file after $Retries attempts: $Uri"
}

Invoke-Download -Uri $DOWNLOAD_URL -OutFile $ZipFile

# Extract zip
Expand-Archive -Path $ZipFile -DestinationPath $BinDir -Force
Remove-Item $ZipFile

# Add to PATH (User environment variable)
$CurrentPath = [Environment]::GetEnvironmentVariable("Path", "User")
if (-not ($CurrentPath -split ";" | Where-Object { $_ -eq $BinDir })) {
    [Environment]::SetEnvironmentVariable("Path", "$BinDir;$CurrentPath", "User")
    Write-Output "Added $BinDir to PATH. Restart your shell or run `$env:Path = '$BinDir;' + $env:Path` to use immediately."
}

Write-Output "✅ omni $LATEST_RELEASE has been installed successfully."