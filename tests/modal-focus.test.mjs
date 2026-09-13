import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {JSDOM} from 'jsdom';
import React,{useState} from 'react';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';

test('dialogs focus their intended control after opening and restore focus without scrolling',async()=>{
 const dom=new JSDOM('<div id="root"></div>');
 const previous={window:globalThis.window,document:globalThis.document,HTMLElement:globalThis.HTMLElement};
 Object.assign(globalThis,{window:dom.window,document:dom.window.document,HTMLElement:dom.window.HTMLElement});
 // Model the browser's initial dialog focus; JSDOM has no modal top layer.
 dom.window.HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');this.querySelector('button')?.focus();};
 dom.window.HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};
 const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
 const {createRoot}=await import('react-dom/client'),{flushSync}=await import('react-dom');
 let root;
 try{
  const i18n=createInstance();await i18n.init({lng:'en',resources:{en:{workspace:{components:{close:'Close {{title}}'}}}},interpolation:{escapeValue:false}});
  const {Modal}=await server.ssrLoadModule('/src/workspace/components.tsx');
  function App(){const [open,setOpen]=useState(false);return React.createElement(React.Fragment,null,
   React.createElement('button',{id:'trigger',onClick:()=>setOpen(true)},'Open'),
   open&&React.createElement(Modal,{title:'Choose memory',onClose:()=>setOpen(false)},
    React.createElement('input',{'data-modal-autofocus':true,'aria-label':'Search memories'})));}
  root=createRoot(document.getElementById('root'));flushSync(()=>root.render(React.createElement(I18nextProvider,{i18n},React.createElement(App))));
  const trigger=document.getElementById('trigger');trigger.focus();
  flushSync(()=>trigger.click());
  assert.equal(document.activeElement,document.querySelector('input'));
  let restoreOptions;const focus=trigger.focus.bind(trigger);trigger.focus=options=>{restoreOptions=options;focus(options);};
  flushSync(()=>document.querySelector('dialog button').click());
  assert.equal(document.querySelector('dialog'),null);assert.equal(document.activeElement,trigger);
  assert.equal(restoreOptions.preventScroll,true);
 }finally{if(root)flushSync(()=>root.unmount());await new Promise(resolve=>setTimeout(resolve,20));await server.close();dom.window.close();Object.assign(globalThis,previous);}
});
