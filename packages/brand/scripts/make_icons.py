#!/usr/bin/env python3
"""Generate every Dolphin Pod icon from packages/brand/source/dolphin.png.

Usage (needs Pillow + numpy):
    python packages/brand/scripts/make_icons.py

Outputs land in packages/brand/generated and are copied to each app:
  apps/desktop/src-tauri/icons     macOS .icns (Big Sur grid), Windows .ico, Linux/Store PNGs
  apps/ios/DolphinPod/Assets.xcassets/AppIcon.appiconset   light / dark / tinted 1024
  apps/website/public              favicons, touch icon, manifest icons, og-image
  apps/desktop/public/brand        in-app logo assets
"""

from __future__ import annotations

import json
import shutil
import subprocess
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[3]
BRAND = ROOT / "packages" / "brand"
SRC = BRAND / "source" / "dolphin.png"
GEN = BRAND / "generated"

TAURI_ICONS = ROOT / "apps" / "desktop" / "src-tauri" / "icons"
IOS_ICONSET = ROOT / "apps" / "ios" / "DolphinPod" / "Assets.xcassets" / "AppIcon.appiconset"
WEB_PUBLIC = ROOT / "apps" / "website" / "public"
DESKTOP_PUBLIC = ROOT / "apps" / "desktop" / "public" / "brand"

M = 1024  # master size


# --------------------------------------------------------------------------- masters


def extract_glyph(src: Image.Image) -> Image.Image:
    """Cut the dolphin off its white tile, keeping anti-aliased edges clean.

    A plain colour-to-alpha would make the pastel belly translucent, so the
    dolphin interior stays fully opaque and only a thin edge band is unmixed
    against white using the locally estimated dolphin colour.
    """
    # Flatten the tile's transparent corners onto white so the background is uniform.
    flat = Image.new("RGBA", src.size, (255, 255, 255, 255))
    flat.alpha_composite(src.convert("RGBA"))
    rgb = np.asarray(flat.convert("RGB")).astype(np.float32)
    min_c = rgb.min(axis=2)

    # Background = near-white pixels connected to the border (flood fill).
    non_white = (min_c < 250).astype(np.uint8) * 255
    mask_img = Image.fromarray(non_white, "L")
    flood = mask_img.copy()
    ImageDraw.floodfill(flood, (0, 0), 128)
    fg = np.asarray(flood) != 128  # dolphin + any enclosed light pixels

    fg_img = Image.fromarray(fg.astype(np.uint8) * 255, "L")
    interior = np.asarray(fg_img.filter(ImageFilter.MinFilter(9))) > 0

    # Local dolphin colour via normalised convolution of the interior.
    w = Image.fromarray(interior.astype(np.uint8) * 255, "L").filter(ImageFilter.GaussianBlur(6))
    wn = np.asarray(w).astype(np.float32) / 255.0
    est = np.zeros_like(rgb)
    for c in range(3):
        ch = Image.fromarray((rgb[..., c] * interior).astype(np.uint8), "L").filter(ImageFilter.GaussianBlur(6))
        est[..., c] = np.asarray(ch).astype(np.float32) / np.maximum(wn, 1e-3)
    est = np.clip(est, 0, 254)

    # Unmix the band: p = a*C + (1-a)*255  =>  a = (255-p)/(255-C), use the strongest channel.
    contrast = 255.0 - est
    ch_idx = contrast.argmax(axis=2)
    p = np.take_along_axis(rgb, ch_idx[..., None], axis=2)[..., 0]
    cc = np.take_along_axis(contrast, ch_idx[..., None], axis=2)[..., 0]
    a_band = np.clip((255.0 - p) / np.maximum(cc, 1.0), 0.0, 1.0)

    band = (np.asarray(fg_img.filter(ImageFilter.MaxFilter(5))) > 0) & ~interior
    alpha = np.where(interior, 1.0, np.where(band, a_band, 0.0))
    color = np.where(interior[..., None], rgb, est)

    out = np.dstack([color, alpha[..., None] * 255.0]).clip(0, 255).astype(np.uint8)
    return Image.fromarray(out, "RGBA")


