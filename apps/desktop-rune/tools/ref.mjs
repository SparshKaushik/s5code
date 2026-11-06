// Capture the real S5 Code web UI as a parity reference.
// usage: node ref.mjs <url> <outName> [--dark|--light] [--wait ms] [--eval js] [--click x,y]...
// Writes ~/shots/<outName>.ref.png and ~/shots/<outName>.ref.json (visible element geometry + styles).
import { spawn } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";

const args = process.argv.slice(2);
const [url, name] = args;
const opt = (k, d) => {
  const i = args.indexOf(k);
  return i >= 0 ? args[i + 1] : d;
};
const all = (k) => args.flatMap((a, i) => (a === k ? [args[i + 1]] : []));
const scheme = args.includes("--light") ? "light" : "dark";
const wait = Number(opt("--wait", "2500"));
const W = 1440, H = 900;
const home = homedir();
mkdirSync(`${home}/shots`, { recursive: true });

const chrome = spawn(`${home}/chrome-headless-shell-linux64/chrome-headless-shell`, [
  "--headless", "--no-sandbox", "--hide-scrollbars", "--remote-debugging-port=9333",
  `--user-data-dir=${home}/.s5ref-profile`, `--window-size=${W},${H}`, "about:blank",
], { stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let list;
for (let i = 0; i < 50; i++) {
  try { list = await (await fetch("http://127.0.0.1:9333/json/list")).json(); break; } catch { await sleep(100); }
}
const page = list.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const pending = new Map();
ws.addEventListener("message", (e) => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  if (m.method === "Runtime.exceptionThrown") logs.push("EXC " + (m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text).slice(0, 300));
  if (m.method === "Runtime.consoleAPICalled" && ["error", "warning"].includes(m.params.type)) logs.push(m.params.type + " " + m.params.args.map((a) => a.value ?? a.description).join(" ").slice(0, 300));
  if (m.method === "Network.loadingFailed") logs.push("NETFAIL " + m.params.errorText);
});
const logs = [];
const call = (method, params = {}) => new Promise((r) => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async (expression) => (await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true })).result?.result?.value;

await call("Page.enable");
await call("Runtime.enable");
await call("Network.enable");
await call("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
await call("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: scheme }, { name: "prefers-reduced-motion", value: "reduce" }] });
await call("Page.navigate", { url });
await sleep(wait);
for (const js of all("--eval")) { await evaluate(js); await sleep(600); }
for (const xy of all("--click")) {
  const [x, y] = xy.split(",").map(Number);
  for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) await call("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount: 1 });
  await sleep(700);
}
await sleep(400);
const shot = await call("Page.captureScreenshot", { format: "png" });
writeFileSync(`${home}/shots/${name}.ref.png`, Buffer.from(shot.result.data, "base64"));
const dump = await evaluate(`(() => {
  const out = [];
  const px = (v) => Math.round(parseFloat(v) * 100) / 100;
  for (const el of document.querySelectorAll("body *")) {
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1 || r.bottom < 0 || r.top > ${H} || r.right < 0 || r.left > ${W}) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === "hidden" || cs.display === "none" || +cs.opacity === 0) continue;
    const own = [...el.childNodes].filter((n) => n.nodeType === 3).map((n) => n.textContent.trim()).join(" ").trim();
    const painted = cs.backgroundColor !== "rgba(0, 0, 0, 0)" || cs.borderTopWidth !== "0px" || cs.boxShadow !== "none";
    if (!own && !painted && el.tagName !== "svg" && el.tagName !== "INPUT" && el.tagName !== "TEXTAREA") continue;
    out.push({
      tag: el.tagName.toLowerCase(), cls: (el.getAttribute("class") || "").slice(0, 160), role: el.getAttribute("role") || undefined,
      aria: el.getAttribute("aria-label") || undefined, text: own.slice(0, 120) || undefined,
      x: px(r.left), y: px(r.top), w: px(r.width), h: px(r.height),
      color: cs.color, bg: cs.backgroundColor, font: cs.fontFamily.split(",")[0], size: cs.fontSize, weight: cs.fontWeight,
      lh: cs.lineHeight, ls: cs.letterSpacing, radius: cs.borderRadius, border: cs.borderTopWidth !== "0px" ? cs.borderTopWidth + " " + cs.borderTopColor : undefined,
      shadow: cs.boxShadow !== "none" ? cs.boxShadow : undefined, pad: cs.padding, opacity: cs.opacity !== "1" ? cs.opacity : undefined,
    });
  }
  return { url: location.href, vars: Object.fromEntries([...getComputedStyle(document.documentElement)].filter((k) => k.startsWith("--")).map((k) => [k, getComputedStyle(document.documentElement).getPropertyValue(k).trim()])), els: out };
})()`);
writeFileSync(`${home}/shots/${name}.ref.json`, JSON.stringify(dump, null, 1));
console.log(`${home}/shots/${name}.ref.png ${dump?.els?.length} elements at ${dump?.url}`);
if (args.includes("--logs")) console.log(logs.slice(0, 40).join("\n"));
ws.close();
chrome.kill();
process.exit(0);
