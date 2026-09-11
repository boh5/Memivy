import test from 'node:test';
import assert from 'node:assert/strict';
import {JSDOM} from 'jsdom';
import {createServer} from 'vite';
import React from 'react';
import {createRoot} from 'react-dom/client';
import {flushSync} from 'react-dom';

test('real React reconciliation preserves unchanged Markdown nodes, selection and table viewport',async()=>{
  const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
  const dom=new JSDOM('<!doctype html><div id="root"></div>');
  const previous={window:globalThis.window,document:globalThis.document};
  globalThis.window=dom.window;globalThis.document=dom.window.document;
  let root;
  try {
    const {default:Markdown}=await server.ssrLoadModule('/src/workspace/Markdown.tsx');
    root=createRoot(document.getElementById('root'));
    const text='# Title\n\nUnchanged paragraph\n\n- Item\n\n| A | B |\n| --- | --- |\n| 1 | 2 |';
    const draw=query=>flushSync(()=>root.render(React.createElement(Markdown,{text,query})));
    draw('');
    const selector='p,h1,li,.markdown-table-scroll';
    const before=[...document.querySelectorAll(selector)];
    const paragraph=before.find(n=>n.tagName==='P');
    const range=document.createRange();range.selectNodeContents(paragraph);
    const selection=window.getSelection();selection.addRange(range);
    const table=document.querySelector('.markdown-table-scroll');table.scrollLeft=37;
    // A different nonmatching query bypasses memo while leaving rendered text unchanged.
    draw('no-match');draw('another-no-match');
    const after=[...document.querySelectorAll(selector)];
    assert.equal(before.length,after.length);
    before.forEach((node,i)=>assert.equal(node,after[i]));
    assert.equal(selection.toString(),'Unchanged paragraph');
    assert.equal(table.scrollLeft,37);
  } finally {
    if(root)flushSync(()=>root.unmount());
    await new Promise(resolve=>setTimeout(resolve,20));
    globalThis.window=previous.window;globalThis.document=previous.document;
    dom.window.close();await server.close();
  }
});
