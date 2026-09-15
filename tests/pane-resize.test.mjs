import test from 'node:test';
import assert from 'node:assert/strict';
import { JSDOM } from 'jsdom';
import { createServer } from 'vite';
import React from 'react';
import { createRoot } from 'react-dom/client';
import { flushSync } from 'react-dom';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';

test('resize keyboard starts at visible width and never rerenders the reading sibling',async()=>{
  const dom = new JSDOM('<div id="root"></div>',{url:'http://localhost'});
  const previous = {window:globalThis.window,document:globalThis.document,ResizeObserver:globalThis.ResizeObserver};
  const callbacks = [];
  Object.assign(globalThis,{window:dom.window,document:dom.window.document,ResizeObserver:class {constructor(callback){callbacks.push(callback);}observe(){}disconnect(){}}});
  let root, reads = 0;
  const server = await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
  try {
    const i18n = createInstance();
    await i18n.init({lng:'en',resources:{en:{workspace:{nav:{resizeList:'Resize memory list'}}}}});
    const {default:Handle} = await server.ssrLoadModule('/src/workspace/PaneResizeHandle.tsx');
    window.localStorage.setItem('memivy.list-width','380');
    function Reader() { reads++; return React.createElement('textarea',{defaultValue:'Unsent draft'}); }
    root = createRoot(document.getElementById('root'));
    flushSync(()=>root.render(React.createElement('div',{className:'memory-app'},React.createElement('div',{className:'library-layout'},React.createElement('div',{className:'library-list-pane'}),React.createElement(Reader),React.createElement(I18nextProvider,{i18n},React.createElement(Handle,{pane:'list'}))))));
    const shell=document.querySelector('.memory-app'),layout=document.querySelector('.library-layout'),list=document.querySelector('.library-list-pane'),handle=document.querySelector('[role="separator"]');
    Object.defineProperty(layout,'clientWidth',{value:800,configurable:true});
    list.getBoundingClientRect=()=>({width:Math.min(272,parseFloat(shell.style.getPropertyValue('--preferred-list-width')))});
    flushSync(()=>callbacks.forEach(callback=>callback()));
    assert.equal(handle.getAttribute('aria-valuenow'),'272');
    assert.equal(handle.getAttribute('aria-valuemax'),'272');
    const input=document.querySelector('textarea');
    input.setSelectionRange(2,5);
    flushSync(()=>handle.dispatchEvent(new window.KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true})));
    assert.equal(shell.style.getPropertyValue('--preferred-list-width'),'262px');
    assert.equal(window.localStorage.getItem('memivy.list-width'),'262');
    assert.equal(document.querySelector('textarea'),input);assert.equal(input.value,'Unsent draft');
    assert.equal(input.selectionStart,2);assert.equal(reads,1);
  } finally {
    if(root)flushSync(()=>root.unmount());
    await new Promise(resolve=>setTimeout(resolve,30));
    await server.close();dom.window.close();Object.assign(globalThis,previous);
  }
});

test('resize follows replacement list panes and retains preferred width across narrow startup',async()=>{
  const dom = new JSDOM('<div id="root"></div>',{url:'http://localhost'});
  const previous = {window:globalThis.window,document:globalThis.document,ResizeObserver:globalThis.ResizeObserver};
  const observers = [];
  Object.assign(globalThis,{window:dom.window,document:dom.window.document,ResizeObserver:class {
    constructor(callback){this.callback=callback;this.targets=[];this.disconnected=false;observers.push(this);}
    observe(target){this.targets.push(target);}disconnect(){this.disconnected=true;}
  }});
  let root, available = 800;
  Object.defineProperty(dom.window.HTMLElement.prototype,'clientWidth',{get(){return this.classList.contains('library-layout')?available:1200;}});
  dom.window.HTMLElement.prototype.getBoundingClientRect=function(){return {width:this.isConnected?Math.min(available*.34,parseFloat(document.querySelector('.memory-app').style.getPropertyValue('--preferred-list-width'))):0};};
  const server = await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
  try {
    const i18n=createInstance();await i18n.init({lng:'en',resources:{en:{workspace:{nav:{resizeList:'Resize memory list'}}}}});
    const {default:Handle}=await server.ssrLoadModule('/src/workspace/PaneResizeHandle.tsx');
    window.localStorage.setItem('memivy.list-width','380');
    root=createRoot(document.getElementById('root'));
    const render=key=>flushSync(()=>root.render(React.createElement('div',{className:'memory-app'},React.createElement('div',{className:'library-layout'},
      React.createElement('div',{className:'library-list-pane',key}),
      React.createElement(I18nextProvider,{i18n},React.createElement(Handle,{pane:'list',targetKey:key}))))));
    render('library');
    const shell=document.querySelector('.memory-app'),original=document.querySelector('.library-list-pane');
    assert.equal(shell.style.getPropertyValue('--preferred-list-width'),'380px');
    flushSync(()=>observers.at(-1).callback());
    assert.equal(document.querySelector('[role="separator"]').getAttribute('aria-valuenow'),'272');
    render('trash');
    const replacement=document.querySelector('.library-list-pane');
    assert.notEqual(replacement,original);
    assert.equal(observers[0].disconnected,true);
    assert.ok(observers.at(-1).targets.includes(replacement));
    available=1000;
    flushSync(()=>observers.at(-1).callback());
    const handle=document.querySelector('[role="separator"]');
    assert.equal(handle.getAttribute('aria-valuenow'),'340');
    assert.equal(handle.getAttribute('aria-valuemax'),'340');
    flushSync(()=>handle.dispatchEvent(new window.KeyboardEvent('keydown',{key:'ArrowLeft',bubbles:true})));
    assert.equal(shell.style.getPropertyValue('--preferred-list-width'),'330px');
  } finally {
    if(root)flushSync(()=>root.unmount());
    await new Promise(resolve=>setTimeout(resolve,30));
    await server.close();dom.window.close();Object.assign(globalThis,previous);
  }
});
