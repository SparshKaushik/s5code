// Pass this file's contents to desk.mjs --eval on the Vite reference app.
// Uses the app's actual primitives, not copied markup. Switch gallery with ?kit=sizes or badges.
(async () => {
  const reactModule = await import('/node_modules/.vite/deps/react.js');
  const React = reactModule.default || reactModule;
  const rootModule = await import('/node_modules/.vite/deps/react-dom_client.js');
  const { createRoot } = rootModule.default || rootModule;
  const { Button } = await import('/src/components/ui/button.tsx');
  const { Badge } = await import('/src/components/ui/badge.tsx');
  const { PlusIcon } = await import('/node_modules/.vite/deps/lucide-react.js');
  const h = React.createElement;
  const kind = window.__kitGallery || 'buttons';
  const names = kind === 'sizes'
    ? ['compact','default','icon','icon-lg','icon-micro','icon-tiny','icon-sm','icon-xl','icon-xs','lg','micro','multiline','sm','sm-multiline','xl','xs']
    : kind === 'badges'
    ? ['default','secondary','outline','destructive','error','info','success','warning','label']
    : ['default','destructive','destructive-outline','ghost','ghost-muted','ghost-destructive','glass','link','media-close','media-navigation','outline','overlay','secondary','warning-outline'];
  const host = document.createElement('div');
  host.style.cssText = 'position:fixed;inset:0;z-index:99999;background:var(--background);color:var(--contrast-foreground);padding:32px;display:flex;flex-direction:column;gap:12px;overflow:auto';
  document.body.append(host);
  const rows = names.map(name => h('div', { key:name, style:{display:'flex',height:kind==='badges'?24:40,flexShrink:0,gap:12,alignItems:'center'} },
    h('span',{style:{width:145,flexShrink:0,fontSize:12,lineHeight:'16px',color:'var(--contrast-muted-foreground)'}},name),
    ...(kind==='badges' ? ['default','sm','lg','control'].map(size => h(Badge,{key:size,size,variant:name},name)) : ['default','hover','pressed','focus','disabled'].map(state => h(Button,{
      key:state, variant:kind==='sizes'?'default':name,size:kind==='sizes'?name:'default',disabled:state==='disabled',
      ...(state==='pressed'?{'data-pressed':''}:{}), 'data-gallery-state':state,
    },h(PlusIcon),...(kind==='sizes'&&name.startsWith('icon')?[]:['Button']))))));
  createRoot(host).render(h(React.Fragment,null,
    h('div',{style:{fontSize:20,lineHeight:'28px'}},`UI kit · ${kind}`),
    ...(kind==='badges'?[]:[h('div',{style:{fontSize:12,lineHeight:'16px',color:'var(--contrast-muted-foreground)'}},'Default                 Hover                 Pressed                 Focus                 Disabled')]),...rows));
})().catch(error => { window.__kitGalleryError = String(error); throw error; });
