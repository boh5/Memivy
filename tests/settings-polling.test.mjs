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

const modelSettings = { llm: null, embedding: { source: 'local' }, voice: { source: 'local' }, revision: 'test' };
const settingsProps = { onClose(){}, onChanged(){}, onRestore(){} };
const timers = { setTimeout, clearTimeout, setInterval: () => 1, clearInterval(){} };

test('reloading a failed direct voice settings entry restores the form', async t => {
 const f = workspaceFixture(t, { native: true, timers });
 f.overrides.models_load = async () => { throw 'Could not read settings'; };
 f.overrides.embedding_status = async () => ({});
 const view = f.mount(f.load('src/workspace/Settings.tsx').default, { ...settingsProps, initialPage: 'voice' });
 await f.settle();
 f.overrides.models_load = async () => modelSettings;
 f.find(view, n => n.type === 'button' && f.text(n) === 'Reload settings').props.onClick();
 await f.settle();
 const capability = f.find(view, n => n.type === f.load('src/workspace/ModelCapability.tsx').default);
 assert.equal(capability.props.kind, 'voice');
 assert.equal(capability.props.draft, modelSettings.voice);
});

test('retry reads the failing model status and clears only its warning on recovery', async t => {
 const f = workspaceFixture(t, { native: true, timers });
 f.overrides.models_load = async () => modelSettings;
 f.overrides.embedding_status = async () => { throw 'Could not read embedding'; };
 const view = f.mount(f.load('src/workspace/Settings.tsx').default, { ...settingsProps, initialPage: 'ai' });
 await f.settle();
 assert(f.find(view, n => n.props.text === 'Could not read the semantic search status.'));
 assert(!f.nodes(view.tree).some(n => n.props.text === 'Could not read the voice input status.'));
 f.overrides.embedding_status = async () => ({ enabled: true });
 f.find(view, n => n.type === 'button' && f.text(n) === 'Retry reading model status').props.onClick();
 await f.settle();
 assert(!f.nodes(view.tree).some(n => n.props.text === 'Could not read the semantic search status.'));
 assert.equal(f.calls.filter(call => call.name === 'embedding_status').length, 2);
 assert.equal(f.calls.filter(call => call.name === 'voice_status').length, 1);
});

test('keyword search repair shows progress and failure in the dialog, then invalidates search after retry', async t => {
 const invalidations = [];
 const f = workspaceFixture(t, { native: true, timers, modules: {
   './resources': { invalidateResources: async changes => invalidations.push(...structuredClone(changes)) },
 } });
 f.overrides.models_load = async () => modelSettings;
 f.overrides.embedding_status = async () => ({});
 let failRepair;
 f.overrides.library_rebuild = () => new Promise((_, reject) => { failRepair = reject; });
 const view = f.mount(f.load('src/workspace/Settings.tsx').default, { ...settingsProps, initialPage: 'data' });
 const button = label => f.find(view, n => n.type === 'button' && f.text(n) === label);
 const dialog = () => f.find(view, n => n.props.title === 'Repair keyword search?');
 await f.settle();
 button('Repair').props.onClick(); await f.settle();
 button('Start repair').props.onClick(); await f.settle();
 assert(f.text(dialog()).includes('Rebuilding the keyword search index…'));
 failRepair('Repair could not finish'); await f.settle();
 assert(f.nodes(dialog()).some(n => n.props.text === 'Repair could not finish'));
 assert.deepEqual(invalidations, []);
 f.overrides.library_rebuild = async () => {};
 button('Retry').props.onClick(); await f.settle();
 assert(!f.nodes(view.tree).some(n => n.props.title === 'Repair keyword search?'));
 assert(f.text(view.tree).includes('Keyword search index rebuilt.'));
 assert.deepEqual(invalidations, [{ domain: 'search' }]);
});
