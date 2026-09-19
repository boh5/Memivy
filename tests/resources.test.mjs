import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import vm from 'node:vm';
import ts from 'typescript';
import * as query from '@tanstack/react-query';
import * as react from 'react';
function fixture(t, invoke) {
  const clients=[];
  class Client extends query.QueryClient { constructor(options){super(options);clients.push(this);} }
  const module={exports:{}};
  const code=ts.transpileModule(fs.readFileSync('src/workspace/resources.ts','utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
  vm.runInNewContext(code,{module,exports:module.exports,console,require:name=>{
    if(name==='@tanstack/react-query')return {...query,QueryClient:Client};
    if(name==='../nativeIpc')return {invoke};
    if(name==='@tauri-apps/api/core')return {invoke,isTauri:()=>true};
    if(name==='@tauri-apps/api/event')return {listen:async()=>()=>{}};
    if(name==='react')return react;
    throw Error(name);
  }});
  t.after(()=>clients.forEach(client=>client.clear()));
  return module.exports;
}
test('resource cache deduplicates reads and invalidates only dependent entities',async t=>{
  const calls=[];
  const r=fixture(t,async(name,args)=>{calls.push(args.key.id);return {body:'body '+args.key.id};});
  const read=id=>r.resourceCall('library_detail',{key:{kind:'memory',id}});
  const [a,a2,b]=await Promise.all([read('a'),read('a'),read('b')]);
  assert.equal(a,a2);assert.deepEqual(calls,['a','b']);
  await r.invalidateResources([{domain:'discussion',entity:'topic'}]);
  assert.equal(await read('a'),a);assert.equal(calls.length,2);
  await r.invalidateResources([{domain:'memory',entity:'memory:a'}]);
  await read('a');assert.equal(calls.length,3);assert.equal(await read('b'),b);
});
test('cancelled old IPC responses cannot overwrite a newer cached result',async t=>{
  let oldReply,newReply,calls=0;
  const r=fixture(t,()=>new Promise(resolve=>{if(++calls===1)oldReply=resolve;else newReply=resolve;}));
  const read=()=>r.resourceCall('library_detail',{key:{kind:'memory',id:'a'}});
  const first=read();await Promise.resolve();
  await r.invalidateResources([{domain:'memory',entity:'memory:a'}]);
  const second=read();await new Promise(resolve=>setTimeout(resolve,0));
  newReply({body:'new'});assert.equal((await second).body,'new');
  oldReply({body:'old'});assert.equal((await first).body,'new');
  assert.equal((await read()).body,'new');assert.equal(calls,2);
});
test('duplicate notification hints with an unchanged cursor do not refetch data',async t=>{
  let cursor=null,reads=0;
  const r=fixture(t,async(name,args)=>{
    if(name==='library_changes'){const reset=!args.cursor;cursor={epoch:'one',sequence:3};return {cursor,reset,changes:[]};}
    reads++;return {body:'stable'};
  });
  await r.syncResources();
  await r.resourceCall('library_detail',{key:{kind:'memory',id:'a'}});
  await Promise.all([r.syncResources(),r.syncResources(),r.syncResources()]);
  await r.resourceCall('library_detail',{key:{kind:'memory',id:'a'}});
  assert.equal(reads,1);assert.equal(cursor.sequence,3);
});
test('stream updates expire only their conversation and retain other cached reads',async t=>{
  const calls=[];
  const r=fixture(t,async(name,args)=>{calls.push([name,args.id??args.key?.id]);return {text:'reply'};});
  const read=id=>r.resourceCall('discussion_messages',{id});
  await read('a');await read('b');await r.resourceCall('library_detail',{key:{kind:'memory',id:'note'}});
  await r.expireQueries(['discussion_messages'],[{domain:'discussion',entity:'a'}]);
  await read('a');await read('b');await r.resourceCall('library_detail',{key:{kind:'memory',id:'note'}});
  assert.deepEqual(calls,[['discussion_messages','a'],['discussion_messages','b'],['library_detail','note'],['discussion_messages','a']]);
});
