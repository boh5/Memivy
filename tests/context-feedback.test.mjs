import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
function clock() {
 let now=0,id=0;const pending=new Map();
 return {timers:{setTimeout(fn,ms){const key=++id;pending.set(key,{fn,at:now+ms});return key;},clearTimeout(key){pending.delete(key);}},
 tick(ms){now+=ms;for(const [key,v] of pending)if(v.at<=now){pending.delete(key);v.fn();}}};
}
test('success toast expires, pauses during interaction, and only the latest message remains',async t=>{
 const time=clock(),f=workspaceFixture(t,{timers:time.timers}),{default:Toast,notify}=f.load('src/workspace/Toast.tsx');const view=f.mount(Toast);await f.settle();
 notify('旧消息',undefined,undefined,25);await f.settle();notify('已记下',undefined,undefined,25);await f.settle();
 assert(!f.text(view.tree).includes('旧消息'));
 f.find(view,n=>n.props.className==='workspace-toast').props.onMouseEnter();time.tick(40);await f.settle();assert(f.text(view.tree).includes('已记下'));
 f.find(view,n=>n.props.className==='workspace-toast').props.onMouseLeave();time.tick(40);await f.settle();assert(!f.text(view.tree).includes('已记下'));f.unmount(view);
});
test('success receipts are absent from the reading status but remain actionable in history',async t=>{
 const f=workspaceFixture(t),Receipt=f.load('src/workspace/OrganizationReceipt.tsx').default;
 f.overrides.organization_jobs=async()=>[{capture_id:'raw',attempt_id:'attempt',status:'done',reason:'',receipt:{request_id:'r',memory_id:'a',status:'applied',after_version:'v'}}];
 const props={record:f.keyA,revision:0,onOpen(){},onRefresh(){}};
 const reading=f.mount(Receipt,{...props,presentation:'status'}),history=f.mount(Receipt,props);await f.settle();
 assert(!f.text(reading.tree).includes('已建立记忆'));assert(!f.text(reading.tree).includes('撤销整理'));assert(f.text(history.tree).includes('撤销整理'));
});
test('dismissal failures retain suggestions so an unsaved preference is never claimed',async t=>{
 const f=workspaceFixture(t,{native:true}),Component=f.load('src/workspace/OrganizationCollections.tsx').default;
 f.overrides.organization_collections=async()=>[{collection:{id:'c',name:'产品',revision:1},reason:'相关'}];
 f.overrides.organization_dismiss=async()=>{throw '未能保存忽略状态';};
 const view=f.mount(Component,{receipt:'r',record:f.keyA,revision:0,disabled:false,onRefresh(){}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='忽略本次建议').props.onClick();await f.settle();assert(f.text(view.tree).includes('未能保存忽略状态'));assert(f.text(view.tree).includes('＋ 产品'));
});
test('library status uses a bounded local batch without launching recommendation models',async t=>{
 const f=workspaceFixture(t,{native:true}),List=f.load('src/workspace/MemoryList.tsx').default;
 f.overrides.organization_states=async()=>[{key:f.keyA,status:'done',recommendations:2},{key:f.keyB,status:'processing',recommendations:0}];
 const view=f.mount(List,{trash:false,active:true,selected:null,revision:0,onSelect(){},onCapture(){},onRefresh(){}});await f.settle();
 assert(f.text(view.tree).includes('2 个专题推荐'));assert(f.text(view.tree).includes('整理中…'));assert.equal(f.calls.filter(c=>c.name==='organization_collections').length,0);
});

test('keyboard focus keeps a toast alive after the pointer leaves and focus moves inside it',async t=>{
 const time=clock(),f=workspaceFixture(t,{timers:time.timers}),{default:Toast,notify}=f.load('src/workspace/Toast.tsx');const view=f.mount(Toast);await f.settle();
 notify('可撤销的操作','撤销',()=>{},25);await f.settle();
 const box=()=>f.find(view,n=>n.props.className==='workspace-toast');
 box().props.onMouseEnter();box().props.onFocus();box().props.onMouseLeave();
 box().props.onBlur({currentTarget:{contains:()=>true},relatedTarget:{}});
 time.tick(45);await f.settle();assert(f.text(view.tree).includes('可撤销的操作'));
 box().props.onBlur({currentTarget:{contains:()=>false},relatedTarget:null});
 time.tick(45);await f.settle();assert(!f.text(view.tree).includes('可撤销的操作'));
});

test('a toast action can publish its result without the old toast clearing it',async t=>{
 const time=clock(),f=workspaceFixture(t,{timers:time.timers}),{default:Toast,notify}=f.load('src/workspace/Toast.tsx');const view=f.mount(Toast);await f.settle();
 notify('已加入','撤销',()=>notify('已撤销'));await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='撤销').props.onClick();await f.settle();
 assert(f.text(view.tree).includes('已撤销'));
});
