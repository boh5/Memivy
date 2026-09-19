import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('parent model polling follows settings pages and ignores results after leaving',async t=>{
 const intervals=new Set();
 const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,
  setInterval(fn){intervals.add(fn);return fn;},clearInterval(fn){intervals.delete(fn);},
 }});
 f.overrides.models_load=async()=>({llm:null,embedding:null,voice:null});
 f.overrides.embedding_status=async()=>({});
 const view=f.mount(f.load('src/workspace/Settings.tsx').default,{onClose(){},onChanged(){},onRestore(){}});
 const reads=()=>f.calls.filter(c=>c.name==='embedding_status'||c.name==='voice_status').length;
 const navigate=async label=>{f.find(view,n=>n.type==='button'&&f.text(n)===label).props.onClick();await f.settle();};
 await f.settle();assert.equal(reads(),0);assert.equal(intervals.size,0);
 await navigate('External access');assert.equal(reads(),0);assert.equal(intervals.size,0);
 await navigate('AI & models');assert.equal(reads(),2);assert.equal(intervals.size,1);
 for(const poll of intervals)poll();await f.settle();assert.equal(reads(),4);
 const Overview=f.load('src/workspace/ModelOverview.tsx').default;
 f.find(view,n=>n.type===Overview).props.onConfigure('voice');await f.settle();
 assert.equal(intervals.size,1);
 for(const poll of intervals)poll();await f.settle();assert.equal(reads(),6);
 await navigate('Data & storage');assert.equal(intervals.size,1);
 let rejectRead;
 f.overrides.embedding_status=()=>new Promise((_,reject)=>{rejectRead=reject;});
 for(const poll of intervals)poll();await f.settle();assert.equal(reads(),8);
 await navigate('General');assert.equal(intervals.size,0);
 rejectRead('Late model status failure');await f.settle();
 assert(!f.nodes(view.tree).some(n=>n.props.className==='model-error'));
 await navigate('External access');assert.equal(reads(),8);assert.equal(intervals.size,0);
 f.overrides.embedding_status=async()=>({});
 await navigate('AI & models');assert.equal(reads(),10);assert.equal(intervals.size,1);
 f.unmount(view);assert.equal(intervals.size,0);
});
