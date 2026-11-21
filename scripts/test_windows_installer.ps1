param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$Portable
)

$ErrorActionPreference = 'Stop'
$registration = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Tervix'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Tervix.lnk'
if ((Test-Path $registration) -or (Test-Path $shortcut)) {
    throw 'Run this smoke test in a Windows profile without an existing Tervix installation.'
}
$destination = Join-Path ([IO.Path]::GetTempPath()) ("Tervix installer test " + [guid]::NewGuid())
$installed = Join-Path $destination 'tervix.exe'
$uninstaller = Join-Path $destination 'Uninstall.exe'
$marker = Join-Path $destination 'user-file.txt'
$expectedHash = (Get-FileHash -LiteralPath $Portable -Algorithm SHA256).Hash

function Invoke-TestProcess([string]$File, [string]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -WindowStyle Hidden -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "$File exited with code $($process.ExitCode)" }
}

try {
    Invoke-TestProcess $Installer "/S /D=$destination"
    if ((Get-FileHash -LiteralPath $installed).Hash -ne $expectedHash) {
        throw 'Installed executable differs from the portable download.'
    }
    if ((Get-ItemPropertyValue $registration InstallLocation) -ne $destination) {
        throw 'Incorrect installation directory in Windows Settings.'
    }
    $link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
    if ($link.TargetPath -ne $installed) { throw 'Incorrect Start menu shortcut.' }
    Invoke-TestProcess $installed '--layout-demo'
    Set-Content -LiteralPath $marker -Value 'keep'
} finally {
    if (Test-Path -LiteralPath $uninstaller) { Invoke-TestProcess $uninstaller '/S' }
}
if ((Test-Path $registration) -or (Test-Path $shortcut) -or (Test-Path $installed) -or
    (Test-Path $uninstaller) -or (Get-Content -LiteralPath $marker) -ne 'keep') {
    throw 'Uninstall verification failed.'
}
Remove-Item -LiteralPath $marker
[IO.Directory]::Delete($destination)
Write-Output 'Windows installation, application startup and uninstall passed.'
