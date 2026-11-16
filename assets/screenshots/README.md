# README screenshots

`monitor-preview.png` and `charts-preview.png` are crops from a real historical
1366×768 Tervix window capture. The terminal area was excluded to avoid publishing
local commands and paths. They show a previous English-language UI revision,
not the final release interface. `prepare-preview.py` reproduces those crops
from the original image using Pillow; it does not generate or simulate UI.

No running project window was available for a current capture. Launching or
manipulating the GUI was not authorized.

## Capture an existing window

With explicit capture permission and an already-open project window, run from
the repository root in Windows PowerShell:

```powershell
.\assets\screenshots\capture-window.ps1 -Profile release
# For a debug build, use -Profile debug.
# If multiple matching windows exist, supply -ProcessId <the intended PID>.
```

The helper matches the exact executable path `target\release\tervix.exe` or
`target\debug\tervix.exe` under this checkout and checks window ownership. It
uses native `PrintWindow` on that window only, with no desktop capture. It does
not launch, activate, move, resize, restore, stop, or send input to any window.
FFI stays in this documentation helper, outside production code.

The unreviewed PNG is saved only under
`$env:TEMP\opencode`. Inspect that image locally before
adding it to tracked assets. Check terminal text, commands, paths, usernames,
hosts, process lists, container names, credentials, and any other private data.
If a minor redaction or app-only crop is sufficient, label it explicitly in the
caption; never replace content with invented UI or data. Do not publish an
image that cannot be made safe without misrepresenting the application.

GPU-backed Iced windows may return blank even when `PrintWindow` reports
success. Reject blank or incomplete output. There is deliberately no desktop
capture fallback. `PrintWindow` is synchronous and may wait for the target
window to respond; use this manual helper only for a responsive window.

## Suggested root README snippet after review

Only add the following once a real, useful, reviewed image has been saved as
`assets/screenshots/tervix-workspace.png`. This filename is reserved guidance,
not an existing screenshot:

```markdown
## Screenshot

![Tervix native workspace](assets/screenshots/tervix-workspace.png)

Actual Tervix window captured with native PrintWindow. <!-- Describe any crop/redaction here. -->
```

Describe only the panes actually visible in the reviewed capture. No simulated
screenshots or external uploads are needed.
