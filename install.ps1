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

$release = 'https://github.com/TheBadRoger/chulch/releases/latest/download'
$archiveName = 'chulch-windows-x64.zip'
[IO.Directory]::CreateDirectory($InstallDir) | Out-Null
$destination = Join-Path $InstallDir 'chulch.exe'
$staged = Join-Path $InstallDir ("chulch-install-{0}.exe" -f [guid]::NewGuid())
$download = [IO.Path]::GetTempFileName()
try {
    Write-Host 'Downloading the latest chulch release...'
    Invoke-WebRequest -UseBasicParsing -Uri "$release/$archiveName" -OutFile $download
    $checksum = (Invoke-RestMethod -Uri "$release/$archiveName.sha256").Trim()
    if ($checksum -notmatch '^[0-9a-fA-F]{64}$' -or (Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash -ne $checksum) {
        throw 'The download did not match its checksum. Run the installer again.'
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($download)
    try {
        $exe = $zip.GetEntry('chulch.exe')
        if (!$exe) { throw 'The release archive is missing chulch.exe.' }
        [IO.Compression.ZipFileExtensions]::ExtractToFile($exe, $staged)
    } finally { $zip.Dispose() }
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
    [IO.File]::Delete($download)
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
