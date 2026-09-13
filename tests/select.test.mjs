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
 let root;
 try{
  const {default:Select}=await server.ssrLoadModule('/src/workspace/Select.tsx');
  root=createRoot(document.getElementById('root'));let changes=[],escapes=0;
  const render=(value='a',disabled=false,options=['a','b','c'])=>flushSync(()=>root.render(React.createElement('label',{onKeyDown:e=>{if(e.key==='Escape')escapes++;}},'Choose',React.createElement(Select,{'aria-label':'Choose',value,disabled,onChange:e=>changes.push(e.target.value)},options.map(v=>React.createElement('option',{key:v,value:v},v))))));
  render();const button=document.querySelector('button');button.focus();
  const key=value=>flushSync(()=>button.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true})));
  key('ArrowDown');key('ArrowDown');assert.deepEqual(changes,[]);
  key('Enter');assert.deepEqual(changes,['b']);assert.equal(button.getAttribute('aria-expanded'),'false');
  render('b');assert.equal(button.textContent,'b');
  key('ArrowDown');key('Escape');assert.equal(escapes,0);assert.equal(button.getAttribute('aria-expanded'),'false');
  flushSync(()=>button.click());flushSync(()=>document.querySelectorAll('[role="option"]')[2].click());
  assert.deepEqual(changes,['b','c']);assert.equal(button.getAttribute('aria-expanded'),'false','enclosing label must not reopen the menu');
  render('c',true);assert.equal(button.disabled,true);

  // Model the list's scroll viewport: opening a restored late selection must
  // reveal it, and keyboard navigation must only scroll this list.
  const options=Array.from({length:30},(_,i)=>String(i));
  render('25',false,options);
  const menu=document.querySelector('[role="listbox"]');
  Object.defineProperties(menu,{clientHeight:{value:120},clientTop:{value:1}});
  menu.getBoundingClientRect=()=>({top:200,bottom:322});
  [...menu.children].forEach((row,index)=>{row.getBoundingClientRect=()=>({top:201+index*40-menu.scrollTop,bottom:241+index*40-menu.scrollTop});});
  const visible=index=>{const r=menu.children[index].getBoundingClientRect();assert(r.top>=201&&r.bottom<=321);};
  flushSync(()=>button.click());visible(25);assert.equal(menu.scrollTop,920);
  key('Home');visible(0);assert.equal(menu.scrollTop,0);
  key('End');visible(29);
  key('Escape');render('1',false,options);
  flushSync(()=>button.click());visible(1);
  assert.equal(button.getAttribute('aria-activedescendant'),menu.children[1].id);
  assert.equal(document.documentElement.scrollTop,0);
 }finally{if(root)flushSync(()=>root.unmount());await new Promise(resolve=>setTimeout(resolve,20));Object.assign(globalThis,previous);dom.window.close();await server.close();}
});
