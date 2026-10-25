#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
# Copyright (c) 2026 tawan475

"""Draw Teyvat.me's app icon and write every size the builds use.

The brand mark is the amber four-point star (the one `ui/widgets.rs` `star`
draws) with its glow, on a deep slate tile. Drawn at 4x and scaled down, so
the edges are smooth at every size.

    python apps/desktop/assets/make_icons.py

writes, next to this script:
  icon-1024.png  the master (the macOS release job makes AppIcon.icns from it)
  icon-256.png   the window and tray icon
  icon.ico       the Windows executable's icon (16 to 256 px)
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

HERE = Path(__file__).resolve().parent
SIZE = 1024
SCALE = 4

TILE_TOP = (30, 41, 59)  # slate-800
TILE_BOTTOM = (15, 23, 42)  # slate-900
AMBER = (245, 158, 11)
AMBER_LIGHT = (252, 211, 77)


def star(cx: float, cy: float, outer: float, inner: float) -> list[tuple[float, float]]:
    """The four-point star: points at the compass tips, waist at the diagonals."""
    points = []
    for i in range(8):
        angle = -math.pi / 2 + i * math.pi / 4
        radius = outer if i % 2 == 0 else inner
        points.append((cx + math.cos(angle) * radius, cy + math.sin(angle) * radius))
    return points


def draw() -> Image.Image:
    big = SIZE * SCALE
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))

    # The tile: a rounded square (macOS-style margin) with a vertical gradient.
    margin = int(big * 0.08)
    radius = int(big * 0.22)
    gradient = Image.new("RGBA", (big, big))
    pixels = gradient.load()
    for y in range(big):
        t = y / (big - 1)
        colour = tuple(round(a + (b - a) * t) for a, b in zip(TILE_TOP, TILE_BOTTOM))
        for x in range(big):
            pixels[x, y] = (*colour, 255)
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        (margin, margin, big - margin, big - margin), radius=radius, fill=255
    )
    img.paste(gradient, (0, 0), mask)

    centre = big / 2
    outer = big * 0.30
    inner = big * 0.075

    # The glow behind the star.
    glow = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    ImageDraw.Draw(glow).ellipse(
        (centre - outer * 0.9, centre - outer * 0.9, centre + outer * 0.9, centre + outer * 0.9),
        fill=(*AMBER, 110),
    )
    glow = glow.filter(ImageFilter.GaussianBlur(big * 0.06))
    glow_masked = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    glow_masked.paste(glow, (0, 0), mask)
    img = Image.alpha_composite(img, glow_masked)

    # The star, lighter at its top for a little depth.
    star_layer = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    ImageDraw.Draw(star_layer).polygon(star(centre, centre, outer, inner), fill=(*AMBER, 255))
    shade = Image.new("RGBA", (big, big))
    shade_pixels = shade.load()
    top, bottom = centre - outer, centre + outer
    for y in range(big):
        t = min(max((y - top) / (bottom - top), 0.0), 1.0)
        colour = tuple(round(a + (b - a) * t) for a, b in zip(AMBER_LIGHT, AMBER))
        for x in range(big):
            shade_pixels[x, y] = (*colour, 255)
    star_mask = star_layer.split()[3]
    star_shaded = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    star_shaded.paste(shade, (0, 0), star_mask)
    img = Image.alpha_composite(img, star_shaded)

    return img.resize((SIZE, SIZE), Image.LANCZOS)


def main() -> None:
    master = draw()
    master.save(HERE / "icon-1024.png", optimize=True)
    master.resize((256, 256), Image.LANCZOS).save(HERE / "icon-256.png", optimize=True)
    master.save(
        HERE / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("wrote icon-1024.png, icon-256.png, icon.ico")


if __name__ == "__main__":
    main()
