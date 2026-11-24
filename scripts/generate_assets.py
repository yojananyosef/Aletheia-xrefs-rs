#!/usr/bin/env python3
"""Generate the PWA icons and the social preview image.

Run from the repository root: python3 scripts/generate_assets.py
Requires Pillow. The design mirrors the arc mark used in the app header.
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
PUBLIC = ROOT / "crates/app/public"
ICONS = PUBLIC / "icons"
SERIF = "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf"
SERIF_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSerif-Bold.ttf"
SANS = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
SANS_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"

BG = (11, 12, 14)
TEXT = (243, 241, 234)
MUTED = (154, 160, 166)
GOLD = (228, 179, 99)
PINK = (235, 86, 143)
TEAL = (23, 167, 180)


def font(path: str, size: int) -> ImageFont.FreeTypeFont:
    return ImageFont.truetype(path, size)


def draw_mark(draw: ImageDraw.ImageDraw, size: int, scale: float = 1.0) -> None:
    """Three concentric arcs on a baseline, centred in a square."""
    s = size * scale
    cx, cy = s / 2, s * 0.77
    width = max(2, int(s * 0.042))
    for radius, color in ((s * 0.30, PINK), (s * 0.195, GOLD), (s * 0.09, TEAL)):
        box = (cx - radius, cy - radius, cx + radius, cy + radius)
        draw.arc(box, start=180, end=360, fill=color, width=width)
    draw.line(
        (s * 0.10, cy, s * 0.90, cy),
        fill=(*TEXT, 70),
        width=max(1, int(s * 0.006)),
    )


def icon(size: int, maskable: bool = False) -> Image.Image:
    scale = 4
    s = size * scale
    image = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    radius = 0 if maskable else int(s * 0.22)
    draw.rounded_rectangle((0, 0, s - 1, s - 1), radius=radius, fill=BG)
    draw_mark(draw, s, scale=0.78 if maskable else 0.82)
    return image.resize((size, size), Image.Resampling.LANCZOS)


def og_image() -> Image.Image:
    scale = 2
    w, h = 1200 * scale, 630 * scale
    image = Image.new("RGB", (w, h), BG)
    draw = ImageDraw.Draw(image, "RGBA")

    # Soft colour fields in the corners.
    for step in range(60):
        alpha = 1
        draw.ellipse(
            (
                -w * 0.25 - step * w * 0.006,
                -h * 0.55 - step * h * 0.006,
                w * 0.62 + step * w * 0.006,
                h * 0.55 + step * h * 0.006,
            ),
            fill=(235, 86, 143, alpha),
        )
        draw.ellipse(
            (
                w * 0.58 - step * w * 0.005,
                -h * 0.45 - step * h * 0.005,
                w * 1.22 + step * w * 0.005,
                h * 0.60 + step * h * 0.005,
            ),
            fill=(23, 167, 180, alpha),
        )

    # Arcs on the right, cropped by the canvas.
    cx, cy = w * 0.79, h * 0.96
    for radius, color in ((w * 0.42, PINK), (w * 0.30, GOLD), (w * 0.18, TEAL)):
        draw.arc(
            (cx - radius, cy - radius, cx + radius, cy + radius),
            start=180,
            end=360,
            fill=color,
            width=int(2.2 * scale),
        )
    draw.line(
        (w * 0.36, cy, w * 1.04, cy),
        fill=(255, 255, 255, 22),
        width=scale,
    )

    # Border and vignette.
    draw.rounded_rectangle(
        (scale * 10, scale * 10, w - scale * 10, h - scale * 10),
        radius=scale * 20,
        outline=(255, 255, 255, 18),
        width=scale,
    )
    for step in range(40):
        alpha = int(7 * (1 - step / 40))
        draw.rectangle(
            (
                step * scale * 3,
                step * scale * 2,
                w - step * scale * 3,
                h - step * scale * 2,
            ),
            outline=(0, 0, 0, alpha),
            width=scale * 3,
        )

    x = scale * 76
    draw.text(
        (x, scale * 74),
        "A L E T H E I A   A R C S",
        font=font(SANS_BOLD, 17 * scale),
        fill=GOLD,
    )
    draw.text(
        (x, scale * 118),
        "Referencias cruzadas",
        font=font(SERIF_BOLD, 62 * scale),
        fill=TEXT,
    )
    draw.text(
        (x, scale * 192),
        "de la Biblia",
        font=font(SERIF_BOLD, 62 * scale),
        fill=TEXT,
    )
    draw.text(
        (x, scale * 300),
        "31.102 versículos · 344.799 referencias",
        font=font(SANS, 21 * scale),
        fill=MUTED,
    )
    draw.text(
        (x, scale * 334),
        "Biblia Platense (Straubinger) + American Standard Version",
        font=font(SANS, 19 * scale),
        fill=MUTED,
    )

    badge = "Rust + WebAssembly"
    badge_font = font(SANS_BOLD, 14 * scale)
    tw = draw.textlength(badge, font=badge_font)
    bx, by = x, scale * 414
    draw.rounded_rectangle(
        (bx, by, bx + tw + scale * 34, by + scale * 38),
        radius=scale * 19,
        fill=(228, 179, 99, 18),
        outline=(228, 179, 99, 80),
        width=scale,
    )
    draw.text((bx + scale * 17, by + scale * 11), badge, font=badge_font, fill=GOLD)

    draw.text(
        (x, h - scale * 76),
        "aletheia arcs · referencias cruzadas · visualizador web",
        font=font(SANS, 14 * scale),
        fill=(109, 115, 121),
    )
    return image.resize((1200, 630), Image.Resampling.LANCZOS)


def main() -> None:
    ICONS.mkdir(parents=True, exist_ok=True)
    icon(192).save(ICONS / "icon-192.png")
    icon(512).save(ICONS / "icon-512.png")
    icon(512, maskable=True).save(ICONS / "icon-maskable-512.png")
    icon(180).save(ICONS / "apple-touch-icon.png")
    og_image().save(PUBLIC / "og-image.png")
    print(f"wrote {PUBLIC}")


if __name__ == "__main__":
    main()
