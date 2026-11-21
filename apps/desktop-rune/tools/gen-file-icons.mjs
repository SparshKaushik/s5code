// Emit web's file-type icons (`apps/web/src/pierre-icons.ts` over `@pierre/trees`' "complete"
// set) as Rune data: icon bodies, resolver tables, colours, and web's own answers for every rule
// key so the Rust lookup is checked against the resolver it ports.
// usage (from apps/web): node ../desktop-rune/tools/gen-file-icons.mjs <repo> > ../desktop-rune/src/file_icons/data.rs
import * as NodeFS from "node:fs";
import * as NodePath from "node:path";
import * as NodeVM from "node:vm";

const { readFileSync, realpathSync } = NodeFS;
const { join, dirname } = NodePath;

const repo = process.argv[2];
const web = await import(join(repo, "apps/web/src/pierre-icons.ts"));
const treesDir = dirname(
  realpathSync(join(repo, "apps/web/node_modules/@pierre/trees/package.json")),
);
const trees = await import(join(treesDir, "dist/index.js"));
const builtIn = readFileSync(join(treesDir, "dist/builtInIcons.js"), "utf8");
const table = (name) =>
  NodeVM.runInNewContext(
    "(" + builtIn.match(new RegExp(`const ${name} = (\\{[\\s\\S]*?\\n\\});`))[1] + ")",
  );
const NAME_TOKENS = table("BUILT_IN_FILE_NAME_TOKENS");
const EXTENSION_TOKENS = table("BUILT_IN_FILE_EXTENSION_TOKENS");
const OVERRIDES = table("COMPLETE_EXTENSION_OVERRIDES");
const entryIcon = readFileSync(
  join(repo, "apps/web/src/components/chat/PierreEntryIcon.tsx"),
  "utf8",
);
const COLORS = Object.fromEntries(
  [...entryIcon.matchAll(/^\s+"?([\w-]+)"?: \["(#[0-9a-f]+)", "(#[0-9a-f]+)"\],$/gm)].map((m) => [
    m[1],
    [m[2], m[3]],
  ]),
);

const sprite = trees.getBuiltInSpriteSheet("complete") + web.T3_PIERRE_ICONS.spriteSheet;
const symbols = new Map();
for (const m of sprite.matchAll(
  /<symbol id="([^"]+)" viewBox="0 0 (\d+) \d+">([\s\S]*?)<\/symbol>/g,
))
  symbols.set(m[1], { view: Number(m[2]), inner: m[3] });

