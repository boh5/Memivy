import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
const wait = () => new Promise(resolve => setTimeout(resolve,200));

test('background list refresh retains full-opacity rows; filters still show loading',async t=>{
 const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
 const props={trash:false,active:true,selected:null,revision:0,onSelect(){},onCapture(){},onRefresh(){}};
 const view=f.mount(List,props);await f.settle();let finish;
 f.overrides.library_query=()=>new Promise(resolve=>{finish=resolve;});
 f.render(view,{...props,revision:1});await f.settle();
 const rows=()=>f.find(view,n=>n.props.className?.startsWith('library-rows'));
 assert.equal(rows().props['aria-busy'],true);
 assert(!rows().props.className.includes('is-loading'));
 assert(f.nodes(view.tree).some(n=>n.props.className?.startsWith('library-row ')));
 finish({items:[],next_offset:null});await f.settle();
 f.render(view,{...props,revision:1,collectionId:'different'});await f.settle();
 assert(rows().props.className.includes('is-loading'));
});

test('background detail refresh keeps the reading toolbar enabled',async t=>{
 const f=workspaceFixture(t),Detail=f.load('src/workspace/MemoryDetail.tsx').default;
 const props={record:f.keyA,revision:0,query:'',initialReceipt:null,onChanged(){},onDiscuss(){},onBack(){}};
 const view=f.mount(Detail,props);await f.settle();
 f.overrides.library_detail=()=>new Promise(()=>{});
 f.render(view,{...props,revision:1});await f.settle();
 assert.equal(f.find(view,n=>n.props.label==="Edit body").props.disabled,false);
 assert.equal(f.find(view,n=>n.props.label==="Clean up body").props.disabled,false);
});

test('related results and selection survive refresh, but vanish when the source or scope changes',async t=>{
 const f=workspaceFixture(t),Related=f.load('src/workspace/RelatedMemories.tsx').default;
 const row={memory_id:'b',version_id:'bv',title:"Related content",snippet:"Excerpt",source:{kind:'version',id:'bv'}};
 f.overrides.memory_related=()=>[row];
 const props={memoryId:'a',versionId:'av',revision:0,onOpen(){},onDiscuss(){}};
 const view=f.mount(Related,props);await wait();await f.settle();
 f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
 let finish;f.overrides.memory_related=()=>new Promise(resolve=>{finish=resolve;});
 f.render(view,{...props,revision:1});await f.settle();
 assert(f.text(view.tree).includes("Related content"));
 assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
 await wait();finish([]);await f.settle();assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
 f.overrides.memory_related=()=>[row];f.render(view,{...props,revision:2});await wait();await f.settle();
 f.render(view,{...props,revision:2,collectionId:'new-scope'});await f.settle();
 assert.equal(view.tree,null);
});

test('background refresh preserves every loaded page, including more than the core 100-row limit',async t=>{
 const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
 const all=Array.from({length:160},(_,i)=>({key:{kind:'memory',id:String(i)},title:String(i),snippet:'',updated_at:1,origin:null}));
 f.overrides.library_query=({query})=>{const offset=query.offset||0,limit=Math.min(query.limit,100);return {items:all.slice(offset,offset+limit),next_offset:offset+limit<all.length?offset+limit:null};};
 const props={trash:false,active:true,selected:null,revision:0,onSelect(){},onCapture(){},onRefresh(){}};
 const view=f.mount(List,props);await f.settle();
 for(let i=0;i<2;i++){f.find(view,n=>n.props.className==='load-more').props.onClick();await f.settle();}
 const count=()=>f.nodes(view.tree).filter(n=>n.props['data-record']).length;
 assert.equal(count(),120);f.render(view,{...props,revision:1});await f.settle();assert.equal(count(),120);
});
test('transient detail failure retains reading content; explicit unavailability removes it',async t=>{
 const f=workspaceFixture(t),Detail=f.load('src/workspace/MemoryDetail.tsx').default;
 const props={record:f.keyA,revision:0,query:'',onChanged(){},onDiscuss(){},onBack(){}};
 const view=f.mount(Detail,props);await f.settle();
 f.overrides.library_detail=()=>Promise.reject({code:'busy'});
 f.render(view,{...props,revision:1});await f.settle();
 assert(f.nodes(view.tree).some(n=>n.props.label==="Edit body"));
 f.overrides.library_detail=()=>Promise.reject({code:'unavailable'});
 f.render(view,{...props,revision:2});await f.settle();
 assert(!f.nodes(view.tree).some(n=>n.props.label==="Edit body"));
});
test('desktop state rejects a late snapshot after a newer native event',async t=>{
 const f=workspaceFixture(t,{native:true});let finish;
 f.overrides.desktop_state=()=>new Promise(resolve=>{finish=resolve;});
 const {useDesktop}=f.load('src/workspace/desktopApi.ts');
 const view=f.mount(useDesktop);await f.settle();
 f.emit('desktop-state',{expanded:true,generation:2,sequence:9});await f.settle();
 finish({expanded:false,generation:1,sequence:8});await f.settle();
 assert.equal(view.tree.state.expanded,true);assert.equal(view.tree.state.sequence,9);
});

test('a background head change preserves the displayed version until explicitly opened',async t=>{
 const f=workspaceFixture(t),Detail=f.load('src/workspace/MemoryDetail.tsx').default;
 const document=version=>({key:f.keyA,state:'active',title:'Title',body:'body '+version,current:{id:version,capture_ids:[],created_at:1,actor:'user'},history:[],sources:[]});
 f.overrides.library_detail=()=>document('v1');
 const props={record:f.keyA,revision:0,query:'',onChanged(){},onDiscuss(){},onBack(){}};
 const view=f.mount(Detail,props);await f.settle();
 f.overrides.library_detail=()=>document('v2');
 f.render(view,{...props,revision:1});await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='body v1'));
 f.find(view,n=>n.type==='button'&&f.text(n)==="New version available; view").props.onClick();await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='body v2'));
});

test('the explicit list refresh button still retries after global revision removal',async t=>{
 const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
 const view=f.mount(List,{trash:false,active:true,selected:null,onSelect(){},onCapture(){},onRefresh(){}});await f.settle();
 const before=f.calls.filter(call=>call.name==='library_query').length;
 f.find(view,n=>n.props['aria-label']==="Refresh memory list").props.onClick();await f.settle();
 assert.equal(f.calls.filter(call=>call.name==='library_query').length,before+1);
});

test('pending external heads suspend related queries without clearing chosen evidence',async t=>{
 const f=workspaceFixture(t),Related=f.load('src/workspace/RelatedMemories.tsx').default;
 f.overrides.memory_related=()=>[{memory_id:'b',version_id:'bv',title:'chosen',snippet:'text',source:{kind:'version',id:'bv'}}];
 const props={memoryId:'a',versionId:'av',onOpen(){},onDiscuss(){}};
 const view=f.mount(Related,props);await wait();await f.settle();
 f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
 f.render(view,{...props,revision:1,paused:true});await wait();await f.settle();
 assert.equal(f.calls.filter(call=>call.name==='memory_related').length,1);
 assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
});
