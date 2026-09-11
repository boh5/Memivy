import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

async function editMemory(t, version='v-a') {
  const f=workspaceFixture(t);
  const detail={key:f.keyA,state:'active',title:'标题',body:'## 原来的正文',current:{id:version,capture_ids:[]},history:[],sources:[]};
  f.overrides.library_detail=async()=>detail;
  f.overrides.library_edit=async()=>({request_id:'saved',memory_id:'a'});
  const page=f.mount(f.load('src/workspace/MemoryDetail.tsx').default,{record:f.keyA,revision:0,query:'',initialReceipt:null,onChanged(){},onBack(){},onDiscuss(){}});
  await f.settle();
  f.find(page,n=>n.props.label==='编辑正文').props.onClick();await f.settle();
  const node=f.find(page,n=>typeof n.type==='function'&&n.type.name==='Editor');
  const editor=f.mount(node.type,node.props);await f.settle();
  return {f,editor,node,detail};
}
test('immediate rich-editor save flushes the latest Markdown before IPC',async t=>{
  const {f,editor}=await editMemory(t);
  const field=f.find(editor,n=>n.type==='MarkdownEditor');
  const body='## 中文标题\n\n**末次输入**\n\n> 引用\n\n- 列表\n';
  field.props.onChange(body);
  // No rerender or debounce wait between the last document update and Save.
  field.props.onSave();await f.settle();
  assert.equal(f.calls.find(c=>c.name==='library_edit').args.draft.body,body);
  assert(!f.db.has('memory:a'));
});
test('rich-editor shortcut cannot bypass a version conflict',async t=>{
  const {f,editor,detail}=await editMemory(t);
  f.find(editor,n=>n.type==='MarkdownEditor').props.onChange('保留我的草稿');await f.settle();
  f.render(editor,{...editor.props,detail:{...detail,current:{...detail.current,id:'v-new'}}});
  f.find(editor,n=>n.type==='MarkdownEditor').props.onSave();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='library_edit').length,0);
  assert.equal(f.db.get('memory:a').body,'保留我的草稿');
});

test('title and body edits do not replace the editor helper while persistence is pending',async t=>{
 const {f,editor}=await editMemory(t);let finish;
 f.overrides.draft_write=()=>new Promise(resolve=>{finish=resolve;});
 const heading=()=>f.text(f.find(editor,n=>n.props.className==='section-heading'));
 const before=heading();
 for(const change of [()=>f.find(editor,n=>n.props['aria-label']==='编辑记忆标题').props.onChange({target:{value:'新标题'}}),()=>f.find(editor,n=>n.type==='MarkdownEditor').props.onChange('新正文')]){
  change();await f.settle();assert.equal(heading(),before);finish();await f.settle();assert.equal(heading(),before);
 }
});
