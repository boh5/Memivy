import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const c={id:'collection-a',name:'面试准备',description:'技术问答',revision:1,count:2};

test('collection scope follows top input into a discussion and remains visible',async t=>{
 const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
 f.overrides.navigation_collections=async()=>[c];f.overrides.discussion_ask=async()=>({...f.topic,collection_id:c.id});
 const app=f.mount(App);await f.settle();f.find(app,n=>n.type===Sidebar).props.onCollection(c.id);await f.settle();
 assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('仅在 面试准备 中提问'));
 await f.find(f.query(app),n=>n.type===Form).props.onAsk('我还缺什么？','question-one');await f.settle();
 assert.equal(f.calls.find(v=>v.name==='discussion_ask').args.collectionId,c.id);
 assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes('仅在 面试准备 中提问'));
 f.nodes(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).find(n=>n.props['aria-label']==='改为从全部记忆提问').props.onClick();await f.settle();
 await f.find(f.query(app),n=>n.type===Form).props.onAsk('全库提问','question-two');await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='discussion_ask')[1].args.collectionId,null);
});

test('review retrieves three older memories at a time and replaces the batch',async t=>{
 const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
 f.overrides.library_query=async({query})=>({items:[{key:query.offset?f.keyB:f.keyA,title:query.offset?'第二组':'第一组',snippet:'',origin:null,updated_at:1}],next_offset:query.offset?null:3});
 const view=f.mount(List,{trash:false,active:true,selected:null,review:true,revision:0,onSelect(){},onCapture(){},onRefresh(){}});await f.settle();
 const first=f.calls.find(v=>v.name==='library_query').args.query;assert.equal(first.limit,3);assert.equal(first.oldest,true);
 f.find(view,n=>n.props.className==='load-more review-next').props.onClick();await f.settle();
 assert(f.text(view.tree).includes('第二组'));assert(!f.text(view.tree).includes('第一组'));
});

test('AI suggestions are read-only until an explicit add and repeated clicks write once',async t=>{
 const f=workspaceFixture(t),Suggestions=f.load('src/workspace/CollectionSuggestions.tsx').default;
 f.overrides.navigation_suggest=async()=>[{key:f.keyA,title:'过去的面试想法',snippet:'原文片段'}];let complete;
 f.overrides.navigation_collect=()=>new Promise(r=>{complete=r;});
 const view=f.mount(Suggestions,{collection:c,onChanged(){},onClose(){}});await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='navigation_collect').length,0);
 const button=f.find(view,n=>n.type==='button'&&f.text(n)==='加入');button.props.onClick();button.props.onClick();await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='navigation_collect').length,1);complete();await f.settle();
 assert(f.find(view,n=>n.type==='button'&&f.text(n)==='已加入').props.disabled);
});

test('collection create failure keeps edited name and description for retry',async t=>{
 const f=workspaceFixture(t),Editor=f.load('src/workspace/CollectionEditor.tsx').default;
 f.overrides.navigation_save_collection=async()=>{throw '重名或已修改';};let saved=false;
 const view=f.mount(Editor,{onSaved(){saved=true;},onClose(){}});await f.settle();
 f.find(view,n=>n.props['aria-label']==='专题名称').props.onChange({target:{value:'我的专题'}});
 f.find(view,n=>n.props['aria-label']==='专题说明').props.onChange({target:{value:'要保留的说明'}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='创建专题').props.onClick();await f.settle();
 assert.equal(saved,false);assert.equal(f.find(view,n=>n.props['aria-label']==='专题名称').props.value,'我的专题');
 assert.equal(f.find(view,n=>n.props['aria-label']==='专题说明').props.value,'要保留的说明');
});
