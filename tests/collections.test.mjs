import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';
const c={id:'collection-a',name:"Interview preparation",description:"Technical Q&A",revision:1,count:2};

test('collection scope follows top input into a discussion and remains visible',async t=>{
 const f=workspaceFixture(t),App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Form=f.load('src/workspace/CaptureForm.tsx').default;
 f.overrides.navigation_collections=async()=>[c];f.overrides.discussion_submit=async()=>({...f.topic,collection_id:c.id});
 const app=f.mount(App);await f.settle();f.find(app,n=>n.type===Sidebar).props.onCollection(c.id);await f.settle();
 assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes("Focusing on Interview preparation"));
 await f.find(f.query(app),n=>n.type===Form).props.onSubmit({text:"What am I missing?",id:'question-one',context:[]});await f.settle();
 assert.equal(f.calls.find(v=>v.name==='discussion_submit').args.collectionId,c.id);
 assert(f.text(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).includes("Focusing on Interview preparation"));
 f.nodes(f.find(f.query(app),n=>n.type===f.load('src/workspace/WorkspaceTopBar.tsx').default).props.scope).find(n=>n.props['aria-label']==="Ask across all memories").props.onClick();await f.settle();
 await f.find(f.query(app),n=>n.type===Form).props.onSubmit({text:"Library-wide question",id:'question-two',context:[]});await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='discussion_submit')[1].args.collectionId,null);
});

test('review retrieves three older memories at a time and replaces the batch',async t=>{
 const f=workspaceFixture(t),List=f.load('src/workspace/MemoryList.tsx').default;
 f.overrides.library_query=async({query})=>({items:[{key:query.offset?f.keyB:f.keyA,title:query.offset?"Second group":"First group",snippet:'',origin:null,updated_at:1}],next_offset:query.offset?null:3});
 const view=f.mount(List,{trash:false,active:true,selected:null,review:true,revision:0,onSelect(){},onCapture(){},onRefresh(){}});await f.settle();
 const first=f.calls.find(v=>v.name==='library_query').args.query;assert.equal(first.limit,3);assert.equal(first.oldest,true);
 f.find(view,n=>n.props.className==='load-more review-next').props.onClick();await f.settle();
 assert(f.text(view.tree).includes("Second group"));assert(!f.text(view.tree).includes("First group"));
});

test('AI suggestions are read-only until an explicit add and repeated clicks write once',async t=>{
 const f=workspaceFixture(t),Suggestions=f.load('src/workspace/CollectionSuggestions.tsx').default;
 f.overrides.navigation_suggest=async()=>[{key:f.keyA,title:"Previous interview ideas",snippet:"Original excerpt"}];let complete;
 f.overrides.navigation_collect=()=>new Promise(r=>{complete=r;});
 const view=f.mount(Suggestions,{collection:c,onChanged(){},onClose(){}});await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='navigation_collect').length,0);
 const button=f.find(view,n=>n.type==='button'&&f.text(n)==="Add");button.props.onClick();button.props.onClick();await f.settle();
 assert.equal(f.calls.filter(v=>v.name==='navigation_collect').length,1);complete();await f.settle();
 assert(f.find(view,n=>n.type==='button'&&f.text(n)==="Added").props.disabled);
});

test('collection create failure keeps edited name and description for retry',async t=>{
 const f=workspaceFixture(t),Editor=f.load('src/workspace/CollectionEditor.tsx').default;
 f.overrides.navigation_save_collection=async()=>{throw "Duplicate name or modified";};let saved=false;
 const view=f.mount(Editor,{onSaved(){saved=true;},onClose(){}});await f.settle();
 f.find(view,n=>n.props['aria-label']==="Collection name").props.onChange({target:{value:"My collection"}});
 f.find(view,n=>n.props['aria-label']==="Collection description").props.onChange({target:{value:"Description to preserve"}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Create collection").props.onClick();await f.settle();
 assert.equal(saved,false);assert.equal(f.find(view,n=>n.props['aria-label']==="Collection name").props.value,"My collection");
 assert.equal(f.find(view,n=>n.props['aria-label']==="Collection description").props.value,"Description to preserve");
});
test('workspace collection actions open their dialogs and submit through the existing commands',async t=>{
 const f=workspaceFixture(t);f.overrides.navigation_collections=async()=>[c];f.overrides.navigation_save_collection=async()=>{};f.overrides.navigation_archive_collection=async()=>{};f.overrides.navigation_suggest=async()=>[];
 const App=f.load('src/workspace/App.tsx').default,Sidebar=f.load('src/workspace/WorkspaceSidebar.tsx').default,Editor=f.load('src/workspace/CollectionEditor.tsx').default,Suggestions=f.load('src/workspace/CollectionSuggestions.tsx').default;
 const app=f.mount(App);await f.settle();
 const sidebarNode=f.find(app,n=>n.type===Sidebar),sidebar=f.mount(Sidebar,sidebarNode.props);await f.settle();
 f.find(sidebar,n=>n.type==='button'&&n.props['aria-label']==="New collection").props.onClick();await f.settle();
 let editorNode=f.find(app,n=>n.type===Editor),editor=f.mount(Editor,editorNode.props);await f.settle();
 f.find(editor,n=>n.props['aria-label']==="Collection name").props.onChange({target:{value:"New test collection"}});await f.settle();
 f.find(editor,n=>n.type==='button'&&f.text(n)==="Create collection").props.onClick();await f.settle();
 assert.equal(f.calls.find(call=>call.name==='navigation_save_collection').args.name,"New test collection");assert(!f.nodes(app.tree).some(n=>n.type===Editor));
 sidebarNode.props.onCollection(c.id);await f.settle();
 f.find(app,n=>n.type==='button'&&f.text(n)==="Edit collection").props.onClick();await f.settle();
 editorNode=f.find(app,n=>n.type===Editor);assert.equal(editorNode.props.value.id,c.id);editor=f.mount(Editor,editorNode.props);await f.settle();
 f.find(editor,n=>n.props['aria-label']==="Collection name").props.onChange({target:{value:"Updated collection"}});await f.settle();
 f.find(editor,n=>n.type==='button'&&f.text(n)==="Save").props.onClick();await f.settle();
 assert.equal(f.calls.filter(call=>call.name==='navigation_save_collection')[1].args.expected,c.revision);
 f.find(app,n=>n.type==='button'&&f.text(n)==="AI suggestions").props.onClick();await f.settle();
 const suggestionNode=f.find(app,n=>n.type===Suggestions);const suggestions=f.mount(Suggestions,suggestionNode.props);await f.settle();
 assert.equal(f.calls.find(call=>call.name==='navigation_suggest').args.collection,c.id);suggestionNode.props.onClose();await f.settle();f.unmount(suggestions);
 f.find(app,n=>n.type==='button'&&f.text(n)==="Remove collection").props.onClick();await f.settle();
 const dialog=f.find(app,n=>n.props?.title==="Remove this collection?");assert(f.text(dialog).includes(c.name));
 f.nodes(dialog).find(n=>n.type==='button'&&f.text(n)==="Remove collection").props.onClick();await f.settle();
 assert.equal(f.calls.find(call=>call.name==='navigation_archive_collection').args.id,c.id);
 assert(!f.nodes(app.tree).some(n=>n.props?.title==="Remove this collection?"));
});
