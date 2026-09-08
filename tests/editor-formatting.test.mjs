import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'vite';
import {Schema} from '@milkdown/kit/prose/model';
import {AllSelection,EditorState,TextSelection} from '@milkdown/kit/prose/state';
import {history,undo} from '@milkdown/kit/prose/history';
const schema=new Schema({nodes:{doc:{content:'block+'},text:{group:'inline'},paragraph:{group:'block',content:'inline*'},heading:{group:'block',content:'inline*',attrs:{level:{default:1}}},blockquote:{group:'block',content:'block+'},code_block:{group:'block',content:'text*',code:true},bullet_list:{group:'block',content:'list_item+'},ordered_list:{group:'block',content:'list_item+',attrs:{order:{default:1}}},list_item:{content:'paragraph block*',attrs:{checked:{default:null}}},table:{group:'block',content:'table_header_row table_row+'},table_header_row:{content:'table_header+'},table_row:{content:'table_cell+'},table_header:{content:'paragraph'},table_cell:{content:'paragraph'}},marks:{strong:{},emphasis:{},underline:{},strike_through:{},inlineCode:{code:true}}});
const text=s=>schema.text(s), p=s=>schema.node('paragraph',null,text(s));
function viewFor(doc,from,to=from){const view={editable:true,composing:false,state:EditorState.create({doc,selection:TextSelection.create(doc,from,to),plugins:[history()]}),dispatch(tr){view.state=view.state.apply(tr);},focus(){}};return view;}

