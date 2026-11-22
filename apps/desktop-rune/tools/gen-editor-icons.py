"""Editor marks for Open in editor, from apps/web's Icons.tsx and JetBrainsIcons.tsx.

Rune icons are one colour each, on a square viewBox from 0, with no groups or transforms, so
every shape becomes its own layer: geometry baked into path data, a gradient taking its
midpoint colour. Masks, filters and blend-mode overlays (VS Code's shading) are dropped.

    python3 -I tools/gen-editor-icons.py > src/chat/details/editor_icons.rs
"""

import math
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

WEB = Path(__file__).resolve().parents[2] / "web" / "src" / "components"

# (editor id, file, component)
MARKS = [
    ("cursor", "Icons.tsx", "CursorIcon"),
    ("trae", "Icons.tsx", "TraeIcon"),
    ("kiro", "Icons.tsx", "KiroIcon"),
    ("vscode", "Icons.tsx", "VisualStudioCode"),
    ("vscode-insiders", "Icons.tsx", "VisualStudioCodeInsiders"),
    ("vscodium", "Icons.tsx", "VSCodium"),
    ("zed", "Icons.tsx", "Zed"),
    ("idea", "JetBrainsIcons.tsx", "IntelliJIdeaIcon"),
    ("aqua", "JetBrainsIcons.tsx", "AquaIcon"),
    ("clion", "JetBrainsIcons.tsx", "CLionIcon"),
    ("datagrip", "JetBrainsIcons.tsx", "DataGripIcon"),
    ("dataspell", "JetBrainsIcons.tsx", "DataSpellIcon"),
    ("goland", "JetBrainsIcons.tsx", "GoLandIcon"),
    ("phpstorm", "JetBrainsIcons.tsx", "PhpStormIcon"),
    ("pycharm", "JetBrainsIcons.tsx", "PyCharmIcon"),
    ("rider", "JetBrainsIcons.tsx", "RiderIcon"),
    ("rubymine", "JetBrainsIcons.tsx", "RubyMineIcon"),
    ("rustrover", "JetBrainsIcons.tsx", "RustRoverIcon"),
    ("webstorm", "JetBrainsIcons.tsx", "WebStormIcon"),
    ("finder", "Icons.tsx", "FinderIcon"),
    ("file-explorer", "Icons.tsx", "FileExplorerIcon"),
]
# Marks web colours through a class, as (light, dark).
CLASS_FILL = {"CursorIcon": ("#26251E", "#EDECEC")}
INK = "currentColor"


def component_svg(source: str, name: str) -> str:
    start = re.search(rf"export const {name}\b", source).start()
    nxt = re.search(r"\nexport const ", source[start + 1 :])
    block = source[start : start + 1 + nxt.start()] if nxt else source[start:]
    return block[block.index("<svg") : block.rindex("</svg>") + len("</svg>")]


def jsx_to_xml(jsx: str) -> str:
    s = re.sub(r"\{/\*.*?\*/\}", "", jsx, flags=re.S)
    s = s.replace("{...props}", "")
    s = re.sub(r"className=\{[^}]*\}\)?\}?", "", s)
    s = re.sub(r'className="[^"]*"', "", s)
    s = re.sub(r"style=\{\{\s*mixBlendMode:[^}]*\}\}", 'data-skip="1"', s)
    s = re.sub(r"\{`url\(#\$\{(\w+)\}\)`\}", r'"url(#\1)"', s)
    s = re.sub(r"=\{(\w+)\}", r'="\1"', s)
    s = re.sub(r"\{\}", "", s)
    keep = {"viewBox", "gradientUnits", "maskUnits", "filterUnits", "gradientTransform", "preserveAspectRatio"}
    kebab = lambda name: name if name in keep else re.sub(r"([A-Z])", lambda c: "-" + c.group(1).lower(), name)
    s = re.sub(r" ([a-z]+(?:[A-Z][a-z]*)+)=", lambda m: " " + kebab(m.group(1)) + "=", s)
    return s


