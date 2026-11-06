// Resolve every --color-* token of the live web app to sRGBA bytes, in dark and light.
// usage: node tokens.mjs <url>  -> ~/shots/tokens.json
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";
import { homedir } from "node:os";
const [url] = process.argv.slice(2);
const home = homedir();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const chrome = spawn(`${home}/chrome-headless-shell-linux64/chrome-headless-shell`, ["--headless", "--no-sandbox", "--remote-debugging-port=9334", `--user-data-dir=${home}/.s5ref-profile`, "--window-size=1440,900", "about:blank"], { stdio: "ignore" });
let list;
for (let i = 0; i < 50; i++) { try { list = await (await fetch("http://127.0.0.1:9334/json/list")).json(); break; } catch { await sleep(100); } }
const ws = new WebSocket(list.find((t) => t.type === "page").webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0; const pending = new Map();
ws.addEventListener("message", (e) => { const m = JSON.parse(e.data); if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); } });
const call = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async (expression) => (await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;
await call("Page.enable");
await call("Emulation.setDeviceMetricsOverride", { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
const out = {};
for (const scheme of ["dark", "light"]) {
  await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }] });
  await call("Page.navigate", { url });
  await sleep(5000);
  for (const scope of ["root", "sidebar"]) out[scope === "root" ? scheme : "sidebar-" + scheme] = await evaluate(`(() => {
    const scope = ${JSON.stringify(scope)};
    const names = new Set();
    const walk = (rules) => { for (const r of rules) { if (r.style) for (const p of r.style) if (p.startsWith("--")) names.add(p); if (r.cssRules) walk(r.cssRules); } };
    for (const s of document.styleSheets) { try { walk(s.cssRules); } catch {} }
    const cs = getComputedStyle(document.documentElement);
    const host = scope === "sidebar" ? document.querySelector("[data-app-sidebar]") : document.body;
    if (!host) return null;
    const hcs = getComputedStyle(host);
    const probe = document.createElement("div"); host.appendChild(probe);
    const cv = document.createElement("canvas"); cv.width = cv.height = 1; const g = cv.getContext("2d", { willReadFrequently: true });
    const res = { colors: {}, raw: {}, html: document.documentElement.className };
    for (const n of [...names].sort()) {
      const raw = (scope === "sidebar" ? hcs : cs).getPropertyValue(n).trim();
      if (!raw) continue;
      res.raw[n] = raw;
      probe.style.color = ""; probe.style.color = "var(" + n + ")";
      if (!probe.style.color) continue;
      const c = getComputedStyle(probe).color;
      if (!c || (raw.match(/^[\\d.]+(px|rem|ms|s|%)?$/))) continue;
      g.clearRect(0, 0, 1, 1); g.fillStyle = "#010203"; g.fillStyle = c;
      if (g.fillStyle === "#010203" && c !== "rgb(1, 2, 3)") continue;
      g.fillRect(0, 0, 1, 1);
      const d = g.getImageData(0, 0, 1, 1).data;
      // getImageData premultiplies then unpremultiplies; fine for a=255, close otherwise.
      const a = (c.match(/\\/\\s*([\\d.]+%?)\\)$/) || c.match(/rgba\\([^)]*,\\s*([\\d.]+)\\)$/) || [, "1"])[1];
      res.colors[n] = { css: c, rgba: [d[0], d[1], d[2], a.endsWith("%") ? parseFloat(a) / 100 : parseFloat(a)] };
    }
    probe.remove();
    return res;
  })()`);
}
writeFileSync(`${home}/shots/tokens.json`, JSON.stringify(out, null, 1));
console.log(Object.keys(out["sidebar-dark"]?.colors ?? {}).length, Object.keys(out.dark.colors).length, Object.keys(out.light.colors).length, out.dark.html, "|", out.light.html);
ws.close(); chrome.kill(); process.exit(0);
