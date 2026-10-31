# Package an existing Windows build; this script does not build, sign or publish it.
#Requires -Version 5.1
param(
    [ValidateSet('x64', 'arm64')][string]$Architecture = 'x64',
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '..\dist')
)

$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT' -or $PSVersionTable.PSEdition -ne 'Desktop') {
    throw 'Run with Windows PowerShell 5.1 on Windows.'
}
$package = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$triple = if ($Architecture -eq 'arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
$machine = if ($Architecture -eq 'arm64') { 0xaa64 } else { 0x8664 }
$targetDirectory = Join-Path $package 'target'
if ($env:CARGO_TARGET_DIR) {
    $targetDirectory = $env:CARGO_TARGET_DIR
    if (![IO.Path]::IsPathRooted($targetDirectory)) { $targetDirectory = Join-Path $package $targetDirectory }
}
$binary = Join-Path $targetDirectory "$triple\release\shield.exe"
$bytes = [IO.File]::ReadAllBytes($binary)
if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) { throw 'Not a PE image.' }
$header = [BitConverter]::ToUInt32($bytes, 0x3c)
if ($header -gt $bytes.Length - 26 -or [BitConverter]::ToUInt32($bytes, $header) -ne 0x4550 -or
    [BitConverter]::ToUInt16($bytes, $header + 4) -ne $machine -or
    [BitConverter]::ToUInt16($bytes, $header + 24) -ne 0x20b) {
    throw "The executable does not match $triple."
}
$binaryHash = (Get-FileHash -Algorithm SHA256 $binary).Hash.ToLowerInvariant()
$stage = Join-Path ([IO.Path]::GetTempPath()) ('shield-package-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stage | Out-Null
$script:noticeBytes = 0L
$script:noticeCount = 0
function ReadToolOutput([string]$Tool, [string[]]$Arguments) {
    # Rust tools emit UTF-8 when redirected. Decode it explicitly rather than
    # using PowerShell 5.1's console code page for publisher/cache paths.
    $capture = Join-Path $stage ([Guid]::NewGuid().ToString('N') + '.json')
    try {
        $process = Start-Process -FilePath (Get-Command $Tool -CommandType Application).Source `
            -ArgumentList $Arguments -WorkingDirectory $package -NoNewWindow -Wait -PassThru `
            -RedirectStandardOutput $capture
        if ($process.ExitCode -ne 0) { throw "Pinned $Tool metadata failed ($($process.ExitCode))." }
        return [IO.File]::ReadAllText($capture, [Text.Encoding]::UTF8).TrimEnd([char[]]"`r`n")
    } finally {
        if (Test-Path -LiteralPath $capture) { Remove-Item -LiteralPath $capture -Force }
    }
}
function CopyNotice([string]$Source, [string]$Relative) {
    $file = Get-Item -LiteralPath $Source
    if ($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'A notice must be an ordinary file.'
    }
    $script:noticeBytes += $file.Length
    $script:noticeCount++
    if ($file.Length -gt 8388608 -or $script:noticeBytes -gt 67108864 -or $script:noticeCount -gt 4096) {
        throw 'Notice size bound exceeded.'
    }
    $destination = Join-Path $stage $Relative
    New-Item -ItemType Directory -Force -Path (Split-Path $destination) | Out-Null
    Copy-Item -LiteralPath $file.FullName -Destination $destination
    return $Relative.Replace('\', '/')
}
try {
    $metadata = (ReadToolOutput 'cargo' @('metadata', '--locked', '--offline', '--format-version', '1', '--filter-platform', $triple)) | ConvertFrom-Json
    $sysroot = ReadToolOutput 'rustc' @('--print', 'sysroot')
    $compiler = ReadToolOutput 'rustc' @('--version')
    $packages = @{}
    foreach ($item in $metadata.packages) { $packages[$item.id] = $item }
    $nodes = @{}
    foreach ($node in $metadata.resolve.nodes) { $nodes[$node.id] = $node }
    $seen = @{}
    $pending = [Collections.Generic.Queue[string]]::new()
    $pending.Enqueue($metadata.resolve.root)
    while ($pending.Count) {
        $id = $pending.Dequeue()
        if ($seen.ContainsKey($id)) { continue }
        $seen[$id] = $true
        if ($seen.Count -gt 2048) { throw 'Unexpected dependency graph size.' }
        foreach ($dependency in $nodes[$id].deps) {
            if (@($dependency.dep_kinds | Where-Object { $_.kind -ne 'dev' }).Count) { $pending.Enqueue($dependency.pkg) }
        }
    }
    $version = $packages[$metadata.resolve.root].version
    $archiveName = "shield-$version-$triple.zip"
    $output = [IO.Path]::GetFullPath($OutputDirectory)
    New-Item -ItemType Directory -Force -Path $output | Out-Null
    $archive = Join-Path $output $archiveName
    if ((Test-Path -LiteralPath $archive) -or (Test-Path -LiteralPath ($archive + '.sha256'))) {
        throw 'Output already exists; choose a new output directory.'
    }
    Copy-Item -LiteralPath $binary -Destination (Join-Path $stage 'shield.exe')
    if ((Get-FileHash -Algorithm SHA256 (Join-Path $stage 'shield.exe')).Hash.ToLowerInvariant() -ne $binaryHash) {
        throw 'Executable changed while packaging.'
    }
    Copy-Item -LiteralPath (Join-Path $package 'LICENSE') -Destination (Join-Path $stage 'LICENSE')
    $rows = @()
    $firstParty = @()
    foreach ($id in ($seen.Keys | Sort-Object)) {
        $item = $packages[$id]
        if ($item.name -in @('canonic-shield', 'canonic-mcp-kit', 'canonic-registry-kit', 'canonic-tui-kit') -and !$item.source) {
            $firstParty += "$($item.name) $($item.version)"
            continue
        }
        if (!$item.source -or !$item.source.StartsWith('registry+')) { throw "Unreviewed dependency source: $($item.name)." }
        $root = Split-Path $item.manifest_path
        $prefix = [IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
        $files = @(Get-ChildItem -LiteralPath $root -Recurse -File | Where-Object { $_.Name -match '(?i)licen[sc]e|copying|copyright|notice' })
        if ($item.license_file) {
            $explicit = $item.license_file
            if (![IO.Path]::IsPathRooted($explicit)) { $explicit = Join-Path $root $explicit }
            $files += Get-Item -LiteralPath $explicit
        }
        $files = @($files | Sort-Object -Property FullName -Unique)
        $notices = @()
        if (!$files.Count -and $item.name -eq 'windows-permissions' -and $item.version -eq '0.2.4' -and $item.license -eq 'MIT') {
            $notices += CopyNotice (Join-Path $package 'third-party\windows-permissions-0.2.4\NOTICE.txt') 'licenses\windows-permissions-0.2.4\NOTICE.txt'
        } elseif (!$files.Count) {
            throw "Missing notice files: $($item.name) $($item.version)."
        } else {
            foreach ($file in $files) {
                $path = [IO.Path]::GetFullPath($file.FullName)
                if (!$path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Notice path escapes its crate.' }
                $notices += CopyNotice $path "licenses\$($item.name)-$($item.version)\$($path.Substring($prefix.Length))"
            }
        }
        $rows += [ordered]@{ name=$item.name; version=$item.version; license=$item.license; notices=$notices }
    }
    $null = CopyNotice (Join-Path $sysroot 'share\doc\rust\COPYRIGHT-library.html') 'licenses\rust-standard-library\COPYRIGHT-library.html'
    $manifest = [ordered]@{
        kind='workspace-candidate'; product='Shield'; version=$version; target=$triple;
        compiler=$compiler; binary_sha256=$binaryHash; source_revision=$null;
        first_party=$firstParty; dependencies=$rows
    }
    [IO.File]::WriteAllText((Join-Path $stage 'BUILD.json'), ($manifest | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
    $readme = @(
        "Shield $version - Windows $Architecture portable workspace candidate", '',
        'BUILD.json identifies the executable and notices; it does not attest source provenance or signing.',
        'This package is not evidence of native ARM64 runtime acceptance.', '',
        'Install: extract into a folder you own, then add that folder to PATH.',
        'Check: shield --version. Start a project: shield init.',
        'Update: close Shield and connected agents before replacing shield.exe.',
        'Repair each project registration with shield mcp setup --repair after an update or move.',
        'Remove: run shield mcp remove for each connected agent in each project, then delete the installation.',
        'Keep project .shield folders to preserve policy and history.', '',
        'Guide: https://github.com/canonic2/canonic/blob/main/packages/shield/docs/getting-started.md',
        'LICENSE covers Canonic material. Third-party terms are under licenses/.',
        'Notices include the resolved normal/build dependency closure and Rust standard library.'
    ) -join "`r`n"
    [IO.File]::WriteAllText((Join-Path $stage 'README.txt'), $readme, [Text.UTF8Encoding]::new($false))
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $stream = [IO.File]::Open($archive, [IO.FileMode]::CreateNew)
    try {
        $zip = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $false)
        try {
            foreach ($file in (Get-ChildItem -LiteralPath $stage -Recurse -File | Sort-Object FullName)) {
                $entry = $file.FullName.Substring($stage.Length + 1).Replace('\', '/')
                [IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $file.FullName, $entry, [IO.Compression.CompressionLevel]::Optimal) | Out-Null
            }
        } finally { $zip.Dispose() }
    } finally { $stream.Dispose() }
    $digest = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText(($archive + '.sha256'), "$digest  $archiveName`n", [Text.UTF8Encoding]::new($false))
    Write-Host "Created $archiveName ($digest)."
} finally { Remove-Item -LiteralPath $stage -Recurse -Force }
