param([Parameter(Mandatory=$true)][uint32]$ProcessId)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ShieldConsoleFixture {
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool AttachConsole(uint processId);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool GenerateConsoleCtrlEvent(uint controlEvent, uint processGroupId);
    [DllImport("kernel32.dll", SetLastError=true)]
    public static extern bool FreeConsole();
}
'@
$null = [ShieldConsoleFixture]::FreeConsole()
if (![ShieldConsoleFixture]::AttachConsole($ProcessId)) {
    throw [System.ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())
}
try {
    if (![ShieldConsoleFixture]::GenerateConsoleCtrlEvent(1, $ProcessId)) {
        throw [System.ComponentModel.Win32Exception]::new([Runtime.InteropServices.Marshal]::GetLastWin32Error())
    }
} finally {
    $null = [ShieldConsoleFixture]::FreeConsole()
}
