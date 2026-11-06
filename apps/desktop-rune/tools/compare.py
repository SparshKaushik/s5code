"""compare.py REF.png RUNE.png OUT.png [x0,y0,x1,y1]
Side-by-side (ref | rune | diff heatmap) plus a score: share of pixels whose max channel delta > 24."""
import sys
from PIL import Image, ImageChops

ref = Image.open(sys.argv[1]).convert("RGB")
rune = Image.open(sys.argv[2]).convert("RGB").resize(ref.size)
if len(sys.argv) > 4:
    box = tuple(int(v) for v in sys.argv[4].split(","))
    ref, rune = ref.crop(box), rune.crop(box)
diff = ImageChops.difference(ref, rune)
mask = diff.convert("L").point(lambda v: 255 if v > 24 else 0)
bad = sum(1 for v in mask.getdata() if v)
total = ref.size[0] * ref.size[1]
heat = Image.composite(Image.new("RGB", ref.size, (255, 40, 90)), ref.point(lambda v: v // 4), mask)
w, h = ref.size
out = Image.new("RGB", (w * 3 + 16, h), (40, 40, 40))
out.paste(ref, (0, 0))
out.paste(rune, (w + 8, 0))
out.paste(heat, (w * 2 + 16, 0))
out.save(sys.argv[3])
print(f"mismatch {100 * bad / total:.2f}% of {total} px -> {sys.argv[3]}")