def superellipse_mask(size: int, n: float = 5.0, ss: int = 4) -> Image.Image:
    """Continuous-corner squircle (close to Apple's icon shape)."""
    s = size * ss
    y, x = np.mgrid[0:s, 0:s].astype(np.float32)
    r = (s - 1) / 2.0
    v = np.abs((x - r) / r) ** n + np.abs((y - r) / r) ** n
    m = Image.fromarray(((v <= 1.0) * 255).astype(np.uint8), "L")
    return m.resize((size, size), Image.LANCZOS)


def vertical_gradient(size: tuple[int, int], top: tuple[int, int, int], bottom: tuple[int, int, int]) -> Image.Image:
    w, h = size
    t = np.linspace(0.0, 1.0, h, dtype=np.float32)[:, None, None]
    a = np.array(top, np.float32)[None, None, :]
    b = np.array(bottom, np.float32)[None, None, :]
    g = np.broadcast_to(a + (b - a) * t, (h, w, 3))
    return Image.fromarray(g.astype(np.uint8), "RGB").convert("RGBA")


def place_glyph(canvas: Image.Image, glyph: Image.Image, scale: float, center: tuple[float, float] | None = None) -> Image.Image:
    """Paste glyph so its bounding box spans `scale` of the canvas, optically centred."""
    bbox = glyph.getbbox()
    g = glyph.crop(bbox)
    W, H = canvas.size
    target = scale * min(W, H)
    f = target / max(g.size)
    g = g.resize((max(1, round(g.width * f)), max(1, round(g.height * f))), Image.LANCZOS)
    cx, cy = center or (W / 2, H / 2)
    out = canvas.copy()
    out.alpha_composite(g, (round(cx - g.width / 2), round(cy - g.height / 2)))
    return out


WHITE_TOP = (255, 255, 255)
WHITE_BOTTOM = (240, 243, 250)
NAVY_TOP = (22, 30, 58)
NAVY_BOTTOM = (9, 13, 28)

GLYPH_SCALE = 0.68  # matches the original artwork's proportion
SMALL_GLYPH_SCALE = 0.84  # bolder glyph for 16-32 px so it stays legible


def tile(size: int, glyph: Image.Image, scale: float = GLYPH_SCALE, shape: bool = True) -> Image.Image:
    bg = vertical_gradient((size, size), WHITE_TOP, WHITE_BOTTOM)
    art = place_glyph(bg, glyph, scale)
    if shape:
        art.putalpha(ImageChops.multiply(art.getchannel("A"), superellipse_mask(size)))
    return art


def macos_icon(glyph: Image.Image) -> Image.Image:
    """Big Sur grid: 824 px body centred in 1024 with a soft drop shadow."""
    body = 824
    canvas = Image.new("RGBA", (M, M), (0, 0, 0, 0))
    t = tile(body, glyph)
    off = (M - body) // 2
    shadow = Image.new("RGBA", (M, M), (0, 0, 0, 0))
    sh = Image.new("RGBA", (body, body), (0, 0, 0, 90))
    sh.putalpha(ImageChops.multiply(t.getchannel("A"), Image.new("L", (body, body), 90)))
    shadow.alpha_composite(sh, (off, off + 12))
    shadow = shadow.filter(ImageFilter.GaussianBlur(14))
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(t, (off, off))
    return canvas


def desktop_icon(glyph: Image.Image, size: int) -> Image.Image:
    """Windows / Linux: squircle with a little breathing room, no heavy shadow."""
    pad = round(size * 0.03)
    inner = size - 2 * pad
    scale = SMALL_GLYPH_SCALE if size <= 32 else GLYPH_SCALE
    c = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    c.alpha_composite(tile(inner * 4, glyph, scale).resize((inner, inner), Image.LANCZOS), (pad, pad))
    return c


# --------------------------------------------------------------------------- writers


