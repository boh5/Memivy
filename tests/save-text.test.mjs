import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
function setup(t) {
 const f=workspaceFixture(t),view=f.mount(f.load('src/workspace/SaveText.tsx').default,{message:{id:'message',turn_id:'input-1',text:'待保存的文字'},topic:f.topic,onClose(){},onSaved(){}});
 return {f,view};
}
const save=(f,v)=>f.find(v,n=>n.type==='button'&&f.text(n)==='保存文字为记忆');
const select=(f,v,id)=>f.find(v,n=>n.type==='select').props.onChange({target:{value:id}});
test('failed destination lookup cannot silently save to a different destination',async t=>{
 const {f,view}=setup(t);await f.settle();f.overrides.library_detail=async()=>{throw '目标已删除';};
 select(f,view,'a');await f.settle();assert(save(f,view).props.disabled);save(f,view).props.onClick();await f.settle();
 assert(!f.calls.some(c=>c.name==='discussion_save_text'));
});
test('manual save sends edited text and the selected version exactly once',async t=>{
 const {f,view}=setup(t);await f.settle();select(f,view,'a');await f.settle();
 f.find(view,n=>n.type==='MarkdownEditor').props.onChange('我修改过的文字');await f.settle();
 f.overrides.discussion_save_text=async()=>({status:'applied'});
 save(f,view).props.onClick();save(f,view).props.onClick();await f.settle();
 const calls=f.calls.filter(c=>c.name==='discussion_save_text');assert.equal(calls.length,1);
 assert.equal(calls[0].args.text,'我修改过的文字');assert.equal(calls[0].args.destination.expected_version,'v-a');assert.equal(calls[0].args.inputId,'input-1');
});
test('an unacknowledged manual save reuses its request and keeps the draft across reopening',async t=>{
 const {f,view}=setup(t);await f.settle();select(f,view,'a');await f.settle();
 f.overrides.discussion_save_text=async()=>{throw '暂时断开';};
 for(let i=0;i<2;i++){save(f,view).props.onClick();await f.settle();}
 const calls=f.calls.filter(c=>c.name==='discussion_save_text');assert.equal(calls.length,2);assert.equal(calls[0].args.id,calls[1].args.id);
 f.unmount(view);const reopened=f.mount(f.load('src/workspace/SaveText.tsx').default,view.props);await f.settle();
 assert.equal(f.find(reopened,n=>n.type==='MarkdownEditor').props.value,'待保存的文字');
 assert.equal(f.db.get('save:message').destination.expected_version,'v-a');
});