# --- path geometry -----------------------------------------------------------------------------

ARGS = {"M": 2, "L": 2, "H": 1, "V": 1, "C": 6, "S": 4, "Q": 4, "T": 2, "A": 7, "Z": 0}
NUM = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")


def parse_path(d: str):
    """Absolute segments [(cmd, [floats])], with H/V turned into L."""
    out = []
    i, cmd = 0, None
    x = y = sx = sy = 0.0
    while i < len(d):
        c = d[i]
        if c in " ,\t\n\r":
            i += 1
            continue
        if c.upper() in ARGS:
            cmd = c
            i += 1
            if c.upper() == "Z":
                out.append(("Z", []))
                x, y = sx, sy
            continue
        up = cmd.upper()
        args = []
        for k in range(ARGS[up]):
            while i < len(d) and d[i] in " ,\t\n\r":
                i += 1
            if up == "A" and k in (3, 4):
                args.append(float(d[i]))
                i += 1
                continue
            m = NUM.match(d, i)
            args.append(float(m.group()))
            i = m.end()
        rel = cmd.islower()
        if up == "M":
            if rel:
                args = [args[0] + x, args[1] + y]
            x, y = args
            sx, sy = x, y
            out.append(("M", args))
            cmd = "l" if rel else "L"
        elif up == "L" or up == "T":
            if rel:
                args = [args[0] + x, args[1] + y]
            x, y = args
            out.append((up, args))
        elif up == "H":
            nx = args[0] + x if rel else args[0]
            out.append(("L", [nx, y]))
            x = nx
        elif up == "V":
            ny = args[0] + y if rel else args[0]
            out.append(("L", [x, ny]))
            y = ny
        elif up in ("C", "S", "Q"):
            if rel:
                args = [a + (x if j % 2 == 0 else y) for j, a in enumerate(args)]
            x, y = args[-2], args[-1]
            out.append((up, args))
        elif up == "A":
            if rel:
                args[5] += x
                args[6] += y
            x, y = args[5], args[6]
            out.append(("A", args))
    return out


def apply(m, px, py):
    a, b, c, d, e, f = m
    return a * px + c * py + e, b * px + d * py + f


def mul(m1, m2):
    a1, b1, c1, d1, e1, f1 = m1
    a2, b2, c2, d2, e2, f2 = m2
    return (a1 * a2 + c1 * b2, b1 * a2 + d1 * b2, a1 * c2 + c1 * d2, b1 * c2 + d1 * d2, a1 * e2 + c1 * f2 + e1, b1 * e2 + d1 * f2 + f1)


def parse_transform(t: str):
    m = (1, 0, 0, 1, 0, 0)
    for kind, raw in re.findall(r"(\w+)\(([^)]*)\)", t or ""):
        v = [float(n) for n in NUM.findall(raw)]
        if kind == "translate":
            step = (1, 0, 0, 1, v[0], v[1] if len(v) > 1 else 0)
        elif kind == "scale":
            step = (v[0], 0, 0, v[1] if len(v) > 1 else v[0], 0, 0)
        elif kind == "matrix":
            step = tuple(v)
        elif kind == "rotate":
            r = math.radians(v[0])
            step = (math.cos(r), math.sin(r), -math.sin(r), math.cos(r), 0, 0)
        else:
            raise SystemExit(f"transform {kind} unsupported")
        m = mul(m, step)
    return m


def fmt(n: float) -> str:
    s = f"{n:.3f}".rstrip("0").rstrip(".")
    return "0" if s in ("-0", "") else s


