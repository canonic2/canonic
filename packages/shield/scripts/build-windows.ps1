# Build an AMD64 or ARM64 distribution binary with static CRT and neutral diagnostic paths.
# Run from a Visual Studio developer terminal using Windows PowerShell 5.1.
#Requires -Version 5.1
param([ValidateSet('x64', 'arm64')][string]$Architecture = 'x64')

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'This build requires native Windows and the MSVC toolchain.' }
if ($PSVersionTable.PSEdition -ne 'Desktop') { throw 'Run this script with Windows PowerShell 5.1 (powershell.exe).' }
$triple = if ($Architecture -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$alias = if ($Architecture -eq 'arm64') { 'build-windows-arm64' } else { 'build-windows' }
$machine = if ($Architecture -eq 'arm64') { 0xaa64 } else { 0x8664 }
$developerArchitectures = if ($Architecture -eq 'arm64') { @('arm64') } else { @('x64', 'amd64') }
if ($env:VSCMD_ARG_TGT_ARCH -notin $developerArchitectures) {
    throw "Use an MSVC developer terminal targeting $Architecture (VsDevCmd -arch=$Architecture)."
}
foreach ($name in @('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS', 'CARGO_TARGET_AARCH64_PC_WINDOWS_MSVC_RUSTFLAGS')) {
    if ([Environment]::GetEnvironmentVariable($name)) {
        throw "Clear $name before building the Windows distribution; it overrides the verified compiler flags."
    }
}
$packageDirectory = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$sourceDirectory = Split-Path (Split-Path $packageDirectory)
$profileDirectory = [Environment]::GetFolderPath('UserProfile')
if (!$profileDirectory) { throw 'The native publisher profile could not be resolved.' }
$cargoDirectory = Join-Path $profileDirectory '.cargo'
if ($env:CARGO_HOME) {
    $cargoDirectory = $env:CARGO_HOME
    if (![IO.Path]::IsPathRooted($cargoDirectory)) { $cargoDirectory = Join-Path $packageDirectory $cargoDirectory }
    $cargoDirectory = [IO.Path]::GetFullPath($cargoDirectory)
}

# Later mappings win: the precise source/cache mappings override the profile fallback.
$flags = @()
foreach ($mapping in @(
    @($profileDirectory, '/publisher'),
    @($sourceDirectory, '/canonic'),
    @($cargoDirectory, '/cargo')
)) {
    foreach ($prefix in @($mapping[0], $mapping[0].Replace('\', '/'), "\\?\$($mapping[0])")) {
        $flags += "--remap-path-prefix=$prefix=$($mapping[1])"
    }
}
$configuration = "[target.$triple]`nrustflags = $(ConvertTo-Json -Compress -InputObject $flags)`n"
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("shield-release-" + [Guid]::NewGuid().ToString('N'))
$security = [Security.AccessControl.DirectorySecurity]::new()
$security.SetAccessRuleProtection($true, $false)
$operatorSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$security.SetOwner($operatorSid)
foreach ($sid in @($operatorSid.Value, 'S-1-5-18', 'S-1-5-32-544')) {
    $security.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
        [Security.Principal.SecurityIdentifier]::new($sid),
        [Security.AccessControl.FileSystemRights]::FullControl,
        [Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit',
        [Security.AccessControl.PropagationFlags]::None,
        [Security.AccessControl.AccessControlType]::Allow))
}
[IO.Directory]::CreateDirectory($temporary, $security) | Out-Null
try {
    $path = Join-Path $temporary 'release.toml'
    [IO.File]::WriteAllText($path, $configuration, [Text.UTF8Encoding]::new($false))
    Push-Location -LiteralPath $packageDirectory
    try {
        $ErrorActionPreference = 'Continue'
        & cargo $alias --config $path
        $buildExit = $LASTEXITCODE
        $ErrorActionPreference = 'Stop'
        if ($buildExit -ne 0) { throw "Windows distribution build failed ($buildExit)." }
        $binary = Join-Path $packageDirectory "target\$triple\release\shield.exe"
        if ($env:CARGO_TARGET_DIR) {
            $targetDirectory = $env:CARGO_TARGET_DIR
            if (![IO.Path]::IsPathRooted($targetDirectory)) { $targetDirectory = Join-Path $packageDirectory $targetDirectory }
            $binary = Join-Path $targetDirectory "$triple\release\shield.exe"
        }
        $bytes = [IO.File]::ReadAllBytes($binary)
        if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
            throw 'The Windows build did not produce a PE image.'
        }
        $header = [BitConverter]::ToUInt32($bytes, 0x3c)
        if ($header -gt $bytes.Length - 26 -or [BitConverter]::ToUInt32($bytes, $header) -ne 0x4550 -or
            [BitConverter]::ToUInt16($bytes, $header + 4) -ne $machine -or
            [BitConverter]::ToUInt16($bytes, $header + 24) -ne 0x20b) {
            throw "The Windows build has the wrong PE architecture for $triple."
        }
        # Inspect UTF-8 and either alignment of UTF-16 strings without printing
        # publisher paths. Compiler remapping must also hold in the final image.
        foreach ($encoded in @(
            [Text.Encoding]::UTF8.GetString($bytes),
            [Text.Encoding]::Unicode.GetString($bytes),
            [Text.Encoding]::Unicode.GetString($bytes, 1, $bytes.Length - 1)
        )) {
            foreach ($prefix in @($profileDirectory, $sourceDirectory, $cargoDirectory)) {
                foreach ($spelling in @($prefix, $prefix.Replace('\', '/'))) {
                    if ($encoded.IndexOf($spelling, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
                        throw 'The Windows executable contains a publisher path; do not distribute it.'
                    }
                }
            }
        }
        Write-Host "Windows $triple distribution build passed architecture and publisher-path inspection."
    } finally { Pop-Location }
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }
