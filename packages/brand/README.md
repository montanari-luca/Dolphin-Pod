# Brand assets

- `source/dolphin.png`: original app logo (2000 px). Edit this, then regenerate.
- `source/mintflow-*.webp`: MintFlow Technologies logos (credits).
- `scripts/make_icons.py`: builds every platform icon. Requires Pillow + numpy:

```bash
python3 -m pip install pillow numpy && python3 packages/brand/scripts/make_icons.py
```

| Output | Notes |
| --- | --- |
| `apps/desktop/src-tauri/icons/icon.icns` | macOS Big Sur grid (824 px body, soft shadow) |
| `apps/desktop/src-tauri/icons/icon.ico` + PNGs | Windows/Linux; 16-32 px use a bolder glyph |
| `apps/ios/.../AppIcon.appiconset` | opaque light, transparent dark, grayscale tinted |
| `apps/website/public/*` | favicons, apple-touch-icon, PWA + maskable icons, og-image |
| `apps/desktop/public/brand/*` | in-app logo, glyph and MintFlow logos |
