import test from 'node:test';
import assert from 'node:assert/strict';
import { DraftQueue, DRAFT_CONFLICT } from '../src/workspace/draftQueue.ts';

const draft = (key, request_id, body, context = []) => ({key, request_id, body, title: '', expected_version: null, context});
const deferred = () => { let resolve; const promise = new Promise(r => {resolve = r;}); return {promise,resolve}; };
function fixture() {
  const disk = new Map();
  const writes = [];
  let before = async () => {};
  const queue = new DraftQueue(async (name, args) => {
    await before(name, args);
    if (name === 'draft_read') return structuredClone(disk.get(args.key) || null);
    if (name === 'draft_clear') return disk.delete(args.key);
    const value = args.draft;
    if (Buffer.byteLength(value.title) > 200) throw 'invalid title';
    writes.push(value);
    disk.set(value.key, structuredClone(value));
  });
  return {queue,disk,writes,intercept(fn){before=fn;}};
}

test('a failed draft does not block another key, but still prevents silent exit', async () => {
  const f=fixture(), a={...draft('memory:a','a1',"Editing draft"),title:'中文'.repeat(40)};
  await assert.rejects(f.queue.write(a), e=>e==='invalid title');
  const b=draft('capture','b1',"Another valid note");
  await f.queue.write(b); await f.queue.flush('capture');
  assert.deepEqual(f.disk.get('capture'),b);
  await assert.rejects(f.queue.flushAll(), e=>e==='invalid title');
  await f.queue.consume(a.key,a.request_id,null);
  await f.queue.flushAll();
});

test('a blocked write cannot delay another record', async () => {
  const f=fixture(), gate=deferred(), started=deferred();
  f.intercept(async(name,args)=>{if(name==='draft_write'&&args.draft.key==='a'){started.resolve();await gate.promise;}});
  const writeA=f.queue.write(draft('a','a1','slow'));await started.promise;
  await f.queue.write(draft('b','b1','ready'));await f.queue.flush('b');
  assert.equal(f.disk.get('b').body,'ready');gate.resolve();await writeA;
});

test('late submission completion cannot overwrite an edited, reopened draft', async () => {
  const f=fixture(), sent=draft('discussion:a','sent',"Question");
  await f.queue.write(sent);
  const newer=draft(sent.key,'new',"New draft after returning");
  await f.queue.write(newer);
  const consumed=await f.queue.consume(sent.key,sent.request_id,{...sent,request_id:'empty',body:''});
  assert.equal(consumed,false);
  await f.queue.flushAll();assert.deepEqual(f.disk.get(sent.key),newer);
});

test('completion updates a reopened input when the sent draft is still current', async () => {
  const f=fixture(), sent=draft('discussion:a','sent',"Question",[{kind:'capture',id:'source'}]);
  await f.queue.write(sent);
  const seen=[];const unsubscribe=f.queue.subscribe(sent.key,s=>seen.push(s));await f.queue.read(sent.key);
  const empty={...sent,request_id:'empty',body:''};
  assert.equal(await f.queue.consume(sent.key,sent.request_id,empty),true);
  assert.deepEqual(seen.at(-1),{draft:empty,error:null});
  assert.deepEqual(f.disk.get(sent.key),empty);unsubscribe();
});

test('editing during clear IPC wins both in the input and on disk', async () => {
  const f=fixture(), sent=draft('discussion:a','sent',"Question");await f.queue.write(sent);
  const gate=deferred(), started=deferred(), seen=[];f.queue.subscribe(sent.key,s=>seen.push(s.draft));
  f.intercept(async(name,args)=>{if(name==='draft_clear'){started.resolve();await gate.promise;}});
  const clearing=f.queue.consume(sent.key,sent.request_id,null);await started.promise;
  const newer=draft(sent.key,'new',"New input");const writing=f.queue.write(newer);
  gate.resolve();await clearing;await writing;await f.queue.flushAll();
  assert.deepEqual(f.disk.get(sent.key),newer);assert.deepEqual(seen.at(-1),newer);
  assert(!seen.includes(null));
});

test('an edit made during draft loading is not replaced by the old read', async () => {
  const f=fixture(), gate=deferred(), started=deferred(), seen=[];
  f.queue.subscribe('capture',s=>seen.push(s.draft));
  f.intercept(async name=>{if(name==='draft_read'){started.resolve();await gate.promise;}});
  const reading=f.queue.read('capture');await started.promise;
  const newer=draft('capture','new',"Input while loading");const writing=f.queue.write(newer);
  gate.resolve();await reading;await writing;
  assert.deepEqual(seen.at(-1),newer);assert(!seen.includes(null));
});

