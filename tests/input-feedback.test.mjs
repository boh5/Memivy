import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

for (const [presentation,quick,mode] of [['query',false,'ask'],['capture',false,'capture'],['panel',true,'capture'],['panel',true,'ask']]) {
  test(`input help remains stable during slow writes: ${presentation}/${mode}`,async t=>{
    const f=workspaceFixture(t),Form=f.load('src/workspace/CaptureForm.tsx').default;
    const view=f.mount(Form,{presentation,quick,mode,focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}});
    await f.settle();
    const footer=()=>f.find(view,n=>['composer-bottom','recall-panel-footer'].includes(n.props.className));
    const before=f.text(footer());let finish;
    f.overrides.draft_write=()=>new Promise(resolve=>{finish=resolve;});
    f.find(view,n=>n.type==='textarea').props.onChange({target:{value:'中文输入'}});
    await f.settle();assert.equal(f.text(footer()),before);
    finish();await f.settle();assert.equal(f.text(footer()),before);
  });
}

test('discussion help is stable while its draft is pending',async t=>{
 const f=workspaceFixture(t),view=f.mount(f.load('src/workspace/Discussion.tsx').default,{topic:f.topic,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
 await f.settle();let finish;f.overrides.draft_write=()=>new Promise(resolve=>{finish=resolve;});
 f.find(view,n=>n.type==='textarea').props.onChange({target:{value:'继续讨论'}});await f.settle();
 assert(f.text(view.tree).includes('确认后才存为记忆 · ⌘ Enter 发送'));
 assert(!f.text(view.tree).includes('保存草稿中'));finish();await f.settle();
 assert(f.text(view.tree).includes('确认后才存为记忆 · ⌘ Enter 发送'));
});
