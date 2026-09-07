import test from 'node:test';
import assert from 'node:assert/strict';
import { DraftQueue } from '../src/workspace/draftQueue.ts';

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
  const f=fixture(), a={...draft('memory:a','a1','编辑草稿'),title:'中文'.repeat(40)};
  await assert.rejects(f.queue.write(a), e=>e==='invalid title');
  const b=draft('capture','b1','另一条有效记录');
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
  const f=fixture(), sent=draft('discussion:a','sent','问题');
  await f.queue.write(sent);
  const newer=draft(sent.key,'new','切回来后的新草稿');
  await f.queue.write(newer);
  const consumed=await f.queue.consume(sent.key,sent.request_id,{...sent,request_id:'empty',body:''});
  assert.equal(consumed,false);
  await f.queue.flushAll();assert.deepEqual(f.disk.get(sent.key),newer);
});

test('completion updates a reopened input when the sent draft is still current', async () => {
  const f=fixture(), sent=draft('discussion:a','sent','问题',[{kind:'capture',id:'source'}]);
  await f.queue.write(sent);
  const seen=[];const unsubscribe=f.queue.subscribe(sent.key,s=>seen.push(s));await f.queue.read(sent.key);
  const empty={...sent,request_id:'empty',body:''};
  assert.equal(await f.queue.consume(sent.key,sent.request_id,empty),true);
  assert.deepEqual(seen.at(-1),{draft:empty,saved:true,error:null});
  assert.deepEqual(f.disk.get(sent.key).context,sent.context);unsubscribe();
});

test('editing during clear IPC wins both in the input and on disk', async () => {
  const f=fixture(), sent=draft('discussion:a','sent','问题');await f.queue.write(sent);
  const gate=deferred(), started=deferred(), seen=[];f.queue.subscribe(sent.key,s=>seen.push(s.draft));
  f.intercept(async(name,args)=>{if(name==='draft_clear'){started.resolve();await gate.promise;}});
  const clearing=f.queue.consume(sent.key,sent.request_id,null);await started.promise;
  const newer=draft(sent.key,'new','新输入');const writing=f.queue.write(newer);
  gate.resolve();await clearing;await writing;await f.queue.flushAll();
  assert.deepEqual(f.disk.get(sent.key),newer);assert.deepEqual(seen.at(-1),newer);
  assert(!seen.includes(null));
});

test('an edit made during draft loading is not replaced by the old read', async () => {
  const f=fixture(), gate=deferred(), started=deferred(), seen=[];
  f.queue.subscribe('capture',s=>seen.push(s.draft));
  f.intercept(async name=>{if(name==='draft_read'){started.resolve();await gate.promise;}});
  const reading=f.queue.read('capture');await started.promise;
  const newer=draft('capture','new','加载期间的输入');const writing=f.queue.write(newer);
  gate.resolve();await reading;await writing;
  assert.deepEqual(seen.at(-1),newer);assert(!seen.includes(null));
});

test('failed writes keep their latest text and can be retried', async () => {
  const f=fixture(), value=draft('capture','request','不能丢');let fail=true;const seen=[];
  f.queue.subscribe(value.key,s=>seen.push(s));
  f.intercept(async name=>{if(name==='draft_write'&&fail)throw 'busy';});
  await assert.rejects(f.queue.write(value),e=>e==='busy');
  assert.deepEqual(seen.at(-1),{draft:value,saved:false,error:'busy'});
  fail=false;await f.queue.flush(value.key);
  assert.deepEqual(f.disk.get(value.key),value);assert.equal(seen.at(-1).saved,true);
});
