# AGM Installer for Windows
# Usage: irm https://raw.githubusercontent.com/konghayao/peri/main/agm/install.ps1 | iex
#
# Options:
#   $env:AGM_INSTALL_VERSION  Specific version tag (e.g. agm-v0.1.0), empty = latest
#   $env:AGM_INSTALL_DIR      Install directory (default: $env:USERPROFILE\.agm)
#   $env:GITHUB_PROXY         GitHub download proxy prefix (replaces https://github.com)
#   $env:GITHUB_TOKEN         GitHub personal access token (bypasses API rate limiting)
#   $env:AGM_NO_PATH_HINT     Set to 1 to skip PATH hint
#   $env:AGM_INSTALL_PLATFORM Override platform detection (e.g. windows-x86_64)
#
# Example:
#   $env:AGM_INSTALL_VERSION="agm-v0.1.0"; irm ... | iex
#   $env:GITHUB_PROXY="https://ghproxy.com/https://github.com"; irm ... | iex

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# --- Logging ---
function info  { Write-Host "[INFO]  $args" -ForegroundColor Green }
function warn  { Write-Host "[WARN]  $args" -ForegroundColor Yellow }
function error { Write-Host "[ERROR] $args" -ForegroundColor Red }
function step  { Write-Host "[STEP]  $args" -ForegroundColor Cyan }

# --- Platform Detection ---
function Detect-Platform {
    if ($env:AGM_INSTALL_PLATFORM) {
        if ($env:AGM_INSTALL_PLATFORM -notmatch '^(macos|linux|windows)-(x86_64|aarch64|riscv64)$') {
            error "Invalid AGM_INSTALL_PLATFORM: $env:AGM_INSTALL_PLATFORM"
            Write-Host "  Expected: macos-x86_64 | macos-aarch64 | linux-x86_64 | linux-aarch64 | linux-riscv64 | windows-x86_64"
            exit 1
        }
        info "Platform (manual): $env:AGM_INSTALL_PLATFORM"
        return $env:AGM_INSTALL_PLATFORM
    }

    $arch = switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { "x86_64" }
        "ARM64" { "aarch64" }
        default {
            error "Unsupported architecture: $env:PROCESSOR_ARCHITECTURE"
            exit 1
        }
    }

    $platform = "windows-${arch}"
    info "Detected platform: $platform"
    return $platform
}

# --- Download with optional proxy ---
function Get-DownloadUrl {
    param([string]$Url)
    if ($env:GITHUB_PROXY) {
        return $Url -replace 'https://github\.com', $env:GITHUB_PROXY
    }
    return $Url
}

# --- GitHub API request (with optional token) ---
function Invoke-GitHubApi {
    param([string]$Url)
    $headers = @{}
    if ($env:GITHUB_TOKEN) {
        $headers["Authorization"] = "Bearer $env:GITHUB_TOKEN"
    }
    $response = Invoke-RestMethod -Uri $Url -Headers $headers -ErrorAction SilentlyContinue
    return $response
}

# --- Cleanup Old Versions ---
function Clean-OldVersions {
    param([string]$InstallDir, [string]$CurrentVersion)

    $oldDirs = @(Get-ChildItem -Path $InstallDir -Directory | Where-Object {
        $_.Name -match '^agm-v' -and $_.Name -ne $CurrentVersion
    })

    if ($oldDirs.Count -eq 0) {
        info "No old versions to clean up."
        return
    }

    Write-Host ""
    warn "Found $($oldDirs.Count) old version(s):"
    $totalSize = 0
    foreach ($d in $oldDirs) {
        $size = (Get-ChildItem -Path $d.FullName -Recurse -File -ErrorAction SilentlyContinue |
                 Measure-Object -Property Length -Sum).Sum
        if (-not $size) { $size = 0 }
        $totalSize += $size
        $sizeMB = [math]::Round($size / 1MB, 1)
        Write-Host "  $($d.Name)  ($sizeMB MB)"
    }
    $totalMB = [math]::Round($totalSize / 1MB, 1)
    Write-Host "  Total: $totalMB MB"
    Write-Host ""

    $answer = Read-Host "Delete old versions? [y/N]"
    switch ($answer) {
        { $_ -match '^[yY](es)?$' } {
            foreach ($d in $oldDirs) {
                Remove-Item -Recurse -Force $d.FullName
                info "Removed: $($d.Name)"
            }
            info "Cleaned up $($oldDirs.Count) old version(s)."
        }
        default {
            info "Skipped cleanup."
        }
    }
}

