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
 notify("Old message",undefined,undefined,25);await f.settle();notify("Captured",undefined,undefined,25);await f.settle();
 assert(!f.text(view.tree).includes("Old message"));
 f.find(view,n=>n.props.className==='workspace-toast').props.onMouseEnter();time.tick(40);await f.settle();assert(f.text(view.tree).includes("Captured"));
 f.find(view,n=>n.props.className==='workspace-toast').props.onMouseLeave();time.tick(40);await f.settle();assert(!f.text(view.tree).includes("Captured"));f.unmount(view);
});
test('keyboard focus keeps a toast alive after the pointer leaves and focus moves inside it',async t=>{
 const time=clock(),f=workspaceFixture(t,{timers:time.timers}),{default:Toast,notify}=f.load('src/workspace/Toast.tsx');const view=f.mount(Toast);await f.settle();
 notify("Reversible action","Undo",()=>{},25);await f.settle();
 const box=()=>f.find(view,n=>n.props.className==='workspace-toast');
 box().props.onMouseEnter();box().props.onFocus();box().props.onMouseLeave();
 box().props.onBlur({currentTarget:{contains:()=>true},relatedTarget:{}});
 time.tick(45);await f.settle();assert(f.text(view.tree).includes("Reversible action"));
 box().props.onBlur({currentTarget:{contains:()=>false},relatedTarget:null});
 time.tick(45);await f.settle();assert(!f.text(view.tree).includes("Reversible action"));
});

test('a toast action can publish its result without the old toast clearing it',async t=>{
 const time=clock(),f=workspaceFixture(t,{timers:time.timers}),{default:Toast,notify}=f.load('src/workspace/Toast.tsx');const view=f.mount(Toast);await f.settle();
 notify("Added","Undo",()=>notify("Undone"));await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Undo").props.onClick();await f.settle();
 assert(f.text(view.tree).includes("Undone"));
});


test('language changes update the existing export toast without publishing it again',async t=>{
 const f=workspaceFixture(t,{native:true});await f.language('zh-CN');const Detail=f.load('src/workspace/MemoryDetail.tsx').default;
 const {default:Toast}=f.load('src/workspace/Toast.tsx');
 const view=f.mount(Detail,{record:f.keyA,onChanged(){},onDiscuss:async()=>{},onBack(){}}),toast=f.mount(Toast);
 f.overrides.memory_export=async()=>'/tmp/Original.md';
 await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='导出 Markdown').props.onClick();await f.settle();
 assert(f.text(toast.tree).includes('/tmp/Original.md'));
 await f.language('en');
 assert(f.text(toast.tree).includes('exported'));
 f.find(toast,n=>n.type==='button'&&n.props['aria-label']==='Dismiss notification').props.onClick();await f.settle();
 await f.language('zh-CN');
 assert(!f.text(toast.tree).includes('/tmp/Original.md'),'a dismissed export notification must stay dismissed');
 assert.equal(f.calls.filter(c=>c.name==='memory_export').length,1);
});

test('discussion failures keep their recovery guidance across languages and preserve existing text',async t=>{
 const f=workspaceFixture(t);await f.language('zh-CN');const Discussion=f.load('src/workspace/Discussion.tsx').default;
 const failures=[
  ['invalid_answer','这次回答缺少有效依据','lacked valid supporting evidence'],
  ['source_unavailable','请重新选择记忆后提问','Select memories again before asking'],
  ['interrupted','可以继续提问','You can continue asking questions'],
 ];
 const history='Historical answer / 原有回答';
 f.messages(()=>[
  ...failures.map(([code],i)=>({id:`failure-${i}`,turn_id:`turn-${i}`,role:'assistant',status:code==='interrupted'?'interrupted':'failed',error_code:code,text:'',citations:[]})),
  {id:'history',turn_id:'old-turn',role:'assistant',status:'complete',text:history,citations:[]},
 ]);
 const view=f.mount(Discussion,{topic:f.topic,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
 await f.settle();
 for(const [,zh] of failures) assert(f.text(view.tree).includes(zh));
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text===history));
 const reads=f.calls.filter(c=>c.name==='discussion_messages').length;
 await f.language('en');
 for(const [,,en] of failures) assert(f.text(view.tree).includes(en));
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text===history));
 assert.equal(f.calls.filter(c=>c.name==='discussion_messages').length,reads,'translating a failure must not reload the conversation');
});