def transform_path(segs, m) -> str:
    det = m[0] * m[3] - m[1] * m[2]
    scale = math.sqrt(abs(det))
    angle = math.degrees(math.atan2(m[1], m[0]))
    out = []
    for cmd, a in segs:
        if cmd == "Z":
            out.append("Z")
        elif cmd == "A":
            ex, ey = apply(m, a[5], a[6])
            sweep = a[4] if det > 0 else 1 - a[4]
            out.append(f"A{fmt(a[0] * scale)} {fmt(a[1] * scale)} {fmt(a[2] + angle)} {int(a[3])} {int(sweep)} {fmt(ex)} {fmt(ey)}")
        else:
            pts = [apply(m, a[j], a[j + 1]) for j in range(0, len(a), 2)]
            out.append(cmd + " ".join(f"{fmt(px)} {fmt(py)}" for px, py in pts))
    return "".join(out)


def shape_d(el) -> str:
    tag = el.tag
    n = lambda k, dflt=0.0: float(el.get(k, dflt))
    if tag == "path":
        return el.get("d")
    if tag == "rect":
        x, y, w, h = n("x"), n("y"), n("width"), n("height")
        rx = el.get("rx", el.get("ry"))
        r = min(float(rx), w / 2, h / 2) if rx else 0
        if not r:
            return f"M{x} {y}H{x + w}V{y + h}H{x}Z"
        return (f"M{x + r} {y}H{x + w - r}A{r} {r} 0 0 1 {x + w} {y + r}V{y + h - r}A{r} {r} 0 0 1 {x + w - r} {y + h}"
                f"H{x + r}A{r} {r} 0 0 1 {x} {y + h - r}V{y + r}A{r} {r} 0 0 1 {x + r} {y}Z")
    if tag in ("circle", "ellipse"):
        cx, cy = n("cx"), n("cy")
        rx, ry = (n("r"), n("r")) if tag == "circle" else (n("rx"), n("ry"))
        return f"M{cx - rx} {cy}A{rx} {ry} 0 1 0 {cx + rx} {cy}A{rx} {ry} 0 1 0 {cx - rx} {cy}Z"
    raise SystemExit(f"shape {tag} unsupported")


# --- paint -------------------------------------------------------------------------------------

def hex_rgb(c: str):
    c = c.lstrip("#")
    if len(c) == 3:
        c = "".join(ch * 2 for ch in c)
    return tuple(int(c[i : i + 2], 16) for i in (0, 2, 4))


def gradient_mid(grad) -> tuple:
    """(hex, opacity) where the gradient is halfway along."""
    stops = []
    for st in grad.findall("stop"):
        off = st.get("offset", "0")
        off = float(off[:-1]) / 100 if off.endswith("%") else float(off)
        stops.append((off, hex_rgb(NAMED.get(st.get("stop-color", "#000"), st.get("stop-color", "#000"))), float(st.get("stop-opacity", 1))))
    stops.sort()
    lo = max((s for s in stops if s[0] <= 0.5), default=stops[0])
    hi = min((s for s in stops if s[0] >= 0.5), default=stops[-1])
    t = 0 if hi[0] == lo[0] else (0.5 - lo[0]) / (hi[0] - lo[0])
    rgb = tuple(round(lo[1][k] + (hi[1][k] - lo[1][k]) * t) for k in range(3))
    return "#%02X%02X%02X" % rgb, lo[2] + (hi[2] - lo[2]) * t


NAMED = {"#fff": "#FFFFFF", "#000": "#000000", "white": "#FFFFFF", "black": "#000000"}


def resolve(paint, grads):
    if paint is None or paint == "none":
        return None
    if paint == "currentColor":
        return INK, 1.0
    m = re.match(r"url\(#(\w+)\)", paint)
    if m:
        return gradient_mid(grads[m.group(1)])
    if re.fullmatch(r"#[0-9A-Fa-f]{8}", paint):
        return "#" + paint[1:7].upper(), int(paint[7:9], 16) / 255
    p = NAMED.get(paint, paint)
    return "#%02X%02X%02X" % hex_rgb(p), 1.0


SKIP = {"defs", "mask", "clipPath", "filter", "image", "linearGradient", "radialGradient", "title"}


