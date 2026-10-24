"""Renders YARTT's icons: a monitor with a thin 8-ray sun.

    python -I assets/icons/render.py

Writes assets/app.png (exe and settings window) and assets/icons/tray_{light,dark}_{size}.png.
Each tray size is drawn separately with its own line width, so it stays crisp instead of being
scaled down from one large image. "light" is the dark glyph for a light taskbar.
Needs Pillow.
"""
from math import sqrt
from pathlib import Path

from PIL import Image, ImageDraw

ASSETS = Path(__file__).resolve().parent.parent
SUPERSAMPLE = 16

# Per tray size, in units of the 24-unit design grid: (line width, ray width, sun radius).
# The smaller sizes get a bolder sun, since rays under a pixel wide blur into a grey smudge.
TRAY_SIZES = {20: (1.8, 1.4, 1.8), 24: (1.6, 1.0, 1.5), 32: (1.6, 1.0, 1.5)}

# At 16 px (100% scaling) the screen is about 10x7 pixels inside, too small for a drawn sun to
# survive antialiasing, so that size is pixel art instead. The screen is 13 wide so the sun
# has a centre pixel.
PIXELS_16 = """
................
................
..###########...
.#...........#..
.#.....#.....#..
.#...#...#...#..
.#....###....#..
.#..#.###.#..#..
.#....###....#..
.#...#...#...#..
.#.....#.....#..
.#...........#..
..###########...
.......#........
.....#####......
................
"""
APP_SIZE = 256

WHITE = (255, 255, 255, 255)
BLACK = (26, 26, 26, 255)
TILE = (44, 44, 42, 255)
AMBER = (250, 199, 117, 255)

D = 1 / sqrt(2)
RAYS = [(0, -1), (0, 1), (-1, 0), (1, 0), (-D, -D), (D, D), (-D, D), (D, -D)]


class Canvas:
    """Draws on the 24-unit grid, oversampled so the downscale antialiases it."""

    def __init__(self, size, origin=(0.0, 0.0), scale=1.0):
        self.size = size
        self.image = Image.new("RGBA", (size * SUPERSAMPLE,) * 2, (0, 0, 0, 0))
        self.draw = ImageDraw.Draw(self.image)
        self.origin = origin
        self.scale = scale

    def px(self, u):
        return u * self.scale * self.size / 24 * SUPERSAMPLE

    def pt(self, x, y):
        return (self.px(self.origin[0] / self.scale + x), self.px(self.origin[1] / self.scale + y))

    def rounded_rect(self, x, y, w, h, r, color, stroke=None):
        if stroke is None:
            self.draw.rounded_rectangle([self.pt(x, y), self.pt(x + w, y + h)], radius=self.px(r), fill=color)
            return
        # Pillow strokes inward, so grow the box by half the width to centre the line on it
        half = stroke / 2
        self.draw.rounded_rectangle(
            [self.pt(x - half, y - half), self.pt(x + w + half, y + h + half)],
            radius=self.px(r + half), outline=color, width=round(self.px(stroke)))

    def line(self, x1, y1, x2, y2, color, stroke):
        a, b = self.pt(x1, y1), self.pt(x2, y2)
        self.draw.line([a, b], fill=color, width=round(self.px(stroke)))
        r = self.px(stroke) / 2
        for cx, cy in (a, b):
            self.draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)

    def dot(self, x, y, radius, color):
        cx, cy = self.pt(x, y)
        r = self.px(radius)
        self.draw.ellipse([cx - r, cy - r, cx + r, cy + r], fill=color)

    def save(self, path):
        self.image.resize((self.size, self.size), Image.LANCZOS).save(path)


def monitor(c, color, stroke, ray_width, sun_radius, sun_color=None):
    sun_color = sun_color or color
    c.rounded_rect(2.5, 3.5, 19, 13, 2, color, stroke)
    c.line(12, 16.5, 12, 20.5, color, stroke)
    c.line(8.5, 20.5, 15.5, 20.5, color, stroke)
    c.dot(12, 10, sun_radius, sun_color)
    inner = sun_radius + 1.6
    for dx, dy in RAYS:
        c.line(12 + dx * inner, 10 + dy * inner, 12 + dx * 4.4, 10 + dy * 4.4, sun_color, ray_width)


def main():
    rows = PIXELS_16.strip().splitlines()
    for name, color in (("light", BLACK), ("dark", WHITE)):
        image = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
        for y, row in enumerate(rows):
            for x, cell in enumerate(row):
                if cell == "#":
                    image.putpixel((x, y), color)
        image.save(ASSETS / "icons" / f"tray_{name}_16.png")

    for size, (stroke, ray_width, sun_radius) in TRAY_SIZES.items():
        for name, color in (("light", BLACK), ("dark", WHITE)):
            c = Canvas(size)
            monitor(c, color, stroke, ray_width, sun_radius)
            c.save(ASSETS / "icons" / f"tray_{name}_{size}.png")

    app = Canvas(APP_SIZE)
    app.rounded_rect(0, 0, 24, 24, 5, TILE)
    glyph = Canvas(APP_SIZE, origin=(3, 3), scale=0.75)
    glyph.image = app.image
    glyph.draw = app.draw
    monitor(glyph, WHITE, 1.6, 1.0, 1.5, AMBER)
    app.save(ASSETS / "app.png")


if __name__ == "__main__":
    main()
