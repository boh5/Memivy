import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {JSDOM} from 'jsdom';
import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';

test('shared selection commits once, keeps dialog open on Escape, and follows controlled values',async()=>{
 const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
 const dom=new JSDOM('<div id="root"></div>'), previous={window:globalThis.window,document:globalThis.document,Node:globalThis.Node};
 Object.assign(globalThis,{window:dom.window,document:dom.window.document,Node:dom.window.Node});
 dom.window.HTMLElement.prototype.showPopover=function(){this.dataset.open='true';};
 dom.window.HTMLElement.prototype.hidePopover=function(){delete this.dataset.open;};
 dom.window.HTMLElement.prototype.scrollIntoView=function(){};
 let root;
 try{
  const {default:Select}=await server.ssrLoadModule('/src/workspace/Select.tsx');
  root=createRoot(document.getElementById('root'));let changes=[],escapes=0;
  const render=(value='a',disabled=false)=>flushSync(()=>root.render(React.createElement('label',{onKeyDown:e=>{if(e.key==='Escape')escapes++;}},'Choose',React.createElement(Select,{'aria-label':'Choose',value,disabled,onChange:e=>changes.push(e.target.value)},['a','b','c'].map(v=>React.createElement('option',{key:v,value:v},v))))));
  render();const button=document.querySelector('button');button.focus();
  const key=value=>flushSync(()=>button.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true})));
  key('ArrowDown');key('ArrowDown');assert.deepEqual(changes,[]);
  key('Enter');assert.deepEqual(changes,['b']);assert.equal(button.getAttribute('aria-expanded'),'false');
  render('b');assert.equal(button.textContent,'b');
  key('ArrowDown');key('Escape');assert.equal(escapes,0);assert.equal(button.getAttribute('aria-expanded'),'false');
  flushSync(()=>button.click());flushSync(()=>document.querySelectorAll('[role="option"]')[2].click());
  assert.deepEqual(changes,['b','c']);assert.equal(button.getAttribute('aria-expanded'),'false','enclosing label must not reopen the menu');
  render('c',true);assert.equal(button.disabled,true);
 }finally{if(root)flushSync(()=>root.unmount());await new Promise(resolve=>setTimeout(resolve,20));Object.assign(globalThis,previous);dom.window.close();await server.close();}
});
