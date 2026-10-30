"""Draw the app icon (src/calcbridge/assets/icon.png, 1024 px). Run once; the PNG is committed."""
import os

from PIL import Image, ImageDraw

S = 1024
OUT = os.path.join(os.path.dirname(__file__), "..", "src", "calcbridge", "assets", "icon.png")


def lerp(a, b, t):
    return tuple(int(x + (y - x) * t) for x, y in zip(a, b))


img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
bg = Image.new("RGBA", (S, S))
top, bottom = (39, 110, 230), (14, 48, 122)
for y in range(S):
    ImageDraw.Draw(bg).line([(0, y), (S, y)], fill=lerp(top, bottom, y / S) + (255,))
mask = Image.new("L", (S, S), 0)
ImageDraw.Draw(mask).rounded_rectangle([40, 40, S - 40, S - 40], radius=200, fill=255)
img.paste(bg, (0, 0), mask)
d = ImageDraw.Draw(img)

# calculator body
d.rounded_rectangle([300, 170, 724, 870], radius=70, fill=(245, 247, 252, 255))
# screen
d.rounded_rectangle([350, 225, 674, 455], radius=24, fill=(28, 36, 52, 255))
# a little graph on the screen
pts = [(375 + i * 3, 400 - int(130 * ((i / 95.0 - 0.45) ** 2) * 4)) for i in range(96)]
d.line(pts, fill=(90, 200, 255, 255), width=12, joint="curve")
d.line([(370, 410), (655, 410)], fill=(120, 130, 150, 255), width=5)
# keypad
for r in range(4):
    for c in range(3):
        x, y = 372 + c * 100, 510 + r * 85
        d.rounded_rectangle([x, y, x + 78, y + 58], radius=16,
                            fill=(39, 110, 230, 255) if (r, c) == (3, 2) else (196, 205, 222, 255))
# link arrows (the "bridge") on the lower right
d.ellipse([650, 650, 900, 900], fill=(255, 200, 40, 255))
d.polygon([(700, 745), (800, 745), (800, 715), (850, 765), (800, 815), (800, 785), (700, 785)], fill=(14, 48, 122, 255))

os.makedirs(os.path.dirname(OUT), exist_ok=True)
img.save(OUT)
img.resize((256, 256), Image.LANCZOS).save(OUT.replace("icon.png", "icon-256.png"))
print("wrote", os.path.abspath(OUT))
