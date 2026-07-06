# Peri Agent Installer for Windows (Local Build Edition)
# Usage: cd peri; .\scripts\install.ps1
#
# This fork builds Peri from source instead of downloading pre-built binaries.
# Prerequisites: Rust toolchain (cargo), Git
#
# Options:
#   $env:PERI_INSTALL_DIR       Install directory (default: $env:USERPROFILE\.peri)
#   $env:PERI_NO_PATH_HINT      Set to 1 to skip PATH hint
#
# Example:
#   cd peri; .\scripts\install.ps1
#   $env:PERI_INSTALL_DIR="C:\Tools\peri"; .\scripts\install.ps1

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

# --- Logging ---
function info  { Write-Host "[INFO]  $args" -ForegroundColor Green }
function warn  { Write-Host "[WARN]  $args" -ForegroundColor Yellow }
function error { Write-Host "[ERROR] $args" -ForegroundColor Red }
function step  { Write-Host "[STEP]  $args" -ForegroundColor Cyan }

# --- Cleanup Old Versions ---
function Clean-OldVersions {
    param([string]$InstallDir, [string]$CurrentVersion)

    # Collect agent-v* directories, excluding current version
    $oldDirs = @(Get-ChildItem -Path $InstallDir -Directory | Where-Object {
        $_.Name -match '^agent-v' -and $_.Name -ne $CurrentVersion
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
    $InstallDir = if ($env:PERI_INSTALL_DIR) { $env:PERI_INSTALL_DIR } else { Join-Path $env:USERPROFILE ".peri" }
    $ExeName = "peri.exe"

    Write-Host ""
    info "Peri Agent Installer (Windows, Local Build)"
    info "-------------------------------"

    # Check prerequisites
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        error "cargo not found. Please install Rust: https://rustup.rs"
        exit 1
    }

    # Determine version from git
    $VersionTag = "local-$(Get-Date -Format 'yyyyMMdd')"
    if (Get-Command git -ErrorAction SilentlyContinue) {
        try {
            $gitOutput = git describe --tags --always 2>$null
            if ($LASTEXITCODE -eq 0 -and $gitOutput) {
                $VersionTag = $gitOutput
            }
        } catch {}
    }
    info "Build version: $VersionTag"

    # Build
    step "Building peri from source..."
    cargo build -p peri-tui --release
    if ($LASTEXITCODE -ne 0) {
        error "Build failed."
        exit 1
    }

    $BinaryPath = "target\release\$ExeName"
    if (-not (Test-Path $BinaryPath)) {
        error "Binary not found at $BinaryPath"
        exit 1
    }

    # Create install directory
    $VersionDir = Join-Path $InstallDir $VersionTag
    New-Item -ItemType Directory -Force -Path $VersionDir | Out-Null

    $TargetExe = Join-Path $VersionDir $ExeName

    # Copy binary
    step "Installing..."
    Copy-Item -Force $BinaryPath $TargetExe
    info "Installed to: $TargetExe"

    # Create convenience copy (Windows doesn't support symlinks without admin)
    $LinkPath = Join-Path $InstallDir $ExeName
    Copy-Item -Force $TargetExe $LinkPath

    # Write current version
    $VersionFile = Join-Path $InstallDir "current-version.txt"
    $VersionTag | Out-File -FilePath $VersionFile -Encoding ascii -NoNewline

    # --- PATH Setup ---
    if ($env:PERI_NO_PATH_HINT -ne "1") {
        $currentPath = [Environment]::GetEnvironmentVariable("Path", "User") -split ";"
        $installPathNormalized = (Resolve-Path $InstallDir).Path.TrimEnd("\")

        # Check if install dir is already in PATH (case-insensitive)
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

            # Refresh current session's PATH
            $env:Path = "$InstallDir;$env:Path"
        }
    }

    # Offer to clean up old versions
    Clean-OldVersions -InstallDir $InstallDir -CurrentVersion $VersionTag

    Write-Host ""
    info "Installation complete! Version: $VersionTag"
    Write-Host ""
    info "Open a new terminal and run 'peri' to start."
    Write-Host ""
}

Main
