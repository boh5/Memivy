import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

for (const [presentation,quick] of [['query',false],['discussion',false],['panel',true]]) {
  test(`input help remains stable during slow writes: ${presentation}/${quick}`,async t=>{
    const f=workspaceFixture(t),Form=f.load('src/workspace/CaptureForm.tsx').default;
    const view=f.mount(Form,{presentation,quick,focus:0,onSubmit:async()=>{},onEdit(){}});
    await f.settle();
    const footer=()=>f.find(view,n=>['composer-bottom','recall-panel-footer'].includes(n.props.className));
    const before=f.text(footer());let finish;
    f.overrides.draft_write=()=>new Promise(resolve=>{finish=resolve;});
    f.find(f.composer(view),n=>n.type==='textarea').props.onChange({target:{value:'中文输入'}});
    await f.settle();assert.equal(f.text(footer()),before);
    finish();await f.settle();assert.equal(f.text(footer()),before);
  });
}

test('discussion help is stable while its draft is pending',async t=>{
 const f=workspaceFixture(t),view=f.mount(f.load('src/workspace/Discussion.tsx').default,{topic:f.topic,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
 await f.settle();let finish;f.overrides.draft_write=()=>new Promise(resolve=>{finish=resolve;});
 f.find(f.composer(view),n=>n.type==='textarea').props.onChange({target:{value:"Continue discussion"}});await f.settle();
 assert(f.text(f.composer(view).tree).includes("⌘ Enter to send · Enter for a new line"));
 assert(!f.text(view.tree).includes("Saving draft"));finish();await f.settle();
 assert(f.text(f.composer(view).tree).includes("⌘ Enter to send · Enter for a new line"));
});