test('failed writes keep their latest text and can be retried', async () => {
  const f=fixture(), value=draft('capture','request',"Must preserve");let fail=true;const seen=[];
  f.queue.subscribe(value.key,s=>seen.push(s));
  f.intercept(async name=>{if(name==='draft_write'&&fail)throw 'busy';});
  await assert.rejects(f.queue.write(value),e=>e==='busy');
  assert.deepEqual(seen.at(-1),{draft:value,error:'busy'});
  assert.equal(f.disk.has(value.key),false);
  fail=false;await f.queue.flush(value.key);
  assert.deepEqual(f.disk.get(value.key),value);assert.deepEqual(seen.at(-1),{draft:value,error:null});
});

function twoWindows() {
  const disk = new Map();
  const call = async (name, args) => {
    if (name === 'draft_read') return structuredClone(disk.get(args.key) || null);
    const key=args.key || args.draft.key, old=disk.get(key);
    if (name === 'draft_clear') {
      if (old?.request_id !== args.request) return false;
      disk.delete(key); return true;
    }
    if (JSON.stringify(old) === JSON.stringify(args.draft)) return true;
    if ((old?.request_id || null) !== args.expectedRequest) return false;
    disk.set(key,structuredClone(args.draft)); return true;
  };
  return {disk,a:new DraftQueue(call),b:new DraftQueue(call)};
}
test('two windows never silently overwrite a concurrently edited draft', async () => {
  const {disk,a,b}=twoWindows();
  await a.read('quick_capture'); await b.read('quick_capture');
  await a.write(draft('quick_capture','a',"Window A note"));
  const seen=[];b.subscribe('quick_capture',s=>seen.push(s));
  await assert.rejects(b.write(draft('quick_capture','b',"Window B note")),e=>e===DRAFT_CONFLICT);
  assert.equal(disk.get('quick_capture').body,"Window A note");
  assert.equal(seen.at(-1).draft.body,"Window B note");
  await assert.rejects(b.flushAll(),e=>e===DRAFT_CONFLICT);
  await b.resolve('quick_capture',true,'a');
  assert.equal(disk.get('quick_capture').body,"Window B note");
  await a.refresh(); await a.flushAll();
});
test('a late save acknowledgement preserves a newer draft from another window', async () => {
  const {disk,a,b}=twoWindows();
  await a.write(draft('quick_capture','sent',"Saved original text"));
  await b.read('quick_capture');
  await b.write(draft('quick_capture','new',"New original text"));
  assert.equal(await a.consume('quick_capture','sent',null),false);
  assert.equal(disk.get('quick_capture').body,"New original text");
  await a.flushAll();
});
test('conflict resolution only replaces the version actually reviewed', async () => {
  const {disk,a,b}=twoWindows();
  await a.read('quick_question');await b.read('quick_question');
  await a.write(draft('quick_question','first',"First copy"));
  await assert.rejects(b.write(draft('quick_question','local',"Local content")));
  await a.write(draft('quick_question','later',"Changed again"));
  await assert.rejects(b.resolve('quick_question',true,'first'),e=>e===DRAFT_CONFLICT);
  assert.equal(disk.get('quick_question').body,"Changed again");
});

test('cross-window draft changes only read the affected mounted draft', async () => {
  const f=fixture(), reads=[];
  const stop=f.queue.subscribe('capture',()=>{});
  f.queue.subscribe('question',()=>{});
  await f.queue.read('old-editor');
  f.intercept(async(name,args)=>{if(name==='draft_read')reads.push(args.key);});
  await f.queue.refresh('capture');
  assert.deepEqual(reads,['capture']);
  stop();await f.queue.refresh('capture');await f.queue.refresh('unknown');
  assert.deepEqual(reads,['capture']);
});

test('typing after a failed save keeps the warning until successful persistence', async () => {
  const f=fixture(), seen=[];let fail=true;
  f.queue.subscribe('capture',s=>seen.push(s));
  f.intercept(async name=>{if(name==='draft_write'&&fail)throw 'disk unavailable';});
  await assert.rejects(f.queue.write(draft('capture','1','a')));
  const retry=f.queue.write(draft('capture','2','ab'));
  assert.equal(seen.at(-1).error,'disk unavailable');
  await assert.rejects(retry);
  fail=false;await f.queue.flush('capture');
  assert.equal(seen.at(-1).error,null);
  assert.equal(f.disk.get('capture').body,'ab');
});
