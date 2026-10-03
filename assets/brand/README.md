# Glassview logo

The ripple: a solid dot with two concentric rings, the way a click spreads on screen.
The inner ring is solid; the outer ring is thinner and drawn at 55% opacity, so the
mark reads as a ripple rather than a target. The geometry is shared across all versions.

## Assets

- `mark.svg`: master; `currentColor` when used inline. Its default is black when loaded as an image.
- `mark-dark.svg`, `mark-white.svg`, `mark-accent.svg`: fixed-color versions for websites, documentation, and images.
- `app-icon.svg`, `.png`, `.icns`, `.ico`: cyan tile, ink mark, transparent outer padding. PNG is 1024 × 1024.
- `github-avatar.svg`, `.png`: full square background with enough space for a circular crop. PNG is 1024 × 1024.
- `menu-bar-{16,22,32,44}.png`: black alpha masks. Use as macOS template images so the OS determines the displayed color. The outer ring's partial alpha is intentional and renders as a lighter ring.
- `favicon.svg`: transparent mark that follows light/dark appearance. PNG and ICO fallbacks use the cyan tile.
- `preview.png`: review sheet; the wordmark uses a system sans serif and is a layout example.

Colors: cyan `#4FC5D5` (the app's default ripple color), ink `#0E2F38`, and white `#FFFFFF`.
Use the dark mark on light backgrounds, cyan or white on dark backgrounds.
Keep at least 3 units of clear space around the mark's 24-unit canvas.

## Tray states

The tray shows whether Glassview is visualizing input. On is the full ripple. Off keeps
the same silhouette at half opacity with a diagonal slash cut through it, so the icon
doesn't change size and the two states stay distinct in a monochrome menu bar.
The tray images live in `apps/desktop/src-tauri/icons`:

- `tray-on-template.png`, `tray-off-template.png`: macOS template images at 36 px for Tauri's 18 pt presentation on Retina displays.
- `tray-on.png`, `tray-off.png`: Windows icons at 32 px. On is the cyan tile; off is a grey tile. Both fill the canvas because Windows shows them at 16 px.

## Regeneration

The mark is defined in `scripts/generate-brand-assets.py`; there is no separate source file.
Edit it there, then run `python3 scripts/generate-brand-assets.py` from the repository root.
This optional asset-generation command requires `rsvg-convert` and Python's Pillow package;
neither is a runtime dependency. Every graphic in this folder is generated.

The generator also writes `apps/desktop/src-tauri/icons/icon.{png,icns,ico}`, the four
tray images in the same folder, and `favicon.{svg,ico}` in `apps/desktop/public`.
