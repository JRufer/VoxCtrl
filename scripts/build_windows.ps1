#Requires -Version 5.1
<#
.SYNOPSIS
    Build VoxCtrl for Windows.

.PARAMETER Cuda
    Enable CUDA GPU acceleration (requires NVIDIA GPU + CUDA Toolkit).

.PARAMETER Debug
    Build in debug mode (faster compile, larger binary, no optimisation).

.PARAMETER SkipNodeInstall
    Skip 'npm install' (use when deps are already installed).

.EXAMPLE
    .\build_windows.ps1
    .\build_windows.ps1 -Cuda
    .\build_windows.ps1 -Debug
#>
[CmdletBinding()]
param(
    [switch]$Cuda,
    [switch]$Debug,
    [switch]$SkipNodeInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# ── Helpers ───────────────────────────────────────────────────────────────────

function Write-Step([string]$msg) {
    Write-Host "`n==> $msg" -ForegroundColor Cyan
}

function Write-Ok([string]$msg) {
    Write-Host "    [OK] $msg" -ForegroundColor Green
}

function Write-Fail([string]$msg) {
    Write-Host "    [FAIL] $msg" -ForegroundColor Red
    exit 1
}

function Require-Command([string]$cmd, [string]$hint) {
    if (-not (Get-Command $cmd -ErrorAction SilentlyContinue)) {
        Write-Fail "$cmd not found. $hint"
    }
    Write-Ok "$cmd found"
}

# ── Prerequisite checks ───────────────────────────────────────────────────────

Write-Step "Checking prerequisites"

Require-Command "cargo"  "Install Rust from https://rustup.rs/"
Require-Command "node"   "Install Node.js 18+ from https://nodejs.org/"
Require-Command "npm"    "Install Node.js 18+ from https://nodejs.org/"

# Verify MSVC toolchain
$rustTarget = (rustup show active-toolchain 2>$null) -replace '\s.*',''
if ($rustTarget -notlike "*windows-msvc*") {
    Write-Host "    [WARN] Active toolchain '$rustTarget' is not MSVC." -ForegroundColor Yellow
    Write-Host "           Run: rustup default stable-x86_64-pc-windows-msvc" -ForegroundColor Yellow
}
Write-Ok "Rust toolchain: $rustTarget"

# Verify tauri-cli
if (-not (Get-Command "cargo-tauri" -ErrorAction SilentlyContinue)) {
    Write-Host "    tauri-cli not found — installing..." -ForegroundColor Yellow
    cargo install tauri-cli
}
Write-Ok "tauri-cli found"

# CUDA check
if ($Cuda) {
    if (-not $env:CUDA_PATH) {
        $cudaDirs = Get-ChildItem "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA" `
            -ErrorAction SilentlyContinue | Sort-Object Name -Descending
        if ($cudaDirs) {
            $env:CUDA_PATH = $cudaDirs[0].FullName
            Write-Host "    Auto-detected CUDA at: $($env:CUDA_PATH)" -ForegroundColor Yellow
        } else {
            Write-Fail "CUDA_PATH not set and no CUDA Toolkit found. Install from https://developer.nvidia.com/cuda-downloads"
        }
    }
    Write-Ok "CUDA_PATH = $env:CUDA_PATH"
}

# ── Frontend ──────────────────────────────────────────────────────────────────

if (-not $SkipNodeInstall) {
    Write-Step "Installing frontend dependencies"
    npm install
    if ($LASTEXITCODE -ne 0) { Write-Fail "npm install failed" }
    Write-Ok "npm install complete"
}

# ── Build ─────────────────────────────────────────────────────────────────────

Write-Step "Building VoxCtrl"

$tauriArgs = @()

if ($Debug) {
    $tauriArgs += "--debug"
}

if ($Cuda) {
    $tauriArgs += "--features"
    $tauriArgs += "cuda"
}

Write-Host "    Running: npm run tauri build $($tauriArgs -join ' ')" -ForegroundColor DarkGray

if ($tauriArgs.Count -gt 0) {
    npm run tauri build -- @tauriArgs
} else {
    npm run tauri build
}

if ($LASTEXITCODE -ne 0) { Write-Fail "Build failed" }

# ── Report ────────────────────────────────────────────────────────────────────

Write-Step "Build complete"

$bundleDir = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
if ($Debug) {
    $bundleDir = Join-Path $PSScriptRoot "..\src-tauri\target\debug\bundle"
}

if (Test-Path $bundleDir) {
    $artifacts = Get-ChildItem $bundleDir -Recurse -Include "*.exe","*.msi" |
        Where-Object { $_.Name -notlike "*-setup-stub*" }
    foreach ($a in $artifacts) {
        $size = "{0:N1} MB" -f ($a.Length / 1MB)
        Write-Host "    $($a.FullName) ($size)" -ForegroundColor Green
    }
} else {
    Write-Host "    Bundle directory not found at $bundleDir" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Done." -ForegroundColor Cyan
