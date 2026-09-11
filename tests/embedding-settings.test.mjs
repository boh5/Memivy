import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
const initial={enabled:false,preparing:false,paused:false,state:'not_downloaded',downloaded:0,bytes:639150592,processed:0,total:0,failed:0,error:null};
test('an existing shared model offers enable without another download',async t=>{
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{}}});f.overrides.embedding_status=async()=>({...initial,state:'disabled',downloaded:initial.bytes});
 const view=f.mount(f.load('src/workspace/EmbeddingSettings.tsx').default);await f.settle();
 assert(f.find(view,n=>n.type==='button'&&f.text(n)==='开启语义检索'));
 assert(!f.text(view.tree).includes('下载并启用'));
 f.unmount(view);
});
test('download is a host task and closing settings does not cancel it',async t=>{
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{}}});let state={...initial};f.overrides.embedding_status=async()=>state;f.overrides.embedding_control=async({action})=>{assert.equal(action,'enable');state={...state,preparing:true,state:'downloading'};};
 const view=f.mount(f.load('src/workspace/EmbeddingSettings.tsx').default);await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='下载并启用').props.onClick();await f.settle();assert(f.text(view.tree).includes('正在下载模型'));f.unmount(view);assert.deepEqual(f.calls.filter(c=>c.name==='embedding_control').map(c=>c.args.action),['enable']);
});
test('paused downloads resume without a new model choice and failures remain retryable',async t=>{
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{}}});let state={...initial,paused:true,preparing:true,state:'paused',downloaded:300000000};f.overrides.embedding_status=async()=>state;f.overrides.embedding_control=async({action})=>{assert.equal(action,'resume');state={...state,paused:false,state:'failed',error:'模型校验失败'};};
 const view=f.mount(f.load('src/workspace/EmbeddingSettings.tsx').default);await f.settle();f.find(view,n=>n.type==='button'&&f.text(n)==='继续').props.onClick();await f.settle();assert(f.find(view,n=>n.type==='button'&&f.text(n)==='重试'));assert(!f.text(view.tree).includes('已就绪'));
});
