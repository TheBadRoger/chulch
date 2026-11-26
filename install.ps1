#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\chulch\bin'),
    [switch]$NoPath
)

$ErrorActionPreference = 'Stop'
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($env:OS -ne 'Windows_NT' -or $architecture -ne 'AMD64') {
    throw 'chulch requires Windows x64.'
}
$InstallDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($InstallDir)
if (!$NoPath -and $InstallDir.Contains(';')) { throw 'A PATH entry cannot contain a semicolon. Choose another directory or use -NoPath.' }

$source = Join-Path $PSScriptRoot 'chulch.exe'
if (Test-Path -LiteralPath (Join-Path $PSScriptRoot 'Cargo.toml') -PathType Leaf) {
    if (!(Get-Command cargo.exe -CommandType Application -ErrorAction SilentlyContinue) -or
        !(Get-Command rustc.exe -CommandType Application -ErrorAction SilentlyContinue)) {
        throw 'Building from source requires Rust and Cargo. See docs/install.md or use the Windows x64 ZIP.'
    }
    Push-Location -LiteralPath $PSScriptRoot
    try {
        $hostLine = rustc.exe -vV | Select-String '^host: (x86_64-pc-windows-\S+)$'
        if ($LASTEXITCODE -ne 0 -or !$hostLine) { throw 'Select an x64 Windows Rust toolchain to build chulch.' }
        $triple = $hostLine.Matches[0].Groups[1].Value
        cargo.exe build --locked --release --target $triple --target-dir target
        if ($LASTEXITCODE -ne 0) { throw "Cargo build failed (exit $LASTEXITCODE)." }
        $source = Join-Path $PSScriptRoot "target\$triple\release\chulch.exe"
    } finally { Pop-Location }
}
if (!$source -or !(Test-Path -LiteralPath $source -PathType Leaf)) {
    throw 'Executable missing. Extract the entire Windows x64 ZIP or run the script from the project source.'
}
[IO.Directory]::CreateDirectory($InstallDir) | Out-Null
$destination = Join-Path $InstallDir 'chulch.exe'
$staged = Join-Path $InstallDir ("chulch-install-{0}.exe" -f [guid]::NewGuid())
try {
    [IO.File]::Copy($source, $staged)
    $version = & $staged --version
    if ($LASTEXITCODE -ne 0 -or $version -notmatch '^chulch ') { throw 'The executable failed its version check.' }
    if ([IO.File]::Exists($destination)) {
        if ((Get-FileHash -LiteralPath $destination).Hash -ne (Get-FileHash -LiteralPath $staged).Hash) {
            [IO.File]::Replace($staged, $destination, [System.Management.Automation.Language.NullString]::Value)
        }
    } else {
        [IO.File]::Move($staged, $destination)
    }
} finally {
    if ([IO.File]::Exists($staged)) { [IO.File]::Delete($staged) }
}

function Add-InstallDirectory([string]$pathValue) {
    if (!$pathValue) { return $InstallDir }
    $others = @(foreach ($entry in ($pathValue -split ';')) {
        if ([Environment]::ExpandEnvironmentVariables($entry.Trim('"')).TrimEnd('\') -ine $InstallDir.TrimEnd('\')) { $entry }
    })
    (@($InstallDir) + $others) -join ';'
}
if (!$NoPath) {
    $key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Environment')
    try {
        $raw = $key.GetValue('Path', '', [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $updated = Add-InstallDirectory $raw
        if ($raw -cne $updated) {
            $kind = if ($raw) { $key.GetValueKind('Path') } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
            $key.SetValue('Path', $updated, $kind)
        }
    } finally { $key.Dispose() }
    $env:Path = Add-InstallDirectory $env:Path
}
Write-Output "Installed $version at $destination"