# --- Main ---
function Main {
    $InstallDir = if ($env:AGM_INSTALL_DIR) { $env:AGM_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".agm" }
    $GitHubApi = "https://api.github.com/repos/konghayao/peri"

    Write-Host ""
    info "AGM Installer (Windows)"
    info "-------------------------------"

    $Platform = Detect-Platform
    $AssetName = "agm-${Platform}.zip"
    $ExeName = "agm.exe"

    # Fetch release info
    if ($env:AGM_INSTALL_VERSION) {
        $VersionTag = $env:AGM_INSTALL_VERSION
        step "Fetching release: $VersionTag..."
        try {
            $Release = Invoke-GitHubApi "$GitHubApi/releases/tags/$VersionTag"
        } catch {
            error "Failed to fetch release '$VersionTag'. Does this tag exist?"
            exit 1
        }
    } else {
        step "Fetching latest AGM release..."
        try {
            $Releases = Invoke-GitHubApi "$GitHubApi/releases?per_page=30"
        } catch {
            error "Failed to fetch releases from GitHub."
            exit 1
        }

        $VersionTag = ($Releases | Where-Object { $_.tag_name -like "agm-*" } | Select-Object -First 1).tag_name
        if (-not $VersionTag) {
            error "No AGM release found."
            exit 1
        }

        try {
            $Release = Invoke-GitHubApi "$GitHubApi/releases/tags/$VersionTag"
        } catch {
            error "Failed to fetch release '$VersionTag'."
            exit 1
        }
    }

    info "Found release: $VersionTag"

    # Create install directory
    $VersionDir = Join-Path $InstallDir $VersionTag
    $TargetExe = Join-Path $VersionDir $ExeName

    # Check if already installed
    if (Test-Path $TargetExe) {
        if ($env:AGM_FORCE -ne "1") {
            info "Already up to date ($VersionTag). Use AGM_FORCE=1 to reinstall."
            Clean-OldVersions -InstallDir $InstallDir -CurrentVersion $VersionTag
            exit 0
        }
        info "Force reinstall, removing existing..."
        Remove-Item -Recurse -Force $VersionDir -ErrorAction SilentlyContinue
    }

    # Find matching asset
    $Asset = $Release.assets | Where-Object { $_.name -eq $AssetName }
    if (-not $Asset) {
        error "No binary found for platform '$Platform'."
        Write-Host ""
        Write-Host "Available assets:"
        $Release.assets | ForEach-Object { Write-Host "  - $($_.name)" }
        exit 1
    }

    $DownloadUrl = $Asset.browser_download_url
    info "Binary: $AssetName"

    New-Item -ItemType Directory -Force -Path $VersionDir | Out-Null

    $ZipPath = Join-Path $VersionDir $AssetName

    # Download
    $FinalUrl = Get-DownloadUrl $DownloadUrl
    if ($FinalUrl -ne $DownloadUrl) {
        info "Using proxy: $FinalUrl"
    }

    step "Downloading..."
    try {
        Invoke-WebRequest -Uri $FinalUrl -OutFile $ZipPath
    } catch {
        error "Download failed: $_"
        exit 1
    }

    # Extract
    step "Extracting..."
    try {
        Expand-Archive -Path $ZipPath -DestinationPath $VersionDir -Force
    } catch {
        error "Extraction failed: $_"
        exit 1
    }
    Remove-Item -Force $ZipPath -ErrorAction SilentlyContinue

    # Zip contains agm-<platform>.exe, find and rename to agm.exe
    $SourceExe = Get-ChildItem -Path $VersionDir -Recurse -Filter "*.exe" | Where-Object { $_.Name -notlike "unins*" } | Select-Object -First 1
    if (-not $SourceExe) {
        error "No .exe found in extracted archive."
        Get-ChildItem -Path $VersionDir -Recurse | ForEach-Object { Write-Host "  $($_.FullName)" }
        exit 1
    }

    if ($SourceExe.FullName -ne $TargetExe) {
        Move-Item -Force $SourceExe.FullName $TargetExe
    }

    info "Installed to: $TargetExe"

    # Create convenience copy
    $LinkPath = Join-Path $InstallDir $ExeName
    Copy-Item -Force $TargetExe $LinkPath

    # Write current version
    $VersionFile = Join-Path $InstallDir "current-version.txt"
    $VersionTag | Out-File -FilePath $VersionFile -Encoding ascii -NoNewline

    # --- PATH Setup ---
    if ($env:AGM_NO_PATH_HINT -ne "1") {
        $currentPath = [Environment]::GetEnvironmentVariable("Path", "User") -split ";"
        $installPathNormalized = (Resolve-Path $InstallDir).Path.TrimEnd("\")

        $alreadyInPath = $false
        foreach ($p in $currentPath) {
            if ($p.TrimEnd("\") -eq $installPathNormalized) {
                $alreadyInPath = $true
                break
            }
        }

        if (-not $alreadyInPath) {
            [Environment]::SetEnvironmentVariable("Path", "$InstallDir;$([Environment]::GetEnvironmentVariable('Path', 'User'))", "User")
            info "Added $InstallDir to user PATH"
            $env:Path = "$InstallDir;$env:Path"
        }
    }

    # Offer to clean up old versions
    Clean-OldVersions -InstallDir $InstallDir -CurrentVersion $VersionTag

    Write-Host ""
    info "Installation complete! Version: $VersionTag"
    Write-Host ""
    info "Open a new terminal and run 'agm' to start."
    Write-Host ""
}

Main
