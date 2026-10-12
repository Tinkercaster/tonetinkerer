# One-line installer for Windows (PowerShell):
#
#   irm https://raw.githubusercontent.com/Tinkercaster/tonetinkerer/main/scripts/install.ps1 | iex
#
# Downloads the latest release and installs the VST3 and CLAP plugins into
# C:\Program Files\Common Files\VST3 and \CLAP (the standard plugin folders).
# Writing there needs administrator rights, so Windows asks once.
#
# Option: $env:TONETINKERER_VERSION = "v0.1.0" installs that release.

$ErrorActionPreference = "Stop"
$repo = "Tinkercaster/tonetinkerer"
$self = "https://raw.githubusercontent.com/$repo/main/scripts/install.ps1"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    Write-Host "Asking for administrator rights to install into Program Files..."
    $version = $env:TONETINKERER_VERSION
    $cmd = "`$env:TONETINKERER_VERSION='$version'; irm '$self' | iex; Read-Host 'Press Enter to close'"
    Start-Process powershell -Verb RunAs -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd
    return
}

$version = if ($env:TONETINKERER_VERSION) { $env:TONETINKERER_VERSION } else { "latest" }
$asset = "ToneTinkerer-Windows.zip"
$url = if ($version -eq "latest") {
    "https://github.com/$repo/releases/latest/download/$asset"
} else {
    "https://github.com/$repo/releases/download/$version/$asset"
}

$tmp = Join-Path $env:TEMP ("tonetinkerer-" + [guid]::NewGuid())
New-Item -ItemType Directory $tmp | Out-Null
try {
    Write-Host "Downloading ToneTinkerer ($version)..."
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp $asset) -UseBasicParsing
    Expand-Archive -Path (Join-Path $tmp $asset) -DestinationPath $tmp
    $src = Join-Path $tmp "ToneTinkerer-Windows"

    $vst3 = Join-Path $env:CommonProgramFiles "VST3"
    $clap = Join-Path $env:CommonProgramFiles "CLAP"
    New-Item -ItemType Directory -Force $vst3, $clap | Out-Null
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $vst3 "ToneTinkerer.vst3"), (Join-Path $clap "ToneTinkerer.clap")
    Copy-Item -Recurse (Join-Path $src "ToneTinkerer.vst3") $vst3
    Copy-Item (Join-Path $src "ToneTinkerer.clap") $clap
    # Files downloaded from the internet carry a "blocked" mark; clear it.
    Get-ChildItem -Recurse (Join-Path $vst3 "ToneTinkerer.vst3"), (Join-Path $clap "ToneTinkerer.clap") | Unblock-File

    Write-Host ""
    Write-Host "Installed:"
    Write-Host "  $vst3\ToneTinkerer.vst3"
    Write-Host "  $clap\ToneTinkerer.clap"
    Write-Host ""
    Write-Host "Restart your DAW (or rescan plugins) and look for ToneTinkerer by GuitarWiring."
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
}
