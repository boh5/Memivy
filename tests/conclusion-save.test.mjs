import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
function setup(t) {
  const f=workspaceFixture(t);f.overrides.discussion_targets=async()=>[['a','目标 A']];
  const view=f.mount(f.load('src/workspace/SaveConclusion.tsx').default,{message:{id:'message',text:'整段回答',answer:{conclusion:'确认结论'}},topic:f.topic,context:[],onClose(){},onSaved(){}});
  return {f,view};
}
const button=(f,v,label)=>f.find(v,n=>n.type==='button'&&f.text(n)===label);
const select=(f,v,id)=>f.find(v,n=>n.type==='select').props.onChange({target:{value:id}});
test('a failed destination read cannot silently create a new memory',async t=>{
  const {f,view}=setup(t);f.overrides.library_detail=async()=>{throw '目标已删除';};await f.settle();
  select(f,view,'a');await f.settle();
  const save=button(f,view,'确认保存新记忆');assert(save.props.disabled);save.props.onClick();await f.settle();
  assert.equal(f.calls.filter(c=>c.name==='discussion_save').length,0);
});
test('late merge previews cannot overwrite edited conclusions',async t=>{
  const {f,view}=setup(t);let finish;f.overrides.discussion_merge=()=>new Promise(r=>finish=r);await f.settle();
  select(f,view,'a');await f.settle();button(f,view,'预览融合成文').props.onClick();await f.settle();
  f.find(view,n=>n.type==='textarea').props.onChange({target:{value:'修改后的结论'}});await f.settle();finish('过期融合');await f.settle();
  assert(!f.text(view.tree).includes('审核融合后的完整正文'));
  button(f,view,'确认补充到记忆').props.onClick();await f.settle();
  const call=f.calls.find(c=>c.name==='discussion_save');assert.equal(call.args.request.text,'修改后的结论');assert.equal(call.args.mergedBody,null);
});
test('reviewed integration saves the exact edit and original destination version once',async t=>{
  const {f,view}=setup(t);f.overrides.discussion_merge=async()=> '模型融合预览';await f.settle();select(f,view,'a');await f.settle();
  button(f,view,'预览融合成文').props.onClick();await f.settle();
  f.find(view,n=>n.type==='textarea'&&n.props.rows===10).props.onChange({target:{value:'我审核后修改的完整正文'}});await f.settle();
  const save=button(f,view,'确认融合并保存');save.props.onClick();save.props.onClick();await f.settle();
  const calls=f.calls.filter(c=>c.name==='discussion_save');assert.equal(calls.length,1);assert.equal(calls[0].args.mergedBody,'我审核后修改的完整正文');
  assert.equal(calls[0].args.request.destination.expected_version,'v-a');assert.equal(calls[0].args.request.text,'确认结论');
});
test('a reopened conflicting conclusion exposes and edits the complete retained manuscript', async t => {
  const f=workspaceFixture(t);
  const reviewed={title:'审核标题',body:'完整融合稿\n保留手工细节。'};
  f.overrides.library_detail=async ({key})=>({key,state:'active',title:'短结论',body:'短结论',current:null,history:[],sources:[],reviewed_conclusion:reviewed});
  f.overrides.library_edit=async()=>({request_id:'saved',memory_id:'new-memory',capture_id:'raw',status:'applied'});
  const view=f.mount(f.load('src/workspace/MemoryDetail.tsx').default,{record:{kind:'capture',id:'raw'},revision:0,query:'',initialReceipt:null,onChanged(){},onBack(){},onDiscuss(){}});
  await f.settle();
  assert(f.text(view.tree).includes(reviewed.body));
  button(f,view,'审核保留稿并另存').props.onClick();await f.settle();
  const node=f.find(view,n=>typeof n.type==='function' && n.type.name==='Editor');
  const editor=f.mount(node.type,node.props);await f.settle();
  assert.equal(f.find(editor,n=>n.type==='textarea').props.value,reviewed.body);
  assert.equal(f.find(editor,n=>n.type==='input').props.value,reviewed.title);
  button(f,editor,'确认另存为新记忆').props.onClick();await f.settle();
  const saved=f.calls.find(c=>c.name==='library_edit');
  assert.equal(saved.args.draft.body,reviewed.body);
  assert.equal(saved.args.draft.title,reviewed.title);
  assert.equal(saved.args.draft.key,'capture:raw');
});
