# Run on an interactive Windows desktop after cargo build --release -p tervix.
# The test creates and closes its own window; other Tervix instances are retained.
param(
    [string]$Executable = "$PSScriptRoot\..\..\target\release\tervix.exe",
    [switch]$SkipPointer,
    [ValidateSet('both', 'workspace', 'configuration')]
    [string]$PointerMode = 'both',
    [string]$EvidencePath,
    [string]$ErrorLogPath
)

$ErrorActionPreference = 'Stop'
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class TervixSmokeKeyboard {
    [StructLayout(LayoutKind.Sequential)]
    public struct Point { public int X; public int Y; }
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect { public int Left; public int Top; public int Right; public int Bottom; }
    [StructLayout(LayoutKind.Sequential)]
    public struct CursorInfo { public int Size; public int Flags; public IntPtr Handle; public Point Position; }
    [DllImport("user32.dll")] private static extern bool GetCursorInfo(ref CursorInfo info);
    [DllImport("user32.dll")] private static extern IntPtr LoadCursor(IntPtr instance, IntPtr name);
    public static bool IsTextCursor() {
        var info = new CursorInfo { Size = Marshal.SizeOf(typeof(CursorInfo)) };
        return GetCursorInfo(ref info) && info.Handle == LoadCursor(IntPtr.Zero, new IntPtr(32513));
    }
    [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
    public static bool Activate(IntPtr window) {
        uint pid;
        uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out pid);
        uint current = GetCurrentThreadId();
        bool attached = foreground != current && AttachThreadInput(current, foreground, true);
        try { return SetForegroundWindow(window); }
        finally { if (attached) AttachThreadInput(current, foreground, false); }
    }
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint thread);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr window, uint message, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr window, int index);
    [DllImport("shell32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr ExtractIconW(IntPtr instance, string path, uint index);
    [DllImport("user32.dll")] public static extern bool DestroyIcon(IntPtr icon);
    [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr window, IntPtr after, int x, int y, int width, int height, uint flags);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr context, uint flags);
    [DllImport("user32.dll")] private static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    public static void Left(bool release) { mouse_event(release ? 4u : 2u, 0, 0, 0, UIntPtr.Zero); }
    [DllImport("user32.dll")]
    private static extern void keybd_event(byte key, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")]
    private static extern uint MapVirtualKeyW(uint code, uint type);
    public static void Key(byte code, bool release) {
        keybd_event(code, (byte)MapVirtualKeyW(code, 0), release ? 2u : 0u, UIntPtr.Zero);
        // Legacy injected events are asynchronous. Give the foreign event loop
        // distinct modifier/press/release transitions, like a physical keyboard,
        // rather than a zero-duration chord with all states changing in one tick.
        System.Threading.Thread.Sleep(20);
    }
}
'@
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName System.Windows.Forms
$null = [TervixSmokeKeyboard]::SetProcessDPIAware()
$previousForeground = [TervixSmokeKeyboard]::GetForegroundWindow()
$previousCursor = New-Object TervixSmokeKeyboard+Point
$null = [TervixSmokeKeyboard]::GetCursorPos([ref]$previousCursor)
$process = if ($ErrorLogPath) { Start-Process -FilePath $Executable -PassThru -RedirectStandardError $ErrorLogPath } else { Start-Process -FilePath $Executable -PassThru }
$shell = New-Object -ComObject WScript.Shell

function Assert-Panes([int]$Count) {
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        Start-Sleep -Milliseconds 100
        $process.Refresh()
        if ($process.HasExited) { throw "GUI process exited unexpectedly; code=$($process.ExitCode); stderr=$ErrorLogPath" }
        if ($process.MainWindowTitle -like "Tervix*| $Count panes*" -and $process.Responding -and $process.MainWindowHandle -ne 0) {
            "PASS: PID $($process.Id), responsive native window, $Count panes"
            return
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Expected $Count panes, got '$($process.MainWindowTitle)'"
}

function Send-LayoutKey([byte]$Key, [bool]$Shift = $false, [bool]$Control = $true) {
    Assert-Foreground
    Start-Sleep -Milliseconds 150
    $foreground = [TervixSmokeKeyboard]::GetForegroundWindow()
    if ($foreground -ne $process.MainWindowHandle) {
        [uint32]$owner = 0
        $null = [TervixSmokeKeyboard]::GetWindowThreadProcessId($foreground, [ref]$owner)
        throw "Keyboard target lost foreground: actual handle=$foreground PID=$owner expected handle=$($process.MainWindowHandle) PID=$($process.Id)"
    }
    # Explicit virtual keys avoid SendKeys' VK_PACKET/Unicode injection, which
    # does not represent a physical shortcut in winit on this Windows host.
    if ($Control) { [TervixSmokeKeyboard]::Key(0x11, $false) }
    [TervixSmokeKeyboard]::Key(0x12, $false)
    if ($Shift) { [TervixSmokeKeyboard]::Key(0x10, $false) }
    [TervixSmokeKeyboard]::Key($Key, $false)
    [TervixSmokeKeyboard]::Key($Key, $true)
    if ($Shift) { [TervixSmokeKeyboard]::Key(0x10, $true) }
    [TervixSmokeKeyboard]::Key(0x12, $true)
    if ($Control) { [TervixSmokeKeyboard]::Key(0x11, $true) }
    Start-Sleep -Milliseconds 250
}

function Get-Client {
    $process.Refresh()
    $rect = New-Object TervixSmokeKeyboard+Rect
    $origin = New-Object TervixSmokeKeyboard+Point
    if (-not [TervixSmokeKeyboard]::GetClientRect($process.MainWindowHandle, [ref]$rect) -or
        -not [TervixSmokeKeyboard]::ClientToScreen($process.MainWindowHandle, [ref]$origin)) {
        throw 'Cannot measure native client area'
    }
    @{ X = $origin.X; Y = $origin.Y; Width = ($rect.Right - $rect.Left); Height = ($rect.Bottom - $rect.Top) }
}

function Activate-Window {
    $process.Refresh()
    $null = [TervixSmokeKeyboard]::ShowWindow($process.MainWindowHandle, 9)
    $area = [System.Windows.Forms.Screen]::FromHandle($process.MainWindowHandle).WorkingArea
    # Only our disposable smoke window becomes topmost; unrelated windows are retained.
    if (-not [TervixSmokeKeyboard]::SetWindowPos($process.MainWindowHandle, [IntPtr](-1),
        $area.Left, $area.Top, [Math]::Min(1366, $area.Width), [Math]::Min(768, $area.Height), 0x40)) {
        throw 'Cannot bring owned smoke window above desktop overlays'
    }
    for ($attempt = 0; $attempt -lt 3; $attempt++) {
        $null = $shell.AppActivate($process.Id)
        $null = [TervixSmokeKeyboard]::Activate($process.MainWindowHandle)
        Start-Sleep -Milliseconds 250
        if ([TervixSmokeKeyboard]::GetForegroundWindow() -eq $process.MainWindowHandle) { break }
    }
    if ([TervixSmokeKeyboard]::GetForegroundWindow() -ne $process.MainWindowHandle) {
        $foreground = [TervixSmokeKeyboard]::GetForegroundWindow()
        [uint32]$owner = 0
        $null = [TervixSmokeKeyboard]::GetWindowThreadProcessId($foreground, [ref]$owner)
        throw "Smoke-test window is not foreground: actual handle=$foreground PID=$owner expected handle=$($process.MainWindowHandle) PID=$($process.Id)"
    }
}

function Find-Divider([string]$Mode, [switch]$AllowMissing) {
    # PrintWindow captures the real native client rendering without sampling
    # unrelated desktop pixels. Pointer injection separately checks the target.
    Start-Sleep -Milliseconds 500
    $client = Get-Client
    $bitmap = New-Object System.Drawing.Bitmap($client.Width, $client.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $context = $graphics.GetHdc()
        try {
            if (-not [TervixSmokeKeyboard]::PrintWindow($process.MainWindowHandle, $context, 3)) {
                throw 'Cannot capture rendered native client area with PrintWindow'
            }
        } finally { $graphics.ReleaseHdc($context) }
        if ($EvidencePath) { $bitmap.Save($EvidencePath, [System.Drawing.Imaging.ImageFormat]::Png) }
        $rgb = switch ($Mode) {
            'configuration' { @(100, 224, 192) }
            'dragging' { @(133, 181, 255) }
            default { @(69, 87, 119) }
        }
        # Ratios are near half in this fixture. Require a 40px vertical line,
        # not a single coincidental text pixel or horizontal pane border.
        $y = [int]($client.Height * 0.80)
        for ($x = [int]($client.Width * 0.25); $x -lt $client.Width * 0.75; $x++) {
            $matches = $true
            foreach ($dy in @(0, 10, 20, 30, 40)) {
                $color = $bitmap.GetPixel($x, $y + $dy)
                # PrintWindow applies compositor alpha on this software-renderer
                # path. Bound channel tolerance still separates edit/drag colors.
                if ([Math]::Abs($color.R - $rgb[0]) -gt 18 -or
                    [Math]::Abs($color.G - $rgb[1]) -gt 18 -or
                    [Math]::Abs($color.B - $rgb[2]) -gt 18) { $matches = $false; break }
            }
            # A focused tile has a 1px accent border; only a divider has two
            # adjacent vertical pixels in this color. Ignore borders and shadows.
            if ($matches) {
                $neighbor = $bitmap.GetPixel($x+1, $y)
                if ([Math]::Abs($neighbor.R-$rgb[0]) -gt 18 -or [Math]::Abs($neighbor.G-$rgb[1]) -gt 18 -or [Math]::Abs($neighbor.B-$rgb[2]) -gt 18) { $matches = $false }
            }
            if ($matches) { return $x }
        }
        if ($AllowMissing) { return -1 }
        $sample = $bitmap.GetPixel([int]($client.Width/2), $y)
        throw "No rendered $Mode divider found; center=$sample; client=$($client | ConvertTo-Json -Compress)"
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Move-Pointer([int]$X) {
    $client = Get-Client
    $targetX = $client.X + $X
    $targetY = $client.Y + [int]($client.Height * 0.80)
    if ($X -lt 0 -or $X -ge $client.Width -or
        -not (@([System.Windows.Forms.Screen]::AllScreens | Where-Object { $_.Bounds.Contains($targetX, $targetY) }).Count)) {
        throw "Pointer target outside client or physical screen bounds: $targetX,$targetY"
    }
    if (-not [TervixSmokeKeyboard]::SetCursorPos($targetX, $targetY)) {
        throw 'Cannot move native pointer'
    }
    Start-Sleep -Milliseconds 180
    $point = New-Object TervixSmokeKeyboard+Point
    if (-not [TervixSmokeKeyboard]::GetCursorPos([ref]$point)) { throw 'Cannot read native pointer' }
    if ([Math]::Abs($point.X - $targetX) -gt 2 -or [Math]::Abs($point.Y - $targetY) -gt 2) {
        throw "External cursor interference: requested=$targetX,$targetY actual=$($point.X),$($point.Y)"
    }
    if ([TervixSmokeKeyboard]::WindowFromPoint($point) -ne $process.MainWindowHandle) {
        throw "Pointer target is occluded: cursor=$($point.X),$($point.Y) target=$([TervixSmokeKeyboard]::WindowFromPoint($point)) expected=$($process.MainWindowHandle)"
    }
}

function Assert-Near([int]$Actual, [int]$Expected, [string]$Label) {
    if ([Math]::Abs($Actual - $Expected) -gt 3) { throw "$Label`: expected $Expected, got $Actual" }
    "PASS: $Label (rendered divider x=$Actual)"
}

function Test-PointerResize([string]$Mode) {
    Activate-Window
    $original = Find-Divider $Mode
    "INFO: $Mode pointer fixture original=$original client=$((Get-Client | ConvertTo-Json -Compress))"
    Move-Pointer ($original + 4)
    [TervixSmokeKeyboard]::Left($false)
    Start-Sleep -Milliseconds 180
    try {
        Move-Pointer ($original + 74)
        $preview = Find-Divider 'dragging'
        if ($preview -lt $original + 40) { throw "Native pointer drag did not visibly move divider: original=$original preview=$preview client=$((Get-Client | ConvertTo-Json -Compress))" }
    } finally { [TervixSmokeKeyboard]::Left($true) }
    Start-Sleep -Milliseconds 250
    Assert-Near (Find-Divider $Mode) $preview "$Mode drag commit"
    Send-LayoutKey 0x5A
    Assert-Near (Find-Divider $Mode) $original "$Mode drag undo"

    Move-Pointer ($original + 4)
    [TervixSmokeKeyboard]::Left($false)
    Start-Sleep -Milliseconds 180
    try {
        Move-Pointer ($original - 66)
        $preview = Find-Divider 'dragging'
        if ($preview -gt $original - 40) { throw "Cancel fixture did not visibly move divider: original=$original preview=$preview" }
        [TervixSmokeKeyboard]::Key(0x1B, $false)
        [TervixSmokeKeyboard]::Key(0x1B, $true)
        Start-Sleep -Milliseconds 250
        Assert-Near (Find-Divider $Mode) $original "$Mode Escape cancel"
    } finally { [TervixSmokeKeyboard]::Left($true) }
    Start-Sleep -Milliseconds 200
}

function Test-WorkspaceResizeRejected {
    # Discover the authoritative position while editing, then test the same
    # position in normal mode, where there must be no interactive divider.
    Send-LayoutKey 0x45
    $original = Find-Divider 'configuration'
    Send-LayoutKey 0x45
    if ((Find-Divider 'workspace' -AllowMissing) -ne -1 -or
        (Find-Divider 'configuration' -AllowMissing) -ne -1) {
        throw 'Normal mode still renders a resize divider'
    }
    Move-Pointer ($original + 4)
    [TervixSmokeKeyboard]::Left($false)
    try { Move-Pointer ($original + 74) }
    finally { [TervixSmokeKeyboard]::Left($true) }
    Send-LayoutKey 0x27 $true # Shift+Right: resize must also be rejected.
    Send-LayoutKey 0x45
    Assert-Near (Find-Divider 'configuration') $original 'normal-mode pointer and keyboard resize rejected'
    Send-LayoutKey 0x45
}

function Test-RealTerminal {
    # GUI keyboard -> application port -> writer -> real PowerShell -> filesystem
    # and real ANSI-colored stdout -> VT emulator -> rendered client pixels.
    $marker = Join-Path 'C:\Users\levan\AppData\Local\Temp\opencode' ("tervix-gui-" + [Guid]::NewGuid().ToString('N'))
    try {
        Start-Sleep -Seconds 2
        Assert-Foreground
        $command = "Write-Host TERVIX_GUI_OUTPUT -ForegroundColor Magenta; Write-Host TERVIX_GUI_BACKGROUND -BackgroundColor DarkBlue; New-Item -ItemType File -Path '$marker'"
        # SendWait uses SendInput on this host and does not wait for a foreign
        # process to consume an entire burst. Type ordinary keystrokes instead
        # of racing shell startup/PSReadLine with a synthetic zero-delay flood.
        foreach ($character in $command.ToCharArray()) {
            $key = [string]$character
            if ('+^%~(){}'.Contains($key)) { $key = '{' + $key + '}' }
            [System.Windows.Forms.SendKeys]::SendWait($key)
            Start-Sleep -Milliseconds 10
        }
        [TervixSmokeKeyboard]::Key(0x0D, $false)
        [TervixSmokeKeyboard]::Key(0x0D, $true)
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $marker) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
        Start-Sleep -Milliseconds 500
        $client = Get-Client
        $bitmap = New-Object System.Drawing.Bitmap($client.Width, $client.Height)
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
            $context = $graphics.GetHdc()
            try { $null = [TervixSmokeKeyboard]::PrintWindow($process.MainWindowHandle, $context, 3) }
            finally { $graphics.ReleaseHdc($context) }
            if ($EvidencePath) { $bitmap.Save($EvidencePath, [System.Drawing.Imaging.ImageFormat]::Png) }
            if (-not (Test-Path -LiteralPath $marker)) { throw 'GUI terminal keyboard did not execute the shell command' }
            $pixels = 0
            $backgroundPixels = 0
            $left = if ($process.MainWindowTitle -like '*| 2 panes*') { [int]($client.Width/2)+20 } else { 20 }
            for ($y = 150; $y -lt $client.Height - 40; $y++) {
                for ($x = $left; $x -lt $client.Width - 20; $x++) {
                    $c = $bitmap.GetPixel($x,$y)
                    if ($c.R -gt 140 -and $c.B -gt 160 -and $c.G -lt 145 -and $c.B -gt $c.G * 1.4) { $pixels++ }
                    if ($c.B -gt 150 -and $c.B -gt $c.R * 2.1 -and $c.B -gt $c.G * 1.15) { $backgroundPixels++ }
                }
            }
            if ($pixels -lt 30) { throw "Shell executed, but magenta terminal output was not rendered ($pixels pixels)" }
            if ($backgroundPixels -lt 100) { throw "ANSI background geometry was clipped ($backgroundPixels pixels)" }
            "PASS: real GUI shell input executed; ANSI foreground/background rendered ($pixels/$backgroundPixels pixels)"
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    } finally { if (Test-Path -LiteralPath $marker) { Remove-Item -LiteralPath $marker } }
}

function Assert-Title([string]$Pattern, [string]$Label) {
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $process.Refresh()
        if ($process.HasExited) { throw "GUI process exited unexpectedly; code=$($process.ExitCode); stderr=$ErrorLogPath" }
        if ($process.MainWindowTitle -match $Pattern -and $process.Responding) { "PASS: $Label ($($process.MainWindowTitle))"; return }
        Start-Sleep -Milliseconds 150
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "$Label did not appear: '$($process.MainWindowTitle)'"
}

function Click-Client([int]$X, [int]$Y) {
    Assert-Foreground
    $client = Get-Client
    $null = [TervixSmokeKeyboard]::SetCursorPos($client.X + $X, $client.Y + $Y)
    Start-Sleep -Milliseconds 100
    $point = New-Object TervixSmokeKeyboard+Point
    $null = [TervixSmokeKeyboard]::GetCursorPos([ref]$point)
    if ([Math]::Abs($point.X - ($client.X + $X)) -gt 2 -or [Math]::Abs($point.Y - ($client.Y + $Y)) -gt 2 -or
        [TervixSmokeKeyboard]::WindowFromPoint($point) -ne $process.MainWindowHandle) {
        throw "Click target moved or is occluded: requested=$($client.X + $X),$($client.Y + $Y) actual=$($point.X),$($point.Y)"
    }
    [TervixSmokeKeyboard]::Left($false)
    Start-Sleep -Milliseconds 70
    [TervixSmokeKeyboard]::Left($true)
    Start-Sleep -Milliseconds 180
}

function Assert-Foreground {
    $process.Refresh()
    if ([TervixSmokeKeyboard]::GetForegroundWindow() -ne $process.MainWindowHandle) {
        $null = [TervixSmokeKeyboard]::Activate($process.MainWindowHandle)
    }
    if ([TervixSmokeKeyboard]::GetForegroundWindow() -ne $process.MainWindowHandle) {
        throw 'Native input target is not foreground'
    }
}

function Assert-WindowState([string]$State, [bool]$Expected, [string]$Label) {
    # Window controls schedule an asynchronous Iced/winit task. Poll the actual
    # native state, rather than treating a fixed post-click sleep as completion.
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $actual = if ($State -eq 'maximized') { [TervixSmokeKeyboard]::IsZoomed($process.MainWindowHandle) } else { [TervixSmokeKeyboard]::IsIconic($process.MainWindowHandle) }
        if ($actual -eq $Expected) { return }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "$Label failed: native $State=$actual expected=$Expected"
}

function Test-CustomChrome {
    $handle = $process.MainWindowHandle
    # Winit retains WS_CAPTION for Aero snap but removes its nonclient rendering
    # through WM_NCCALCSIZE. Verify actual geometry, not the retained style bits.
    $outer = New-Object TervixSmokeKeyboard+Rect
    $null = [TervixSmokeKeyboard]::GetWindowRect($handle, [ref]$outer)
    $client = Get-Client
    if ($client.Y - $outer.Top -gt 16) { throw 'Native standard caption is still rendered' }
    if ([TervixSmokeKeyboard]::SendMessageW($handle, 0x7F, [IntPtr]0, [IntPtr]0) -eq [IntPtr]0) { throw 'Window icon missing' }
    $icon = [TervixSmokeKeyboard]::ExtractIconW([IntPtr]0, (Resolve-Path $Executable).Path, 0)
    if ($icon -eq [IntPtr]0 -or $icon -eq [IntPtr]1) { throw 'Executable icon resource missing' }
    $null = [TervixSmokeKeyboard]::DestroyIcon($icon)
    'PASS: custom chrome, native window icon and executable icon resource'
    $client = Get-Client
    Click-Client ($client.Width - 84) 42
    Assert-WindowState 'maximized' $true 'Custom maximize button'
    $client = Get-Client
    Click-Client ($client.Width - 84) 42
    Assert-WindowState 'maximized' $false 'Custom restore button'
    'PASS: custom maximize / restore controls'
    $client = Get-Client
    Click-Client ($client.Width - 131) 42
    Assert-WindowState 'minimized' $true 'Custom minimize button'
    $null = [TervixSmokeKeyboard]::ShowWindow($handle, 9)
    Activate-Window
    'PASS: custom minimize and native restore'
    Send-LayoutKey 0x79
    Assert-WindowState 'maximized' $true 'Keyboard maximize'
    Send-LayoutKey 0x79
    Assert-WindowState 'maximized' $false 'Keyboard restore'
    'PASS: keyboard window controls'
    $client = Get-Client
    $null = [TervixSmokeKeyboard]::SetCursorPos($client.X + 400, $client.Y + 42)
    [TervixSmokeKeyboard]::Left($false)
    Start-Sleep -Milliseconds 150
    $null = [TervixSmokeKeyboard]::SetCursorPos($client.X + 450, $client.Y + 82)
    Start-Sleep -Milliseconds 200
    [TervixSmokeKeyboard]::Left($true)
    Start-Sleep -Milliseconds 300
    $after = Get-Client
    if ([Math]::Abs($after.X - $client.X) -lt 20 -and [Math]::Abs($after.Y - $client.Y) -lt 20) { throw 'Custom titlebar drag failed' }
    'PASS: titlebar native drag'
    Start-Sleep -Milliseconds 500
    Click-Client 400 42
    Click-Client 400 42
    Assert-WindowState 'maximized' $true 'Titlebar double click'
    Send-LayoutKey 0x79
    'PASS: titlebar double-click maximize / keyboard restore'
    foreach ($edge in 0..7) {
        $area = [System.Windows.Forms.Screen]::FromHandle($handle).WorkingArea
        $null = [TervixSmokeKeyboard]::SetWindowPos($handle, [IntPtr]0, $area.Left+70, $area.Top+70, 1000, 700, 0x14)
        Start-Sleep -Milliseconds 300
        $client = Get-Client
        $x = switch ($edge) { 0 { 2 } 1 { $client.Width-2 } 2 { 500 } 3 { 500 } 4 { 4 } 5 { $client.Width-4 } 6 { 4 } 7 { $client.Width-4 } }
        $y = switch ($edge) { 0 { 350 } 1 { 350 } 2 { 2 } 3 { $client.Height-2 } 4 { 4 } 5 { 4 } 6 { $client.Height-4 } 7 { $client.Height-4 } }
        $null = [TervixSmokeKeyboard]::SetCursorPos($client.X+$x, $client.Y+$y)
        Start-Sleep -Milliseconds 80
        [TervixSmokeKeyboard]::Left($false)
        Start-Sleep -Milliseconds 180
        $null = [TervixSmokeKeyboard]::SetCursorPos($client.X+$x+25, $client.Y+$y+25)
        Start-Sleep -Milliseconds 180
        [TervixSmokeKeyboard]::Left($true)
        Start-Sleep -Milliseconds 300
        $after = Get-Client
        if ([Math]::Abs($after.Width - $client.Width) -lt 10 -and [Math]::Abs($after.Height - $client.Height) -lt 10) { throw "Native resize edge $edge failed" }
        "PASS: native resize edge/corner $edge"
    }
    Activate-Window
}

function Test-ProcessQuery {
    Send-LayoutKey 0x51 # Q: focus this process pane's search without a pointer.
    [System.Windows.Forms.SendKeys]::SendWait('tervix')
    Assert-Title 'Processes [1-9][0-9]* / [1-9][0-9]* \| Filter: tervix' 'real process query found this application'
}

function Save-MonitorEvidence {
    $client = Get-Client
    $bitmap = New-Object System.Drawing.Bitmap($client.Width, $client.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $context = $graphics.GetHdc()
        try { $null = [TervixSmokeKeyboard]::PrintWindow($process.MainWindowHandle, $context, 3) }
        finally { $graphics.ReleaseHdc($context) }
        if ($EvidencePath) { $bitmap.Save(($EvidencePath -replace '\.png$', '.monitor.png'), [System.Drawing.Imaging.ImageFormat]::Png) }
        # Exclude the color legend above the plot. Both real series must render
        # below it; this catches backend-culling regressions invisible to DTO tests.
        $cpuPixels = 0
        $memoryPixels = 0
        for ($y = 540; $y -lt 690; $y++) {
            for ($x = [int]($client.Width * 0.55); $x -lt $client.Width - 40; $x++) {
                $color = $bitmap.GetPixel($x, $y)
                if ($color.B -gt 150 -and $color.B -gt $color.R * 1.3 -and $color.B -gt $color.G * 1.05) { $cpuPixels++ }
                if ($color.B -gt 160 -and $color.R -gt 130 -and $color.R -gt $color.G * 1.08 -and $color.B -gt $color.R * 1.2) { $memoryPixels++ }
            }
        }
        if ($cpuPixels -lt 20 -or $memoryPixels -lt 20) { throw "Live chart series not rendered: CPU=$cpuPixels RAM=$memoryPixels" }
        "PASS: real CPU/RAM chart series rendered (CPU=$cpuPixels RAM=$memoryPixels pixels)"
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Save-ViewEvidence([string]$Name) {
    if (-not $EvidencePath) { return }
    $client = Get-Client
    $bitmap = New-Object System.Drawing.Bitmap($client.Width, $client.Height)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $context = $graphics.GetHdc()
        try { $null = [TervixSmokeKeyboard]::PrintWindow($process.MainWindowHandle, $context, 3) }
        finally { $graphics.ReleaseHdc($context) }
        $bitmap.Save(($EvidencePath -replace '\.png$', ".$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally { $graphics.Dispose(); $bitmap.Dispose() }
}

function Test-InactiveProcessClick {
    Send-LayoutKey 0x25 # Left: domain focus is the terminal, not Processes.
    $process.Refresh()
    if ($process.MainWindowTitle -match '\| Processes ') { throw 'Process pane was not made inactive' }
    Activate-Window
    $client = Get-Client
    $x = $client.X + [int]($client.Width * 0.75)
    $found = $false
    # Locate the real input by Iced's I-beam cursor, avoiding font/DPI-specific
    # hard-coded textbox coordinates. No focus shortcut or preliminary click.
    for ($y = 130; $y -lt [Math]::Min(400, $client.Height / 2); $y += 4) {
        $null = [TervixSmokeKeyboard]::SetCursorPos($x, $client.Y + $y)
        Start-Sleep -Milliseconds 25
        if ([TervixSmokeKeyboard]::IsTextCursor()) { $found = $true; break }
    }
    if (-not $found) { throw 'Inactive Processes input I-beam not found' }
    $point = New-Object TervixSmokeKeyboard+Point
    $null = [TervixSmokeKeyboard]::GetCursorPos([ref]$point)
    if ([TervixSmokeKeyboard]::WindowFromPoint($point) -ne $process.MainWindowHandle) { throw 'Search click is occluded' }
    [TervixSmokeKeyboard]::Left($false)
    [TervixSmokeKeyboard]::Left($true)
    Assert-Title '\| Processes .*\| Filter: tervix' 'inactive input click changes domain focus'
    # Append at the clicked caret: domain focus must not steal widget focus.
    [System.Windows.Forms.SendKeys]::SendWait('pointerfocus')
    Assert-Title '\| Processes .*\| Filter: tervixpointerfocus$' 'clicked input keeps keyboard focus'
    Send-LayoutKey 0x53 # Sort must target this process pane while input is captured.
    Send-LayoutKey 0x25
    Send-LayoutKey 0x27
    Assert-Title '\| Processes .*\| Filter: tervixpointerfocus$' 'directional focus returns to clicked process pane'
    Send-LayoutKey 0x57 # Close Processes directly, not the sibling shell/modal.
    Assert-Panes 1
    Send-LayoutKey 0x5A
    Assert-Panes 2
    Assert-Title '\| Processes .*\| Filter: tervixpointerfocus$' 'close and undo target clicked process pane'
}

try {
    Assert-Panes 1
    Activate-Window
    if (-not $SkipPointer) { Test-CustomChrome }
    Test-RealTerminal
    Send-LayoutKey 0x57 # W: Last pane close must be rejected.
    Assert-Panes 1
    Send-LayoutKey 0x56 # V
    Assert-Panes 2
    Test-RealTerminal # Rendering in a translated right-hand terminal must work too.
    if ($SkipPointer) { 'SKIP: real pointer drag/cancel (explicit -SkipPointer)' }
    elseif ($PointerMode -ne 'configuration') { Test-WorkspaceResizeRejected }
    else { 'SKIP: workspace pointer fixture (explicit -PointerMode configuration)' }
    Send-LayoutKey 0x45 # E: configuration changes the divider to green.
    $null = Find-Divider 'configuration'
    'PASS: Configuration mode visibly changes native divider accent'
    Send-LayoutKey 0x54 # T: PowerShell
    Send-LayoutKey 0x54 # T: cmd
    Send-LayoutKey 0x54 # T: WSL default distro
    Assert-Title '\| WSL default' 'default WSL profile retained'
    Send-LayoutKey 0x4C # L: refresh actual discovery
    Start-Sleep -Seconds 6
    Send-LayoutKey 0x44 # D: explicitly select first installed distribution, if present.
    $process.Refresh()
    if ($process.MainWindowTitle -notmatch '\| WSL default') {
        Assert-Title '\| WSL .+' 'explicit discovered WSL distribution selected'
    } else { 'SKIP: explicit distro selection has no installed discovery result; inspect WSL environment tests' }
    Send-LayoutKey 0x54 # T: SSH, selection alone must not connect
    Assert-Title '\| SSH disconnected' 'SSH pane selected without connecting'
    Send-LayoutKey 0x51 # Q: host field
    [System.Windows.Forms.SendKeys]::SendWait('tervix-smoke.invalid')
    [TervixSmokeKeyboard]::Key(0x09, $false)
    [TervixSmokeKeyboard]::Key(0x09, $true)
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('smoke')
    [TervixSmokeKeyboard]::Key(0x09, $false)
    [TervixSmokeKeyboard]::Key(0x09, $true)
    Start-Sleep -Milliseconds 200
    [System.Windows.Forms.SendKeys]::SendWait('2222')
    Assert-Title '\| SSH draft: tervix-smoke.invalid \| user: smoke \| port: 2222' 'SSH keyboard host/user/port fields retain independent input'
    $sshChildren = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($process.Id)" | Where-Object { $_.Name -eq 'ssh.exe' })
    if ($sshChildren.Count -ne 0) { throw 'Editing SSH form started a remote client without Connect' }
    'PASS: SSH form editing performs no remote connection'
    Save-ViewEvidence 'ssh'
    Send-LayoutKey 0x54 # T: Monitor
    Assert-Title '\| CPU [0-9]+\.[0-9]% \| RAM [0-9]+\.[0-9]+ GiB' 'native CPU and RAM published into monitor pane'
    Send-LayoutKey 0x45
    Start-Sleep -Seconds 2
    Save-MonitorEvidence
    Send-LayoutKey 0x45
    Send-LayoutKey 0x54 # T: Processes
    Assert-Title '\| Processes [1-9][0-9]* / [1-9][0-9]*' 'native process list published into process pane'
    Send-LayoutKey 0x45 # E: leave edit mode for process input
    Test-ProcessQuery
    if (-not $SkipPointer) { Test-InactiveProcessClick }
    Send-LayoutKey 0x45 # E: edit again
    Send-LayoutKey 0x54 # T: Docker Containers
    Assert-Title '\| Docker Disconnected' 'Docker selection does not connect'
    Send-LayoutKey 0x45 # E: normal mode
    Send-LayoutKey 0x51 # Q: focus Docker search
    [System.Windows.Forms.SendKeys]::SendWait('offline-fixture')
    Assert-Title '\| Docker Disconnected \| Filter: offline-fixture' 'Docker search keyboard input'
    # Unfocus search via layout focus navigation, then explicitly connect using c.
    Send-LayoutKey 0x25
    Send-LayoutKey 0x27
    Assert-Foreground
    [uint32]$owner = 0
    $inputThread = [TervixSmokeKeyboard]::GetWindowThreadProcessId($process.MainWindowHandle, [ref]$owner)
    "INFO: Docker keyboard layout=$([TervixSmokeKeyboard]::GetKeyboardLayout($inputThread)) Ctrl=$([TervixSmokeKeyboard]::GetAsyncKeyState(0x11)) Alt=$([TervixSmokeKeyboard]::GetAsyncKeyState(0x12)) Shift=$([TervixSmokeKeyboard]::GetAsyncKeyState(0x10))"
    # These are logical character commands. Unlike physical layout chords, a
    # VK_C can mean Cyrillic 'с' on the user's active keyboard layout.
    [System.Windows.Forms.SendKeys]::SendWait('c')
    Assert-Title '\| Docker (Connecting|Retrying|Connected)' 'explicit Docker Connect is responsive (read-only handshake/list only)'
    Start-Sleep -Seconds 6
    Assert-Title '\| Docker (Retrying|Connected)' 'Docker unavailable/retry or connected state is published'
    Save-ViewEvidence 'docker'
    Assert-Foreground
    [System.Windows.Forms.SendKeys]::SendWait('x')
    Assert-Title '\| Docker Disconnected' 'explicit Docker disconnect'
    Send-LayoutKey 0x45 # E: configuration
    Send-LayoutKey 0x54 # T: Docker Logs without a target
    Assert-Panes 2
    Send-LayoutKey 0x54 # T: Docker Inspect without a target
    Assert-Panes 2
    Send-LayoutKey 0x54 # T: Help
    Send-LayoutKey 0x54 # T: Settings
    Send-LayoutKey 0x52 # R: reduced motion
    Send-LayoutKey 0x46 # F: stretch
    Assert-Panes 2
    Send-LayoutKey 0x46 # F: restore
    if (-not $SkipPointer -and $PointerMode -ne 'workspace') { Test-PointerResize 'configuration' }
    Send-LayoutKey 0x45 # E: clean workspace
    if ((Find-Divider 'workspace' -AllowMissing) -ne -1) {
        throw 'Leaving Configuration mode kept a resize divider'
    }
    Send-LayoutKey 0x5A # Cancelled drags/configuration must not consume layout undo.
    Assert-Panes 1
    Send-LayoutKey 0x56
    Assert-Panes 2
    Send-LayoutKey 0x48 # H
    Assert-Panes 3
    $ownedChildren = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($process.Id)" | ForEach-Object { @{ Id = $_.ProcessId; Created = $_.CreationDate } })
    Send-LayoutKey 0x26 # Up
    Send-LayoutKey 0x50 # P: previous focus (avoid Windows-reserved Ctrl+Alt+Tab).
    Send-LayoutKey 0x28 $true # Shift+Down
    Assert-Panes 3
    Send-LayoutKey 0x57
    # A real live terminal requires explicit termination confirmation.
    [TervixSmokeKeyboard]::Key(0x0D, $false)
    [TervixSmokeKeyboard]::Key(0x0D, $true)
    Assert-Panes 2
    Send-LayoutKey 0x5A # Z
    Assert-Panes 3
    $null = [TervixSmokeKeyboard]::SetWindowPos($process.MainWindowHandle, [IntPtr](-2), 0, 0, 0, 0, 0x13)
    Send-LayoutKey 0x73 $false $false # Alt+F4
    Start-Sleep -Milliseconds 300
    if ($process.HasExited) { throw 'Native close skipped active-shell confirmation' }
    [TervixSmokeKeyboard]::Key(0x1B, $false)
    [TervixSmokeKeyboard]::Key(0x1B, $true)
    Assert-Panes 3
    $client = Get-Client
    Click-Client ($client.Width - 37) 42
    [TervixSmokeKeyboard]::Key(0x0D, $false)
    [TervixSmokeKeyboard]::Key(0x0D, $true)
    if (-not $process.WaitForExit(5000)) { throw 'Native close did not terminate process' }
    'PASS: native close terminated the smoke-test process'
    Start-Sleep -Seconds 2
    foreach ($child in $ownedChildren) {
        $remaining = Get-CimInstance Win32_Process -Filter "ProcessId=$($child.Id)"
        if ($remaining -and $remaining.CreationDate -eq $child.Created) { throw "Native close left a child process alive: PID $($child.Id)" }
    }
    "PASS: native close released $($ownedChildren.Count) directly owned child processes"
} finally {
    [TervixSmokeKeyboard]::Left($true)
    $process.Refresh()
    if (-not $process.HasExited) {
        $null = [TervixSmokeKeyboard]::SetWindowPos($process.MainWindowHandle, [IntPtr](-2), 0, 0, 0, 0, 0x13)
        Stop-Process -Id $process.Id
    }
    $null = [TervixSmokeKeyboard]::SetCursorPos($previousCursor.X, $previousCursor.Y)
    $null = [TervixSmokeKeyboard]::SetForegroundWindow($previousForeground)
}
