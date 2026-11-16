# Manual documentation helper; never invoked by the application or build.
# Run only when the user authorizes capturing an already-open Tervix window.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('debug', 'release')]
    [string] $Profile,

    [int] $ProcessId = 0
)

$ErrorActionPreference = 'Stop'
$repository = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$expectedExecutable = Join-Path $repository "target\$Profile\tervix.exe"
$candidates = @(Get-Process -Name tervix -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -eq $expectedExecutable -and
    $_.MainWindowHandle -ne [IntPtr]::Zero -and
    ($ProcessId -eq 0 -or $_.Id -eq $ProcessId)
})
if ($candidates.Count -ne 1) {
    throw 'Expected exactly one already-open project Tervix window. Specify -ProcessId if multiple match. Nothing was captured or launched.'
}

$window = $candidates[0].MainWindowHandle
$ownerId = $candidates[0].Id
$temporaryDirectory = Join-Path ([System.IO.Path]::GetTempPath()) 'opencode'
if (-not (Test-Path -LiteralPath $temporaryDirectory -PathType Container)) {
    throw "Approved temporary directory is missing: $temporaryDirectory"
}

Add-Type -AssemblyName System.Drawing
# SAFETY: FFI is confined to this opt-in script. Only the verified project
# HWND is passed to PrintWindow; no desktop DC, input, focus, positioning,
# process termination, or application launch APIs are used. The bitmap owns
# its HDC, which is released in finally before disposing the bitmap.
if (-not ('TervixDocumentationCapture' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TervixDocumentationCapture
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool PrintWindow(IntPtr window, IntPtr destination, uint flags);
}
'@
}

[uint32] $currentOwner = 0
$null = [TervixDocumentationCapture]::GetWindowThreadProcessId($window, [ref] $currentOwner)
$verifiedProcess = Get-Process -Id $ownerId -ErrorAction Stop
if ($currentOwner -ne $ownerId -or $verifiedProcess.Path -ne $expectedExecutable) {
    throw 'Window ownership changed. Nothing was captured.'
}
if ([TervixDocumentationCapture]::IsIconic($window)) {
    throw 'The window is minimized. It will not be restored by this script.'
}
$rectangle = New-Object TervixDocumentationCapture+Rect
if (-not [TervixDocumentationCapture]::GetWindowRect($window, [ref] $rectangle)) {
    throw 'Cannot read the verified window bounds.'
}
$width = $rectangle.Right - $rectangle.Left
$height = $rectangle.Bottom - $rectangle.Top
if ($width -le 0 -or $height -le 0 -or $width -gt 16384 -or $height -gt 16384) {
    throw 'Window dimensions are invalid or exceed the capture limit.'
}

$bitmap = New-Object System.Drawing.Bitmap($width, $height)
try {
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $deviceContext = $graphics.GetHdc()
        try {
            # PW_RENDERFULLCONTENT = 2; GPU-backed windows can still return blank.
            $captured = [TervixDocumentationCapture]::PrintWindow($window, $deviceContext, 2)
        }
        finally {
            $graphics.ReleaseHdc($deviceContext)
        }
    }
    finally {
        $graphics.Dispose()
    }
    if (-not $captured) {
        throw 'PrintWindow failed. No desktop-capture fallback is permitted.'
    }
    $output = Join-Path $temporaryDirectory ("tervix-unreviewed-{0}.png" -f [Guid]::NewGuid().ToString('N'))
    $bitmap.Save($output, [System.Drawing.Imaging.ImageFormat]::Png)
    "UNREVIEWED temporary window capture: $output"
    'Inspect the image for sensitive content and blank/GPU output before copying it into the repository.'
}
finally {
    $bitmap.Dispose()
}
