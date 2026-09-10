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
  f.find(view,n=>n.type==='MarkdownEditor').props.onChange('修改后的结论');await f.settle();finish('过期融合');await f.settle();
  assert(!f.text(view.tree).includes('审核融合后的完整正文'));
  button(f,view,'确认补充到记忆').props.onClick();await f.settle();
  const call=f.calls.find(c=>c.name==='discussion_save');assert.equal(call.args.request.text,'修改后的结论');assert.equal(call.args.mergedBody,null);
});
test('reviewed integration saves the exact edit and original destination version once',async t=>{
  const {f,view}=setup(t);f.overrides.discussion_merge=async()=> '模型融合预览';await f.settle();select(f,view,'a');await f.settle();
  button(f,view,'预览融合成文').props.onClick();await f.settle();
  f.find(view,n=>n.type==='MarkdownEditor'&&n.props.label==='审核融合后的完整正文').props.onChange('我审核后修改的完整正文');await f.settle();
  const save=button(f,view,'确认融合并保存');save.props.onClick();save.props.onClick();await f.settle();
  const calls=f.calls.filter(c=>c.name==='discussion_save');assert.equal(calls.length,1);assert.equal(calls[0].args.mergedBody,'我审核后修改的完整正文');
  assert.equal(calls[0].args.request.destination.expected_version,'v-a');assert.equal(calls[0].args.request.text,'确认结论');
});
test('conflicting save retains the complete review across reopening and requires target recheck', async t => {
  const {f,view}=setup(t); let version='v-a';
  f.overrides.library_detail=async({key})=>({key,state:'active',title:'目标 A',body:'当前正文',current:{id:version},history:[],sources:[]});
  f.overrides.discussion_merge=async()=> '完整融合稿\n保留手工细节。';
  f.overrides.discussion_save=async()=>({status:'needs_review',capture_id:null,memory_id:null});
  await f.settle(); select(f,view,'a'); await f.settle();
  button(f,view,'预览融合成文').props.onClick(); await f.settle();
  button(f,view,'确认融合并保存').props.onClick(); await f.settle();
  const stored=f.db.get('conclusion:message');
  assert.equal(stored.conclusion.merged_body,'完整融合稿\n保留手工细节。');
  assert.equal(stored.conclusion.destination.expected_version,'v-a');
  assert(button(f,view,'确认融合并保存').props.disabled);
  f.unmount(view);version='v-new';
  const reopened=f.mount(f.load('src/workspace/SaveConclusion.tsx').default,view.props);await f.settle();
  assert.equal(f.find(reopened,n=>n.type==='MarkdownEditor'&&n.props.label==='审核融合后的完整正文').props.value,stored.conclusion.merged_body);
  assert(button(f,reopened,'确认融合并保存').props.disabled);
  button(f,reopened,'重新读取目标并核对').props.onClick();await f.settle();
  f.overrides.discussion_save=async()=>({status:'applied',memory_id:'a'});
  button(f,reopened,'确认融合并保存').props.onClick();await f.settle();
  const saved=f.calls.filter(c=>c.name==='discussion_save').at(-1);
  assert.equal(saved.args.request.destination.expected_version,'v-new');
  assert.equal(saved.args.mergedBody,stored.conclusion.merged_body);
});

test('a late regenerated preview cannot overwrite hand edits to the complete manuscript', async t=>{
  const {f,view}=setup(t);await f.settle();select(f,view,'a');await f.settle();
  f.overrides.discussion_merge=async()=> '第一版融合稿';
  button(f,view,'预览融合成文').props.onClick();await f.settle();
  let finish;f.overrides.discussion_merge=()=>new Promise(resolve=>finish=resolve);
  button(f,view,'预览融合成文').props.onClick();await f.settle();
  f.find(view,n=>n.type==='MarkdownEditor'&&n.props.label==='审核融合后的完整正文').props.onChange('用户正在核对的完整稿');await f.settle();
  finish('迟到的新融合稿');await f.settle();
  assert.equal(f.find(view,n=>n.type==='MarkdownEditor'&&n.props.label==='审核融合后的完整正文').props.value,'用户正在核对的完整稿');
  assert.equal(f.db.get('conclusion:message').conclusion.merged_body,'用户正在核对的完整稿');
});

test('an unacknowledged save retries the original request without rebasing the destination', async t=>{
  const {f,view}=setup(t);await f.settle();select(f,view,'a');await f.settle();
  f.overrides.discussion_save=async()=>{throw '数据库忙';};
  button(f,view,'确认补充到记忆').props.onClick();await f.settle();
  button(f,view,'确认补充到记忆').props.onClick();await f.settle();
  const calls=f.calls.filter(c=>c.name==='discussion_save');assert.equal(calls.length,2);
  assert.equal(calls[0].args.request.request_id,calls[1].args.request.request_id);
  assert.equal(calls[1].args.request.destination.expected_version,'v-a');
});
