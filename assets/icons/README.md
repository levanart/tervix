# Tervix icon

Original geometric artwork created for Tervix: a cyan terminal chevron and violet
cursor in the left tile, with violet workspace lines and teal monitoring bars on
the right. A fine colored rim and deep navy rounded-square body keep the mark
legible on light and dark surfaces. The canvas outside the rounded body is
transparent. No fonts, stock artwork, or downloaded assets are used.

## Assets

| File | Format / intended use |
| --- | --- |
| `tervix.svg` | Editable vector source, 256 × 256 view box; documentation and scalable branding |
| `tervix-{16,24,32,48,64,128,256}.png` | Straight-alpha RGBA8 PNG at each named square size |
| `tervix-256.rgba` | 256 × 256, row-major RGBA8 bytes; 262,144 bytes; no header or row padding |
| `tervix.ico` | Windows multi-image icon, all seven sizes above, 32-bit PNG-compressed entries |

The bold prompt and asymmetric tile silhouette carry the small-size identity;
the detail inside the secondary tiles becomes more apparent at larger sizes.
Windows 11 supports the PNG-compressed ICO entries.

## Reproduce and verify

From the repository root, using Python 3.10+ and only its standard library:

```powershell
python assets/icons/generate.py
python assets/icons/generate.py --check
```

The generator reads colors and geometry directly from `tervix.svg`. It implements
only the SVG subset used here: opaque rounded rectangles, polygons, and two-stop
user-space linear gradients. Extend the renderer if introducing other SVG
features. Rasterization uses 4 × 4 supersampling with alpha-aware downsampling.
There are no runtime dependencies, network calls, or timestamp metadata.
PNG compression uses the installed Python's zlib; for byte-identical builds across
machines, use the same Python/zlib versions. `--check` regenerates in memory,
validates PNG checksums, dimensions, scanlines, transparency and ICO directory
entries, and compares every output with the checked-in bytes without writing.

## Application integration

For an Iced native window, use `tervix-256.rgba` with
`iced::window::icon::from_rgba(bytes.to_vec(), 256, 256)` and handle the returned
`Result` when populating window settings. `include_bytes!` embeds it without an
image-decoding dependency. Resolve the relative path from the calling Rust source
file; this directory is at repository-root `assets/icons`. The raw byte file is
straight alpha, matching the PNG, rather than premultiplied alpha.

If the application already enables PNG decoding, `tervix-256.png` is the smaller
embedded alternative. Smaller PNGs are useful for custom titlebars and menus;
choose the size appropriate for device pixels rather than stretching 16px art.

For the Windows executable's Explorer/taskbar identity, embed `tervix.ico` as the
primary `ICON` resource through the project's eventual Windows resource build
step. Setting the Iced window icon and embedding the executable resource are
separate integration steps. The SVG is source artwork, not an ICO replacement.