def layers(svg, name):
    grads = {g.get("id"): g for g in svg.iter() if g.tag in ("linearGradient", "radialGradient")}
    vb = [float(v) for v in NUM.findall(svg.get("viewBox"))]
    minx, miny, w, h = vb
    view = max(w, h)
    base = (1, 0, 0, 1, -minx + (view - w) / 2, -miny + (view - h) / 2)
    out = []

    def walk(el, m, inherited):
        for child in el:
            if child.tag in SKIP or child.get("data-skip"):
                continue
            paint = dict(inherited)
            for k in ("fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin", "fill-rule", "opacity"):
                if child.get(k) is not None:
                    paint[k] = child.get(k)
            cm = mul(m, parse_transform(child.get("transform")))
            if child.tag == "g":
                walk(child, cm, paint)
                continue
            d = transform_path(parse_path(shape_d(child)), cm)
            opacity = float(paint.get("opacity", 1))
            fill = resolve(paint.get("fill", "#000000"), grads)
            if name in CLASS_FILL and paint.get("fill") is None:
                fill = ("class", 1.0)
            if fill:
                rule = ' fill-rule="evenodd"' if paint.get("fill-rule") == "evenodd" else ""
                alpha = opacity * fill[1]
                op = f' fill-opacity="{fmt(alpha)}"' if alpha < 1 else ""
                out.append((f'<path d="{d}"{rule}{op} fill="currentColor"/>', fill[0]))
            stroke = resolve(paint.get("stroke"), grads)
            if stroke:
                width = float(paint.get("stroke-width", 1)) * math.sqrt(abs(cm[0] * cm[3] - cm[1] * cm[2]))
                caps = "".join(f' {k}="{paint[k]}"' for k in ("stroke-linecap", "stroke-linejoin") if k in paint)
                alpha = opacity * stroke[1]
                op = f' stroke-opacity="{fmt(alpha)}"' if alpha < 1 else ""
                out.append((f'<path d="{d}" fill="none" stroke="currentColor" stroke-width="{fmt(width)}"{caps}{op}/>', stroke[0]))

    walk(svg, base, {k: svg.get(k) for k in ("fill",) if svg.get(k) is not None})
    return view, out


def main():
    print("//! Editor marks from apps/web Icons.tsx and JetBrainsIcons.tsx. Generated by")
    print("//! tools/gen-editor-icons.py: one single-colour layer per shape. Do not edit.")
    print("use rune::Icon;\n")
    table = []
    for editor, file, comp in MARKS:
        source = (WEB / file).read_text()
        svg = ET.fromstring(jsx_to_xml(component_svg(source, comp)))
        view, parts = layers(svg, comp)
        merged = []
        for body, colour in parts:
            if merged and merged[-1][1] == colour:
                merged[-1] = (merged[-1][0] + body, colour)
            else:
                merged.append((body, colour))
        parts = merged
        const = editor.upper().replace("-", "_")
        rows = []
        for i, (body, colour) in enumerate(parts):
            body = body.replace("\\", "\\\\").replace('"', '\\"')
            print(f'static {const}_{i}: Icon = Icon {{ name: "editor-{editor}-{i}", view: {fmt(view) if "." in fmt(view) else fmt(view) + ".0"}, body: "{body}" }};')
            light, dark = CLASS_FILL[comp] if colour == "class" else (colour, colour)
            rows.append(f'(&{const}_{i}, "{light}", "{dark}")')
        table.append((editor, const, rows))
    print("\ntype Layer = (&'static Icon, &'static str, &'static str);\n")
    for editor, const, rows in table:
        print(f"static {const}: [Layer; {len(rows)}] = [{', '.join(rows)}];")
    print("\n/// Each mark's layers, bottom first: glyph, light-theme colour, dark-theme colour.")
    print('/// `currentColor` takes the foreground, as web\'s `text-foreground` does.')
    print("pub fn layers(editor: &str) -> Option<&'static [Layer]> {")
    print("    Some(match editor {")
    for editor, const, rows in table:
        print(f'        "{editor}" => &{const},')
    print("        _ => return None,")
    print("    })")
    print("}")


if __name__ == "__main__":
    sys.exit(main())
