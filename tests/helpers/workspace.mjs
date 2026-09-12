// Controlled lifecycle/IPC boundary for actual TSX component callbacks. Native
// rendering and scrolling are also checked separately in the Tauri QA app.
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import assert from 'node:assert/strict';
import ts from 'typescript';
import { randomUUID } from 'node:crypto';
import { createInstance } from 'i18next';
import { renderMessage } from '../../src/i18n/messages.ts';

export function workspaceFixture(t, {native = false, modules = {}, timers = {setTimeout, clearTimeout}} = {}) {
const translation = createInstance();
const resources = {};
for (const language of ['en', 'zh-CN']) {
  resources[language] = {};
  for (const name of fs.readdirSync(`locales/${language}`).filter(name => name.endsWith('.json'))) {
    resources[language][name.slice(0, -5)] = JSON.parse(fs.readFileSync(`locales/${language}/${name}`, 'utf8'));
  }
}
translation.init({ resources, lng: 'zh-CN', fallbackLng: 'en', defaultNS: 'common', initImmediate: false, interpolation: { escapeValue: false } });
let active;
const fibers = [];
const hooks = {
  useState(initial) {
    const fiber = active, i = fiber.cursor++;
    if (!(i in fiber.slots)) fiber.slots[i] = typeof initial === 'function' ? initial() : initial;
    return [fiber.slots[i], value => {
      fiber.slots[i] = typeof value === 'function' ? value(fiber.slots[i]) : value;
      fiber.dirty = true;
    }];
  },
  useRef(initial) {
    const f = active, i = f.cursor++;
    return f.slots[i] ||= {current: initial};
  },
  useEffect(effect, deps) {
    const f = active, i = f.cursor++, old = f.slots[i];
    if (!old || !deps || deps.some((d,n) => !Object.is(d, old.deps[n]))) {
      f.effects.push(() => {old?.cleanup?.(); f.slots[i] = {deps, cleanup: effect()};});
    }
  },
  useCallback(fn, deps) { const ref = hooks.useRef({fn,deps}); if (deps.some((d,i)=>!Object.is(d,ref.current.deps[i]))) ref.current={fn,deps}; return ref.current.fn; }
};
hooks.useLayoutEffect = hooks.useEffect;
hooks.useMemo = (fn,deps) => hooks.useCallback(fn,deps)();
hooks.useId = () => hooks.useRef(randomUUID()).current;
const jsx = (type, props, key) => ({type,props:props||{},key});
const db = new Map(), calls = [], overrides = {};
const windowEvents = new Map(), nativeEvents = new Map();
let askResolve, messageResponse, scrolls=0;
const topic = {id:'topic-a',title:'测试讨论',updated_at:1};
const keyA={kind:'memory',id:'a'}, keyB={kind:'memory',id:'b'};
const row = key => ({key,title:key.id,snippet:'test',updated_at:1,origin:null});
const api = {
  native, unavailable:error=>error?.code==='unavailable', uid:randomUUID, keyOf:k=>`${k.kind}:${k.id}`, errorText:error=>error?.code?{ns:'errors',key:error.code}:String(error),
  date:()=>'',fullDate:()=>'',sourceName:()=>'',
  async call(name,args) {
    calls.push({name,args});
    if (overrides[name]) return overrides[name](args);
    if(name==='voice_status') return {enabled:false,preload:false,shortcut:'',state:'unloaded',backend:null,error:null,available:false,downloaded:0,bytes:1019141728,cache:'test',session:null};
    if(name==='voice_applied') return;
    if(name==='voice_take_shortcut') return false;
    if(name==='draft_read') return structuredClone(db.get(args.key)||null);
    if(name==='draft_write') { if(Buffer.byteLength(args.draft.title)>200) throw 'invalid title'; db.set(args.draft.key,structuredClone(args.draft)); return; }
    if(name==='draft_clear') { db.delete(args.key); return; }
    if(name==='discussion_source') return {source:args.source,text:'known source'};
    if(name==='discussion_messages') return messageResponse ? messageResponse(args) : [];
    if(name==='discussion_ask') return new Promise(resolve=>{askResolve=()=>resolve(topic)});
    if(name==='library_query') return {items:[row(keyA),row(keyB)],next_offset:null};
    if(name==='navigation_collections') return [];
    if(name==='navigation_record') return {pinned:false,collections:[]};
    if(name==='library_topics' || name==='library_projects') return [];
    if(name==='workspace_settings') return {configured:true};
    if(name==='library_detail') return {key:args.key,state:'active',title:args.key.id,body:'test',current:{id:'v-'+args.key.id,capture_ids:[],created_at:1,actor:'user'},history:[],sources:[]};
    if(name==='library_action') return {action:'undo'};
    throw Error(name);
  }
};
const cache = new Map();
function load(file) {
  file=path.resolve(file); if(cache.has(file))return cache.get(file);
  const module={exports:{}};cache.set(file,module.exports);
  const code=ts.transpileModule(fs.readFileSync(file,'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.ReactJSX,target:ts.ScriptTarget.ES2022}}).outputText;
  const req = name => {
    if (Object.hasOwn(modules, name)) return modules[name];
    if(name==='./resources')return {useResourceVersion:()=>0,useResourceBridge:()=>{},expireQueries:async()=>{}};
    if(name==='./Select')return {default:'select'}; // Shared control is exercised separately with real React.
    if(name==='react')return hooks;
    if(name==='react-i18next')return {useTranslation:ns=>({t:translation.getFixedT(null,ns),i18n:translation})};
    if(name==='../i18n')return {default:translation,translateCatalog:(key,options={})=>translation.exists(key,options)?translation.t(key,options):translation.t('operation_failed',{ns:'errors'})};
    if(name==='../i18n/preferences')return {getLanguageSnapshot:()=>({preference:'zh-CN',language:'zh-CN',revision:0}),subscribeLanguage:()=>()=>{},refreshLanguage:async()=>{},setLanguagePreference:async()=>{}};
    if(name==='../i18n/format')return {formatNumber:(value,maximumFractionDigits=0)=>new Intl.NumberFormat('zh-CN',{maximumFractionDigits}).format(value)};
    if(name==='../i18n/react')return {useNotice:(initial='')=>{
      const [value,setValue]=hooks.useState(initial);
      return [renderMessage(value,(key,options)=>translation.t(key,options)),setValue,value];
    }};
    if(name==='react-dom')return {createPortal:children=>children};
    if(name==='react/jsx-runtime')return {jsx,jsxs:jsx,Fragment:'fragment'};
    if(name==='./api')return api;
    if(name==='./MarkdownEditor')return {default:'MarkdownEditor'};
    if(name==='./Markdown')return {default:'Markdown'};
    if(name==='../ui')return {Icon:'Icon'};
    if(name.startsWith('@tauri'))return {listen:(event,fn)=>{if(!nativeEvents.has(event))nativeEvents.set(event,new Set());nativeEvents.get(event).add(fn);return Promise.resolve(()=>nativeEvents.get(event)?.delete(fn));}};
    if(name.endsWith('.css')||name.endsWith('.svg'))return {};
    let p=path.resolve(path.dirname(file),name);
    for(const ext of ['','.tsx','.ts'])if(fs.existsSync(p+ext))return load(p+ext);
    throw Error(name);
  };
  vm.runInNewContext(code,{require:req,module,exports:module.exports,console,...timers,performance,crypto:{randomUUID},window:{
    addEventListener(name,fn){if(!windowEvents.has(name))windowEvents.set(name,new Set());windowEvents.get(name).add(fn);},
    removeEventListener(name,fn){windowEvents.get(name)?.delete(fn);}
  },document:{querySelector(){return null}},requestAnimationFrame:fn=>fn()},{filename:file});
  return module.exports;
}
function mount(component,props={}) {
  const f={component,props,slots:[],cursor:0,effects:[],dirty:true,alive:true,dom:{scrolls:0,messages:new Map()}}; fibers.push(f);render(f);return f;
}
function render(f) {
  f.cursor=0;f.dirty=false;active=f;f.tree=f.component(f.props);active=null;
  const list=f.dom.list ||= {scrollTop:0,getBoundingClientRect:()=>({top:0,bottom:500}),querySelectorAll:()=>f.dom.ordered||[]};
  const articles=nodes(f.tree).filter(n=>n.type==='article'&&n.props.className?.includes('discussion-message'));
  f.dom.ordered=articles.map((n,i)=>{
    if(!f.dom.messages.has(n.key)) f.dom.messages.set(n.key,{key:n.key,getBoundingClientRect(){return {top:this.offset-list.scrollTop,bottom:this.offset+100-list.scrollTop};}});
    const element=f.dom.messages.get(n.key);element.offset=i*100;return element;
  });
  for(const node of nodes(f.tree)) if(node.props?.ref) node.props.ref.current ||= node.props.className==='discussion-messages'?list:{focus(){},scrollIntoView(){f.dom.scrolls++;list.scrollTop=Math.max(0,articles.length*100-500);}};
  for(const effect of f.effects.splice(0))effect();
}
function unmount(f) { f.alive=false; for(const value of f.slots)value?.cleanup?.(); for(const n of nodes(f.tree))if(n.props?.ref)n.props.ref.current=null; }
async function settle() {
  for(let i=0;i<8;i++){await new Promise(resolve=>setTimeout(resolve,1));for(const f of fibers)if(f.alive&&f.dirty)render(f);}
}
function nodes(node) {
  if(!node)return [];
  if(Array.isArray(node))return node.flatMap(nodes);
  if(typeof node!=='object')return [];
  return [node,...nodes(node.props?.children)];
}
function text(node) { if(Array.isArray(node))return node.map(text).join(''); if(typeof node==='string')return node; return node?.props?text(node.props.children):''; }
const querySurfaces = new Map();
function query(app) {
  const component=load('src/workspace/WorkspaceQuery.tsx').default;
  const node=nodes(app.tree).find(n=>n.type===component);
  assert(node,'missing query surface');
  let surface=querySurfaces.get(app);
  if(!surface){surface=mount(component,node.props);querySurfaces.set(app,surface);}
  else if(surface.props!==node.props){surface.props=node.props;render(surface);}
  return surface;
}
function find(f,predicate){const node=nodes(f.tree).find(predicate);assert(node,'missing element');return node;}
  t.after(()=>{for(const f of fibers)if(f.alive)unmount(f);});
  return {load,mount,unmount,settle,find,query,nodes,text,db,calls,overrides,topic,keyA,keyB,
    async language(code){await translation.changeLanguage(code);for(const f of fibers)if(f.alive)f.dirty=true;await settle();},
    focus(){windowEvents.get('focus')?.forEach(fn=>fn());},
    key(event){windowEvents.get('keydown')?.forEach(fn=>fn(event));},
    emit(name,payload){nativeEvents.get(name)?.forEach(fn=>fn({payload}));},
    completeAsk(){assert(askResolve);askResolve();},
    messages(fn){messageResponse=fn;},
    render(f,props){f.props=props;render(f);}
  };
}
