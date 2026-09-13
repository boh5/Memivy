import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {JSDOM} from 'jsdom';
import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';

test('shared action popovers keep focus, dismissal and edge placement inside scrolling dialogs',async()=>{
 const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
 const dom=new JSDOM('<div id="root"></div>'),previous={window:globalThis.window,document:globalThis.document,Node:globalThis.Node,HTMLElement:globalThis.HTMLElement};
 Object.assign(globalThis,{window:dom.window,document:dom.window.document,Node:dom.window.Node,HTMLElement:dom.window.HTMLElement});
 const rect=(left,top,width,height)=>({left,top,width,height,right:left+width,bottom:top+height});
 // JSDOM has no top layer or layout. Keep the native API boundary explicit;
 // actual WebKit rendering is checked in the main and capture windows.
 dom.window.HTMLElement.prototype.showPopover=function(){
  this.dataset.open='true'; const event=new dom.window.Event('toggle');event.newState='open';this.dispatchEvent(event);
 };
 dom.window.HTMLElement.prototype.hidePopover=function(){
  delete this.dataset.open; const event=new dom.window.Event('toggle');event.newState='closed';this.dispatchEvent(event);
 };
 dom.window.HTMLElement.prototype.getBoundingClientRect=function(){
  if(this.classList.contains('record-more-trigger')||this.classList.contains('action-tooltip'))return rect(286,190,24,24);
  if(this.getAttribute('role')==='menu')return rect(0,0,170,120);
  if(this.getAttribute('role')==='tooltip')return rect(0,0,140,30);
  return rect(0,0,0,0);
 };
 Object.defineProperty(window,'innerWidth',{configurable:true,value:320});
 Object.defineProperty(window,'innerHeight',{configurable:true,value:240});
 let root;
 try {
  const i18n=createInstance();await i18n.init({lng:'en',resources:{en:{workspace:{components:{more:'More actions'}}}},interpolation:{escapeValue:false}});
  const {MoreMenu}=await server.ssrLoadModule('/src/workspace/components.tsx');
  const {ActionTooltip}=await server.ssrLoadModule('/src/workspace/IconButton.tsx');
  root=createRoot(document.getElementById('root'));let chosen=0,modalEscapes=0;
  const render=element=>flushSync(()=>root.render(React.createElement(I18nextProvider,{i18n},element)));
  render(React.createElement('section',{onKeyDown:event=>{if(event.key==='Escape')modalEscapes++;},style:{overflow:'hidden',height:40}},
   React.createElement(MoreMenu,null,
    React.createElement('button',{disabled:true},'Disabled'),
    React.createElement('button',{onClick:()=>chosen++},'Save text'),
    React.createElement('button',{onClick:()=>chosen++},'Delete')),
   React.createElement('button',{id:'outside'},'Outside')));
  const trigger=document.querySelector('.record-more-trigger'),menu=document.querySelector('[role="menu"]'),tooltip=document.querySelector('[role="tooltip"]');
  const key=(target,value,extra={})=>flushSync(()=>target.dispatchEvent(new dom.window.KeyboardEvent('keydown',{key:value,bubbles:true,cancelable:true,...extra})));
  assert.equal(trigger.textContent,'','the trigger is only an icon');assert.equal(trigger.getAttribute('aria-label'),'More actions');
  assert.equal(menu.getAttribute('popover'),'auto');assert.equal(tooltip.getAttribute('popover'),'manual');
  flushSync(()=>trigger.focus());assert.equal(tooltip.dataset.open,'true');
  assert.equal(tooltip.style.left,'170px');assert.equal(tooltip.style.top,'154px','tooltip flips upward near the bottom edge');
  key(trigger,'ArrowDown');
  assert.equal(menu.dataset.open,'true');assert.equal(tooltip.dataset.open,undefined);
  assert.equal(document.activeElement.textContent,'Save text','disabled items are skipped');
  assert.equal(menu.style.left,'140px');assert.equal(menu.style.top,'64px');assert.equal(menu.style.maxHeight,'174px');
  key(document.activeElement,'ArrowUp');assert.equal(document.activeElement.textContent,'Delete');
  key(document.activeElement,'Home');assert.equal(document.activeElement.textContent,'Save text');
  key(document.activeElement,'End');assert.equal(document.activeElement.textContent,'Delete');
  key(document.activeElement,'Escape');assert.equal(menu.dataset.open,undefined);assert.equal(document.activeElement,trigger);assert.equal(modalEscapes,0);
  key(trigger,'ArrowUp');assert.equal(document.activeElement.textContent,'Delete');
  flushSync(()=>document.activeElement.click());assert.equal(chosen,1);assert.equal(trigger.getAttribute('aria-expanded'),'false');assert.equal(document.activeElement,trigger);
  flushSync(()=>trigger.click());key(document.activeElement,'Tab');assert.equal(menu.dataset.open,undefined);assert.equal(document.activeElement,trigger,'Tab resumes from the trigger in document order');
  flushSync(()=>trigger.click());flushSync(()=>{document.getElementById('outside').focus();menu.hidePopover();});
  assert.equal(trigger.getAttribute('aria-expanded'),'false');assert.equal(document.activeElement.id,'outside','native light dismissal does not steal outside focus');
  flushSync(()=>trigger.click());flushSync(()=>window.dispatchEvent(new dom.window.Event('resize')));assert.equal(menu.dataset.open,undefined);

  render(React.createElement(ActionTooltip,{label:'More actions'},React.createElement('button',null,'Action')));
  const wrapper=document.querySelector('.action-tooltip'),tip=document.querySelector('[role="tooltip"]');
  flushSync(()=>wrapper.dispatchEvent(new dom.window.MouseEvent('mouseover',{bubbles:true})));assert.equal(tip.dataset.open,'true');
  key(window,'Escape');assert.equal(tip.dataset.open,undefined,'Escape dismisses a hover even when focus is elsewhere');
  flushSync(()=>wrapper.querySelector('button').focus());assert.equal(tip.dataset.open,'true');
  flushSync(()=>window.dispatchEvent(new dom.window.Event('scroll')));assert.equal(tip.dataset.open,undefined);
 } finally {
  if(root)flushSync(()=>root.unmount());await new Promise(resolve=>setTimeout(resolve,20));Object.assign(globalThis,previous);dom.window.close();await server.close();
 }
});