const DRAWN = new Set(["path", "rect", "circle", "ellipse", "line"]);
const KEEP = [
  "d",
  "x",
  "y",
  "width",
  "height",
  "rx",
  "ry",
  "cx",
  "cy",
  "r",
  "x1",
  "y1",
  "x2",
  "y2",
  "fill",
  "stroke",
  "stroke-width",
  "stroke-linecap",
  "stroke-linejoin",
  "fill-rule",
  "opacity",
  "fill-opacity",
  "stroke-opacity",
];
const INHERITED = [
  "fill",
  "stroke",
  "stroke-width",
  "stroke-linecap",
  "stroke-linejoin",
  "fill-rule",
];
const attrsOf = (s) => {
  const out = {};
  for (const m of s.matchAll(/([\w-]+)="([^"]*)"/g)) out[m[1]] = m[2];
  for (const decl of (out.style ?? "").split(";")) {
    const i = decl.indexOf(":");
    if (i > 0) out[decl.slice(0, i).trim()] = decl.slice(i + 1).trim();
  }
  return out;
};
// Rune icons are one colour: a painted part is currentColor, and its own colour tints the icon.
const colorOf = (v) => {
  const pair = v.match(/light-dark\((#[0-9a-fA-F]+),\s*(#[0-9a-fA-F]+)\)/);
  if (pair) return [pair[1].toLowerCase(), pair[2].toLowerCase()];
  const hex = v.match(/^#[0-9a-fA-F]{3,8}$/);
  return hex ? [v.toLowerCase(), v.toLowerCase()] : null;
};
function convert(inner) {
  let fixed = null;
  const stack = [{}];
  const parts = [];
  let defs = 0;
  for (const m of inner.matchAll(/<(\/?)([\w]+)([^>]*?)(\/?)>/g)) {
    const [, close, tag, rest, selfClose] = m;
    if (tag === "defs") {
      defs += close ? -1 : selfClose ? 0 : 1;
      continue;
    }
    if (defs > 0) continue;
    if (tag === "g") {
      if (close) stack.pop();
      else if (!selfClose)
        stack.push({
          ...stack.at(-1),
          ...Object.fromEntries(
            Object.entries(attrsOf(rest)).filter(([k]) => INHERITED.includes(k) || k === "opacity"),
          ),
        });
      continue;
    }
    if (close || !DRAWN.has(tag)) continue;
    const a = { ...stack.at(-1), ...attrsOf(rest) };
    for (const k of ["fill", "stroke"]) {
      const v = a[k];
      if (v === undefined || v === "none") continue;
      if (v.startsWith("url(")) a[`${k}-opacity`] = "0.5";
      fixed ??= colorOf(v);
      a[k] = "currentColor";
    }
    parts.push(
      `<${tag}${KEEP.filter((k) => a[k] !== undefined)
        .map((k) => ` ${k}="${a[k]}"`)
        .join("")}/>`,
    );
  }
  return { body: parts.join(""), fixed };
}

const icons = [];
const index = new Map();
const iconFor = (name) => {
  if (index.has(name)) return index.get(name);
  const sym = symbols.get(name);
  if (!sym) throw new Error(`no symbol ${name}`);
  const { body, fixed } = convert(sym.inner);
  index.set(name, icons.length);
  icons.push({ name, view: sym.view, body, fixed });
  return icons.length - 1;
};
const tokenIcon = (token) => iconFor(`file-tree-builtin-${token}`);
const lower = (o) => Object.entries(o).map(([k, v]) => [k.toLowerCase(), v]);
const t3Names = lower(web.T3_PIERRE_ICONS.byFileName).map(([k, v]) => [k, iconFor(v)]);
const t3Extensions = lower(web.T3_PIERRE_ICONS.byFileExtension).map(([k, v]) => [k, iconFor(v)]);
const tokens = [
  ...new Set([
    ...Object.values(NAME_TOKENS),
    ...Object.values(EXTENSION_TOKENS),
    ...Object.values(OVERRIDES),
    "default",
    "image",
  ]),
].sort();
const tokenIcons = tokens.map((t) => [t, tokenIcon(t)]);

const esc = (s) => s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
const sorted = (rows) => [...rows].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
const strTable = (rows) =>
  sorted(rows)
    .map(([k, v]) => `    ("${esc(k)}", "${esc(v)}"),`)
    .join("\n");
const idTable = (rows) =>
  sorted(rows)
    .map(([k, v]) => `    ("${esc(k)}", ${v}),`)
    .join("\n");
const color = (c) => (c ? `Some(("${c[0]}", "${c[1]}"))` : "None");

const samples = new Set([
  "unknown",
  "LICENSE",
  "src/main.rs",
  "a/b/App.tsx",
  "foo.d.ts",
  "photo.HEIC",
  "folder/REPORT.PDF",
  "Dockerfile",
  "x.test.tsx",
  "archive.tar.gz",
]);
for (const k of [...Object.keys(NAME_TOKENS), ...Object.keys(web.T3_PIERRE_ICONS.byFileName)])
  samples.add(`dir/${k}`);
for (const k of [
  ...Object.keys(EXTENSION_TOKENS),
  ...Object.keys(OVERRIDES),
  ...Object.keys(web.T3_PIERRE_ICONS.byFileExtension),
])
  samples.add(`dir/file.${k}`);
const cases = [...samples].sort().map((p) => {
  const r = web.resolvePierreIconForEntry(p, "file");
  iconFor(r.name);
  return `    ("${esc(p)}", "${r.name}", ${r.token ? `Some("${r.token}")` : "None"}),`;
});

let out = `//! Generated by tools/gen-file-icons.mjs from apps/web/src/pierre-icons.ts and @pierre/trees
//! ${JSON.parse(readFileSync(join(treesDir, "package.json"), "utf8")).version} (Apache-2.0); T3 symbols from Lucide (ISC). Do not edit.
#![allow(clippy::all)]

use rune::Icon;

pub(super) static ICONS: [Icon; ${icons.length}] = [
${icons.map((i) => `    Icon { name: "${i.name}", view: ${i.view}.0, body: "${esc(i.body)}" },`).join("\n")}
];

/// Each icon's own (light, dark) colour, when it paints one instead of currentColor.
pub(super) static FIXED: [Option<(&str, &str)>; ${icons.length}] = [
${icons.map((i) => `    ${color(i.fixed)},`).join("\n")}
];

/// \`PierreEntryIcon\`'s \`ICON_COLORS\`: token -> (light, dark).
pub(super) static TOKEN_COLORS: &[(&str, (&str, &str))] = &[
${sorted(Object.entries(COLORS))
  .map(([k, [l, d]]) => `    ("${k}", ("${l}", "${d}")),`)
  .join("\n")}
];

/// \`T3_PIERRE_ICONS.byFileName\` and \`byFileExtension\`, lower-cased.
pub(super) static T3_NAMES: &[(&str, usize)] = &[
${idTable(t3Names)}
];
pub(super) static T3_EXTENSIONS: &[(&str, usize)] = &[
${idTable(t3Extensions)}
];

/// \`@pierre/trees\`' \`BUILT_IN_FILE_NAME_TOKENS\`, \`COMPLETE_EXTENSION_OVERRIDES\` and
/// \`BUILT_IN_FILE_EXTENSION_TOKENS\`, keys as shipped.
pub(super) static NAME_TOKENS: &[(&str, &str)] = &[
${strTable(Object.entries(NAME_TOKENS))}
];
pub(super) static EXTENSION_OVERRIDES: &[(&str, &str)] = &[
${strTable(Object.entries(OVERRIDES))}
];
pub(super) static EXTENSION_TOKENS: &[(&str, &str)] = &[
${strTable(Object.entries(EXTENSION_TOKENS))}
];
pub(super) static TOKEN_ICONS: &[(&str, usize)] = &[
${idTable(tokenIcons)}
];

/// \`resolvePierreIconForEntry(path, "file")\` as web answers it: (path, icon, token).
#[cfg(test)]
pub(super) static WEB_CASES: &[(&str, &str, Option<&str>)] = &[
${cases.join("\n")}
];
`;
process.stdout.write(out);
console.error(`${icons.length} icons, ${cases.length} web cases`);
