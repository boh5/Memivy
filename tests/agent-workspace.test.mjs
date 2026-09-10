import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('connection capabilities explain fallback and are read from the saved configuration',async t=>{
 const f=workspaceFixture(t);let capabilities=null;
 f.overrides.workspace_settings=async()=>({configured:true,base_url:'http://127.0.0.1:1/v1',model:'QA',has_key:false,disable_reasoning:false,max_output_tokens:null,output_token_parameter:'max_tokens',model_capabilities:capabilities});
 f.overrides.workspace_configure=async()=>{};
 f.overrides.workspace_test_model=async()=>capabilities={structured_json:true,single_tool:true,multi_turn:false};
 const view=f.mount(f.load('src/workspace/Settings.tsx').default,{onClose(){},onRestore(){},onChanged(){}});await f.settle();
 assert(f.text(view.tree).includes('尚未验证工具能力'));
 f.find(view,n=>n.type==='button'&&f.text(n)==='保存并测试连接').props.onClick();await f.settle();
 assert(f.text(view.tree).includes('支持单次工具，问答和整理使用基本流程'));assert(!f.text(view.tree).includes('已启用增强'));
});

test('a citation shows disjoint source windows separately without invented intervening text',async t=>{
 const f=workspaceFixture(t);const source={kind:'version',id:'v-a'};
 f.messages(()=>[{id:'answer-a',role:'assistant',status:'complete',text:'grounded',answer:{recollections:[{text:'grounded',sources:[source]}],ideas:'',conclusion:''},citations:[{source,available:true}],created_at:1}]);
 f.overrides.discussion_source=async()=>({source,title:'QA',text:'FIRST',start:0,truncated:true,current:true,recorded_at:1,additional_spans:[{start:6000,text:'LAST',truncated:true}]});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,{topic:f.topic,revision:1,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='查看依据').props.onClick();await f.settle();
 const preview=f.find(view,n=>typeof n.type==='function'&&n.type.name==='SourcePreview');const modal=f.mount(preview.type,preview.props);await f.settle();
 assert(f.text(modal.tree).includes('FIRST'));assert(f.text(modal.tree).includes('LAST'));assert(f.text(modal.tree).includes('另一处引用片段'));
 assert.equal(f.calls.filter(c=>c.name==='discussion_source').length,1);
});
