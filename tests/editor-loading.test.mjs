import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('failed editor imports retain a visible current draft and retry without writing it',async t=>{
 const f=workspaceFixture(t,{modules:{'react-dom/client':{createRoot(){throw Error('must not render an uninitialized editor');}}}});
 const Editor=f.load('src/workspace/MarkdownEditor.tsx').default;
 let changes=0;
 const props={label:'编辑记忆内容',value:'## 未保存的中文正文\n\n保留内容',onChange(){changes++;}};
 const view=f.mount(Editor,props);await f.settle();
 assert(f.text(view.tree).includes('编辑器资源未能加载'));
 assert.equal(f.find(view,n=>n.type==='Markdown').props.text,props.value);
 assert.equal(f.find(view,n=>n.props.className==='markdown-editor-host').props.hidden,true);
 f.render(view,{...props,value:'最近的草稿'});await f.settle();
 assert.equal(f.find(view,n=>n.type==='Markdown').props.text,'最近的草稿');
 f.find(view,n=>n.type==='button'&&f.text(n)==='重新加载').props.onClick();await f.settle();
 assert.equal(f.find(view,n=>n.type==='Markdown').props.text,'最近的草稿');
 assert.equal(changes,0);
});