test('format controls change blocks and marks without losing selected or neighbouring text',async t=>{
  const server=await createServer({server:{middlewareMode:true,watch:null},logLevel:'silent'});
  try {
    const {applyFormat,readFormat}=await server.ssrLoadModule('/src/workspace/editorFormatting.ts');
    await t.test('a heading becomes ordinary text with just a cursor, and undo restores it',()=>{
      const view=viewFor(schema.node('doc',null,[schema.node('heading',{level:2},text('中文标题')),p('保留下面的正文')]),2);
      assert.equal(readFormat(view.state).block,'h2');applyFormat(view,'paragraph');
      assert.equal(view.state.doc.firstChild.type.name,'paragraph');assert.equal(view.state.doc.textContent,'中文标题保留下面的正文');
      undo(view.state,view.dispatch);assert.equal(view.state.doc.firstChild.type.name,'heading');
    });
    await t.test('formatting and clearing only affect the selected text',()=>{
      const view=viewFor(schema.node('doc',null,[p('前文重点后文')]),3,5);
      applyFormat(view,'strong');assert(view.state.doc.rangeHasMark(3,5,schema.marks.strong));assert(!view.state.doc.rangeHasMark(1,3,schema.marks.strong));
      applyFormat(view,'clear');assert(!view.state.doc.rangeHasMark(1,7,schema.marks.strong));assert.equal(view.state.doc.textContent,'前文重点后文');
    });
    await t.test('ordinary text can become a heading, list, quote, and back to text',()=>{
      const view=viewFor(schema.node('doc',null,[p('选中这一段'),p('其他段落')]),2);
      for(const format of ['h1','bullet_list','ordered_list','blockquote','paragraph']){applyFormat(view,format);assert.equal(readFormat(view.state).block,format);assert.equal(view.state.doc.textContent,'选中这一段其他段落');}
    });
    await t.test('removing one list item from a list preserves its neighbours',()=>{
      const item=s=>schema.node('list_item',null,p(s));
      const view=viewFor(schema.node('doc',null,[schema.node('bullet_list',null,[item('第一条'),item('第二条')])]),4);
      applyFormat(view,'paragraph');assert.equal(view.state.doc.child(0).type.name,'paragraph');assert.equal(view.state.doc.child(1).type.name,'bullet_list');assert.equal(view.state.doc.textContent,'第一条第二条');
    });
    await t.test('active heading buttons toggle back to text and clear removes heading and marks',()=>{
      const view=viewFor(schema.node('doc',null,[p('标题文字')]),1,5);
      for(const heading of ['h1','h2','h3']){applyFormat(view,heading);assert.equal(readFormat(view.state).block,heading);applyFormat(view,heading);assert.equal(readFormat(view.state).block,'paragraph');}
      applyFormat(view,'h1');applyFormat(view,'strong');applyFormat(view,'clear');
      assert.equal(readFormat(view.state).block,'paragraph');assert(!readFormat(view.state).strong);assert.equal(view.state.doc.textContent,'标题文字');
    });
    await t.test('underline toggles and survives a document roundtrip',()=>{
      const view=viewFor(schema.node('doc',null,[p('前文重点后文')]),3,5);
      applyFormat(view,'underline');
      assert(readFormat(view.state).underline);
      assert(schema.nodeFromJSON(view.state.doc.toJSON()).rangeHasMark(3,5,schema.marks.underline));
      applyFormat(view,'underline');assert(!readFormat(view.state).underline);
    });
    await t.test('task conversion preserves adjacent items and checkbox state',()=>{
      const item=(s,checked)=>schema.node('list_item',{checked},p(s));
      const view=viewFor(schema.node('doc',null,[schema.node('bullet_list',null,[item('第一条',null),item('第二条',true)])]),3,6);
      applyFormat(view,'task_list');assert.equal(readFormat(view.state).block,'task_list');
      const items=[];view.state.doc.descendants(n=>{if(n.type.name==='list_item')items.push([n.textContent,n.attrs.checked]);});
      assert.deepEqual(items,[['第一条',false],['第二条',true]]);
      applyFormat(view,'task_list');assert.equal(readFormat(view.state).block,'paragraph');
      assert.equal(view.state.doc.textContent,'第一条第二条');
    });
    await t.test('table insertion keeps the selected text and undo removes only the table',()=>{
      const view=viewFor(schema.node('doc',null,[p('保留选中文字'),p('下一段')]),1,7);
      applyFormat(view,'table');assert.equal(view.state.doc.firstChild.type.name,'table');
      assert.equal(view.state.doc.firstChild.childCount,2);assert.equal(view.state.doc.firstChild.firstChild.childCount,2);
      assert.equal(view.state.doc.textContent,'保留选中文字下一段');
      undo(view.state,view.dispatch);assert.equal(view.state.doc.firstChild.type.name,'paragraph');
      assert.equal(view.state.doc.textContent,'保留选中文字下一段');
    });
    await t.test('select-all supports task conversion and table insertion without text loss',()=>{
      const view=viewFor(schema.node('doc',null,[p('第一段'),p('第二段')]),1);
      view.dispatch(view.state.tr.setSelection(new AllSelection(view.state.doc)));
      applyFormat(view,'task_list');assert.equal(readFormat(view.state).block,'task_list');
      view.dispatch(view.state.tr.setSelection(new AllSelection(view.state.doc)));
      applyFormat(view,'table');assert.equal(view.state.doc.firstChild.type.name,'table');
      assert.equal(view.state.doc.textContent,'第一段第二段');
    });
    await t.test('direct checklist Markdown shortcuts work and undo preserves literal input',async()=>{
      const {createTaskInputRule}=await server.ssrLoadModule('/src/workspace/editorExtensions.ts');
      for(const prefix of ['[] ','[ ] ','[x] ']){
        const view=viewFor(schema.node('doc',null,[p(prefix.trimEnd())]),prefix.length);
        const rule=createTaskInputRule();
        const tr=rule.handler(view.state,prefix.match(rule.match),1,prefix.length);
        view.dispatch(tr);assert.equal(readFormat(view.state).block,'task_list');
        assert.equal(view.state.doc.firstChild.firstChild.attrs.checked,prefix==='[x] ');
        undo(view.state,view.dispatch);assert.equal(view.state.doc.textContent,prefix.trimEnd());
      }
    });
    await t.test('Return creates an unchecked task and retains the previous completed item',async()=>{
      const {splitTaskListItem}=await server.ssrLoadModule('/src/workspace/editorExtensions.ts');
      const doc=schema.node('doc',null,[schema.node('bullet_list',null,[schema.node('list_item',{checked:true},p('完成'))])]);
      for(const pos of [4,5]){
        const view=viewFor(doc,pos);assert(splitTaskListItem(view.state,view.dispatch));
        assert.equal(view.state.doc.firstChild.child(0).attrs.checked,true);
        assert.equal(view.state.doc.firstChild.child(1).attrs.checked,false);
        assert.equal(view.state.doc.textContent,'完成');
      }
    });
    await t.test('clear formatting spans ordinary paragraphs, quotes and lists',()=>{
      const item=s=>schema.node('list_item',null,p(s));
      const doc=schema.node('doc',null,[schema.node('heading',{level:2},text('标题')),schema.node('paragraph',null,schema.text('正文',[schema.marks.strong.create()])),schema.node('blockquote',null,p('引用')),schema.node('bullet_list',null,[item('第一条'),item('第二条')])]);
      const view=viewFor(doc,1,doc.content.size-3);
      let changes=0;const dispatch=view.dispatch;view.dispatch=tr=>{if(tr.docChanged)changes++;dispatch(tr);};
      applyFormat(view,'clear');assert.equal(changes,1);
      assert.equal(view.state.doc.textContent,doc.textContent);
      view.state.doc.forEach(node=>assert.equal(node.type.name,'paragraph'));
      assert.equal(view.state.doc.textBetween(view.state.selection.from,view.state.selection.to),doc.textBetween(1,doc.content.size-3));
      undo(view.state,view.dispatch);assert(view.state.doc.eq(doc));
    });
    await t.test('read-only and composing editors reject toolbar mutations',()=>{
      const view=viewFor(schema.node('doc',null,[p('不能改变')]),2);view.editable=false;applyFormat(view,'h1');view.editable=true;view.composing=true;applyFormat(view,'h1');assert.equal(view.state.doc.firstChild.type.name,'paragraph');
    });
  }finally{await server.close();}
});
