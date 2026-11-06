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
  if (kind === 'menu') {
    const M = await import('/src/components/ui/menu.tsx');
    const I = await import('/node_modules/.vite/deps/lucide-react.js');
    const host = document.createElement('div');
    host.style.cssText='position:fixed;inset:0;z-index:100;background:var(--background);color:var(--contrast-foreground);padding:32px;display:flex;flex-direction:column;align-items:flex-start;gap:12px';
    document.body.append(host);
    createRoot(host).render(h(React.Fragment,null,
      h('div',{style:{fontSize:20,lineHeight:'28px'}},'UI kit · menu'),
      h(M.Menu,{open:true},h(M.MenuTrigger,{render:h(Button,{variant:'outline'})},'Open menu'),
        h(M.MenuPopup,{align:'start',style:{width:240}},
          h(M.MenuGroup,null,h(M.MenuGroupLabel,null,'Thread')),
          h(M.MenuItem,null,h(I.PlusIcon),'New thread',h(M.MenuShortcut,null,'⌘N')),
          ...['Highlighted','Pressed','Focused'].map((label,i)=>h(M.MenuItem,{'data-gallery-state':['hover','pressed','focus'][i],'data-highlighted':''},label)),
          h(M.MenuSeparator),
          h(M.MenuCheckboxItem,{checked:true},'Show archived'),
          h(M.MenuCheckboxItem,{checked:false},'Notifications'),
          h(M.MenuRadioGroup,{value:'system'},h(M.MenuRadioItem,{value:'system'},h('span',{style:{display:'flex',alignItems:'center',width:'100%'}},h('span',{style:{flex:1}},'System theme'),h(M.MenuRadioItemIndicator)))),
          h(M.MenuSub,{open:true},h(M.MenuSubTrigger,null,'More'),h(M.MenuSubPopup,null,h(M.MenuItem,null,'Move to project'))),
          h(M.MenuItem,{disabled:true},'Disabled'),
          h(M.MenuItem,{variant:'destructive'},h(I.Trash2Icon),'Delete thread')
        ))));
    return;
  }
  if (kind === 'controls' || kind === 'radio') {
    const {Checkbox} = await import('/src/components/ui/checkbox.tsx');
    const {Switch} = await import('/src/components/ui/switch.tsx');
    const {RadioGroup,Radio} = await import('/src/components/ui/radio-group.tsx');
    const host=document.createElement('div');
    host.style.cssText='position:fixed;inset:0;z-index:100;background:var(--background);color:var(--contrast-foreground);padding:32px;display:flex;flex-direction:column;gap:12px';
    document.body.append(host);
    const control=(Component,label,checked,state)=>h('label',{style:{display:'flex',gap:8,alignItems:'center',fontSize:14,lineHeight:'20px'}},
      h(Component,{checked,disabled:state==='disabled','data-gallery-state':state}),label);
    const rows=['default','hover','pressed','focus','disabled'].map(state=>h('div',{style:{display:'flex',gap:24},key:state},
      ...(kind==='controls' ? [
        control(Checkbox,'Checkbox',false,state),control(Checkbox,'Checked',true,state),
        control(Switch,'Off',false,state),control(Switch,'On',true,state)
      ] : [h(RadioGroup,{value:'default',style:{display:'flex',flexDirection:'row',gap:24}},
        h('label',{style:{display:'flex',gap:8,alignItems:'center',fontSize:14,lineHeight:'20px'}},h(Radio,{value:'default',disabled:state==='disabled','data-gallery-state':state}),'Default'),
        h('label',{style:{display:'flex',gap:8,alignItems:'center',fontSize:14,lineHeight:'20px'}},h(Radio,{value:'other',disabled:state==='disabled','data-gallery-state':state}),'Alternative'))])));
    createRoot(host).render(h(React.Fragment,null,h('div',{style:{fontSize:20,lineHeight:'28px'}},'UI kit · '+kind),...rows));
    return;
  }
  if (['popover','tooltip','select','combobox','toggles','feedback'].includes(kind)) {
    const host=document.createElement('div');
    host.style.cssText='position:fixed;inset:0;z-index:100;background:var(--background);color:var(--contrast-foreground);padding:32px;display:flex;align-items:flex-start;flex-direction:column;gap:12px';
    document.body.append(host);
    const title=h('div',{style:{fontSize:20,lineHeight:'28px'}},'UI kit · '+kind);
    let content=[];
    if (kind==='popover') {
      const P=await import('/src/components/ui/popover.tsx');
      content=[h(P.Popover,{open:true},h(P.PopoverTrigger,{render:h(Button,{variant:'outline'})},'More information'),
        h(P.PopoverPopup,null,h(P.PopoverTitle,null,'Preferences'),h('p',{style:{fontSize:14,lineHeight:'20px',marginTop:8,color:'var(--contrast-muted-foreground)'}},'Configure this environment.')))];
    } else if(kind==='tooltip') {
      const P=await import('/src/components/ui/tooltip.tsx');
      content=[h(P.Tooltip,{open:true},h(P.TooltipTrigger,{render:h(Button,{variant:'outline'})},'More information'),
        h(P.TooltipPopup,null,'More information'))];
    } else if(kind==='select') {
      const P=await import('/src/components/ui/select.tsx');
      content=[h(P.Select,{value:'System',open:true},h(P.SelectTrigger,{style:{width:180}},h(P.SelectValue)),
        h(P.SelectPopup,{alignItemWithTrigger:false},...['System','Light','Dark'].map(v=>h(P.SelectItem,{key:v,value:v},v))))];
    } else if(kind==='combobox') {
      const P=await import('/src/components/ui/combobox.tsx');
      const S=await import('/src/components/ui/select.tsx');
      const I=await import('/node_modules/.vite/deps/lucide-react.js');
      content=[h(P.Combobox,{value:'System',open:true,items:['System','Light','Dark']},
        h(P.ComboboxTrigger,{render:h(S.SelectButton,{style:{width:280}})},'System'),
        h(P.ComboboxPopup,{style:{width:280}},h(P.ComboboxSearchInput,{placeholder:'Search'}),h(P.ComboboxList,null,...['System','Light','Dark'].map(v=>h(P.ComboboxItem,{key:v,value:v},v,h(I.CheckIcon,{style:{width:14,height:14,marginLeft:'auto',opacity:v==='System'?1:0}}))))))];
    } else if(kind==='toggles') {
      const P=await import('/src/components/ui/toggle-group.tsx');
      const header=h('div',{style:{whiteSpace:'pre',fontSize:12,lineHeight:'16px',color:'var(--contrast-muted-foreground)'}},'Default                 Hover                 Pressed                 Focus                 Disabled');
      content=[header,...['default','outline','segmented','ghost','pill'].map(variant=>h('div',{style:{display:'flex',gap:12,alignItems:'center'},key:variant},
        h('span',{style:{width:140,fontSize:12,lineHeight:'16px',color:'var(--contrast-muted-foreground)'}},variant),
        ...['default','hover','pressed','focus','disabled'].map(state=>h(P.ToggleGroup,{key:state,value:['a'],variant,size:'segmented'},
          h(P.Toggle,{value:'a'},'A'),h(P.Toggle,{value:'b',disabled:state==='disabled','data-gallery-state':state},'B')))))];
    } else {
      const {Spinner}=await import('/src/components/ui/spinner.tsx');
      const {Kbd}=await import('/src/components/ui/kbd.tsx');
      const {Separator}=await import('/src/components/ui/separator.tsx');
      const {Skeleton}=await import('/src/components/ui/skeleton.tsx');
      content=[...['xs','sm','md','lg'].map(size=>h('div',{key:size,style:{display:'flex',gap:12}},h(Spinner,{size}),h(Spinner,{size,tone:'muted'}))),h(Kbd,null,'⌘K'),h(Separator),h(Skeleton,{style:{width:240,height:16}})];
    }
    createRoot(host).render(h(React.Fragment,null,title,...content));
    return;
  }
  const names = kind === 'sizes'
    ? ['compact','default','icon','icon-lg','icon-micro','icon-tiny','icon-sm','icon-xl','icon-xs','lg','micro','multiline','sm','sm-multiline','xl','xs']
    : kind === 'badges'
    ? ['default','secondary','outline','destructive','error','info','success','warning','label']
    : ['default','destructive','destructive-outline','ghost','ghost-muted','ghost-destructive','glass','link','media-close','media-navigation','outline','overlay','secondary','warning-outline'];
  const host = document.createElement('div');
  host.style.cssText = 'position:fixed;inset:0;z-index:100;background:var(--background);color:var(--contrast-foreground);padding:32px;display:flex;flex-direction:column;gap:12px;overflow:auto';
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
