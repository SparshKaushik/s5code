// Runs the shared Electron harness and forces native CSS pseudo states on gallery cells.
// Usage: node reference-capture.mjs <route> <name> --eval <reference-gallery.js contents> ...
import { readFileSync, writeFileSync, unlinkSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { pathToFileURL } from 'node:url';
const source = readFileSync(`${homedir()}/desk.mjs`, 'utf8');
const hook = `
await call('DOM.enable');
await call('CSS.enable');
const galleryDocument = await call('DOM.getDocument');
const galleryRoot = galleryDocument.result?.root?.nodeId;
if (galleryRoot) {
  for (const [state,pseudo] of [['hover','hover'],['pressed','active'],['focus','focus-visible']]) {
    const nodes = await call('DOM.querySelectorAll', {nodeId:galleryRoot,selector:'[data-gallery-state="'+state+'"]'});
    for (const nodeId of nodes.result?.nodeIds || []) {
      await call('CSS.forcePseudoState', {nodeId,forcedPseudoClasses:state==='pressed'?['hover','active']:[pseudo]});
    }
  }
}
await sleep(250);
`;
const path = `${tmpdir()}/kit-reference-${process.pid}.mjs`;
writeFileSync(path, source.replace('const shot = await call', `${hook}\nconst shot = await call`));
try { await import(pathToFileURL(path)); } finally { unlinkSync(path); }