def save(img: Image.Image, path: Path, size: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    if size:
        img = img.resize((size, size), Image.LANCZOS)
    img.save(path, optimize=True)


def build_icns(master: Image.Image, out: Path) -> None:
    with tempfile.TemporaryDirectory() as tmp:
        iconset = Path(tmp) / "icon.iconset"
        iconset.mkdir()
        for s in (16, 32, 128, 256, 512):
            save(master, iconset / f"icon_{s}x{s}.png", s)
            save(master, iconset / f"icon_{s}x{s}@2x.png", s * 2)
        subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(out)], check=True)


def build_ico(glyph: Image.Image, out: Path, sizes=(16, 24, 32, 48, 64, 128, 256)) -> None:
    imgs = [desktop_icon(glyph, s) for s in sizes]
    out.parent.mkdir(parents=True, exist_ok=True)
    imgs[-1].save(out, format="ICO", sizes=[(s, s) for s in sizes], append_images=imgs[:-1])


def desktop_targets(glyph: Image.Image, mac: Image.Image) -> None:
    TAURI_ICONS.mkdir(parents=True, exist_ok=True)
    big = desktop_icon(glyph, M)
    for name, s in {"32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512}.items():
        save(desktop_icon(glyph, s) if s <= 32 else big, TAURI_ICONS / name, None if s <= 32 else s)
    # Windows Store / MSIX logos (full-bleed squares are expected here).
    store = {"Square30x30Logo.png": 30, "Square44x44Logo.png": 44, "Square71x71Logo.png": 71, "Square89x89Logo.png": 89,
             "Square107x107Logo.png": 107, "Square142x142Logo.png": 142, "Square150x150Logo.png": 150,
             "Square284x284Logo.png": 284, "Square310x310Logo.png": 310, "StoreLogo.png": 50}
    for name, s in store.items():
        save(desktop_icon(glyph, s) if s <= 32 else big, TAURI_ICONS / name, None if s <= 32 else s)
    build_ico(glyph, TAURI_ICONS / "icon.ico")
    build_icns(mac, TAURI_ICONS / "icon.icns")


def ios_targets(glyph: Image.Image) -> None:
    IOS_ICONSET.mkdir(parents=True, exist_ok=True)
    light = tile(M, glyph, shape=False).convert("RGB")  # iOS masks the icon itself; must be opaque
    light.save(IOS_ICONSET / "AppIcon-1024.png", optimize=True)

    # Dark: glyph on transparency; the system supplies the dark backdrop.
    dark = place_glyph(Image.new("RGBA", (M, M), (0, 0, 0, 0)), glyph, GLYPH_SCALE)
    dark.save(IOS_ICONSET / "AppIcon-1024-dark.png", optimize=True)

    # Tinted: grayscale luminance glyph; the system applies the user's tint.
    g = glyph.copy()
    lum = g.convert("L").point(lambda v: min(255, int(v * 1.25)))
    tinted_glyph = Image.merge("RGBA", (lum, lum, lum, g.getchannel("A")))
    tinted = place_glyph(Image.new("RGBA", (M, M), (0, 0, 0, 255)), tinted_glyph, GLYPH_SCALE)
    tinted.convert("RGB").save(IOS_ICONSET / "AppIcon-1024-tinted.png", optimize=True)

    contents = {
        "images": [
            {"filename": "AppIcon-1024.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"},
            {"appearances": [{"appearance": "luminosity", "value": "dark"}], "filename": "AppIcon-1024-dark.png",
             "idiom": "universal", "platform": "ios", "size": "1024x1024"},
            {"appearances": [{"appearance": "luminosity", "value": "tinted"}], "filename": "AppIcon-1024-tinted.png",
             "idiom": "universal", "platform": "ios", "size": "1024x1024"},
        ],
        "info": {"author": "xcode", "version": 1},
    }
    (IOS_ICONSET / "Contents.json").write_text(json.dumps(contents, indent=2) + "\n")
    parent = IOS_ICONSET.parent / "Contents.json"
    if not parent.exists():
        parent.write_text(json.dumps({"info": {"author": "xcode", "version": 1}}, indent=2) + "\n")


def og_image(glyph: Image.Image) -> Image.Image:
    W, H = 1200, 630
    bg = vertical_gradient((W, H), NAVY_TOP, NAVY_BOTTOM)
    # soft brand glows
    glow = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    d = ImageDraw.Draw(glow)
    d.ellipse((-200, 250, 500, 900), fill=(52, 152, 219, 110))
    d.ellipse((700, -250, 1400, 400), fill=(247, 129, 112, 100))
    d.ellipse((350, 150, 850, 650), fill=(176, 132, 196, 70))
    bg.alpha_composite(glow.filter(ImageFilter.GaussianBlur(120)))
    icon = tile(300, glyph)
    bg.alpha_composite(icon, (110, (H - 300) // 2))
    d = ImageDraw.Draw(bg)
    try:
        title = ImageFont.truetype("/System/Library/Fonts/SFNS.ttf", 92)
        sub = ImageFont.truetype("/System/Library/Fonts/SFNS.ttf", 36)
    except OSError:
        title = sub = ImageFont.load_default()
    d.text((470, 220), "Dolphin Pod", font=title, fill=(255, 255, 255))
    d.text((474, 340), "Your pod of AI agents, in one chat.", font=sub, fill=(205, 214, 235))
    return bg.convert("RGB")


def web_targets(glyph: Image.Image) -> None:
    WEB_PUBLIC.mkdir(parents=True, exist_ok=True)
    build_ico(glyph, WEB_PUBLIC / "favicon.ico", sizes=(16, 32, 48))
    save(desktop_icon(glyph, 16), WEB_PUBLIC / "favicon-16.png")
    save(desktop_icon(glyph, 32), WEB_PUBLIC / "favicon-32.png")
    save(tile(M, glyph, shape=False).convert("RGB"), WEB_PUBLIC / "apple-touch-icon.png", 180)
    big = desktop_icon(glyph, M)
    save(big, WEB_PUBLIC / "icon-192.png", 192)
    save(big, WEB_PUBLIC / "icon-512.png", 512)
    # Maskable: full bleed, glyph inside the 80 % safe zone.
    save(tile(M, glyph, scale=0.6, shape=False).convert("RGB"), WEB_PUBLIC / "icon-maskable-512.png", 512)
    og_image(glyph).save(WEB_PUBLIC / "og-image.png", optimize=True)


def in_app_targets(glyph: Image.Image) -> None:
    DESKTOP_PUBLIC.mkdir(parents=True, exist_ok=True)
    g = glyph.crop(glyph.getbbox())
    f = 512 / max(g.size)
    g.resize((round(g.width * f), round(g.height * f)), Image.LANCZOS).save(DESKTOP_PUBLIC / "dolphin-glyph.png", optimize=True)
    save(desktop_icon(glyph, M), DESKTOP_PUBLIC / "dolphin-icon.png", 256)
    for name in ("mintflow-logo.webp", "mintflow-mark.webp"):
        shutil.copy(BRAND / "source" / name, DESKTOP_PUBLIC / name)


def main() -> None:
    GEN.mkdir(parents=True, exist_ok=True)
    src = Image.open(SRC)
    glyph_full = extract_glyph(src)
    glyph = glyph_full.resize((M * 2, M * 2), Image.LANCZOS) if glyph_full.width < M * 2 else glyph_full
    glyph.save(GEN / "glyph-master.png", optimize=True)

    mac = macos_icon(glyph)
    mac.save(GEN / "macos-1024.png", optimize=True)
    desktop_icon(glyph, M).save(GEN / "desktop-1024.png", optimize=True)

    desktop_targets(glyph, mac)
    ios_targets(glyph)
    web_targets(glyph)
    in_app_targets(glyph)
    print("icons generated")


if __name__ == "__main__":
    main()
