import test from 'node:test';
import assert from 'node:assert/strict';
import {JSDOM} from 'jsdom';
import {createServer} from 'vite';
import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';

test('real input DOM and helper text survive draft writes and acknowledgements',async()=>{
 const dom=new JSDOM('<div id="root"></div>');
 const previous={window:globalThis.window,document:globalThis.document};
 globalThis.window=dom.window;globalThis.document=dom.window.document;
 const pending=[];
 globalThis.__feedbackCall=async name=>{if(name==='draft_read')return null;if(name==='draft_write')return new Promise(resolve=>pending.push(resolve));};
 const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent',plugins:[{name:'isolated-draft-ipc',enforce:'pre',load(id){if(id.endsWith('/src/workspace/api.ts'))return `export const call=(...args)=>globalThis.__feedbackCall(...args); export const uid=()=>crypto.randomUUID(); export const errorText=String;`;}}]});
 let root;
 const settle=()=>new Promise(resolve=>setTimeout(resolve,30));
 try{
  const {default:Form}=await server.ssrLoadModule('/src/workspace/CaptureForm.tsx');
  root=createRoot(document.getElementById('root'));
  for(const [presentation,quick,mode] of [['query',false,'ask'],['capture',false,'capture'],['panel',true,'capture'],['panel',true,'ask']]){
   flushSync(()=>root.render(React.createElement(Form,{key:presentation+mode,presentation,quick,mode,focus:0,onSaved(){},onAsk(){},onMode(){},onEdit(){}})));
   await settle();
   const input=document.querySelector('textarea'),footer=document.querySelector('.composer-bottom,.recall-panel-footer'),help=footer.querySelector('span');
   const original=help.textContent,changes=[];
   const observer=new window.MutationObserver(records=>changes.push(...records));observer.observe(help,{subtree:true,characterData:true,childList:true,attributes:true});
   // Exercise the real React handler with the real DraftQueue and delayed IPC.
   const props=()=>input[Object.keys(input).find(k=>k.startsWith('__reactProps$'))];
   for(const value of ['a','ab','中文输入']){
    flushSync(()=>props().onChange({target:{value}}));await settle();
    assert.equal(document.querySelector('textarea'),input);assert.equal(help.textContent,original);
    pending.shift()();await settle();
    assert.equal(input.value,value);assert.equal(help.textContent,original);
   }
   assert.equal(document.querySelector('.composer-bottom,.recall-panel-footer'),footer);
   assert.equal(changes.length,0,'no helper text/attribute/child mutations during typing or acknowledgements');observer.disconnect();
  }
 }finally{
  if(root)flushSync(()=>root.unmount());await settle();await server.close();dom.window.close();Object.assign(globalThis,previous);delete globalThis.__feedbackCall;
 }
});
