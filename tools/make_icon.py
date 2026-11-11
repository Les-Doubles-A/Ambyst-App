"""Generate the Ambyst-App icon (flat, no gradient). Usage: python tools/make_icon.py"""
from pathlib import Path
from PIL import Image, ImageDraw

S = 1024
out = Path(__file__).resolve().parent.parent / "assets"
img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
d = ImageDraw.Draw(img)
d.rounded_rectangle([40, 40, S - 40, S - 40], radius=200, fill=(30, 30, 34, 255))

# Flat crystal: top crown + bottom point, two flat tones
cx = S // 2
top, mid, bot = 270, 430, 800
lft, rgt = 250, S - 250
crown_l, crown_r = 360, S - 360
d.polygon([(crown_l, top), (crown_r, top), (rgt, mid), (lft, mid)], fill=(167, 139, 250, 255))
d.polygon([(lft, mid), (rgt, mid), (cx, bot)], fill=(124, 92, 214, 255))
d.polygon([(cx, mid), (rgt, mid), (cx, bot)], fill=(104, 74, 190, 255))

img.save(out / "icon.png")
small = img.resize((256, 256), Image.LANCZOS)
small.save(out / "icon-256.png")
(out / "icon-128.rgba").write_bytes(img.resize((128, 128), Image.LANCZOS).tobytes())
img.save(out / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("ok")
