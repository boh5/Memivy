import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('changing provider clears the key and connection proof and uses the correct default URL', async t=>{
 const f=workspaceFixture(t,{native:true});
 const {emptyBinding,providerUrls}=f.load('src/workspace/modelTypes.ts');
 const binding={...emptyBinding(),source:'service',base_url:providerUrls.openai_compatible,model:'custom-model',has_key:true,api_key:'private-fixture-key'};
 const models={revision:'r1',llm:binding,embedding:emptyBinding(),voice:emptyBinding(),auto_organize:true};
 let view;
 const props={kind:'llm',models,draft:binding,embedding:null,voice:null,onBusy(){},onSaved(){},onRefresh:async()=>{},setDraft(draft){props.draft=draft;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 const button=text=>f.find(view,n=>n.type==='button'&&f.text(n)===text);
 f.overrides.models_test=async()=>({token:null,binding:{...binding,api_key:undefined},message:'model_test_agent',message_params:{}});
 button('Test connection').props.onClick();await f.settle();
 assert.equal(button('Save settings').props.disabled,false);
 const select=f.find(view,n=>n.type==='select'&&n.props['aria-label']==='Provider');
 select.props.onChange({target:{value:'gemini'}});await f.settle();
 assert.equal(props.draft.provider,'gemini');
 assert.equal(props.draft.base_url,'https://generativelanguage.googleapis.com');
 assert.equal(props.draft.api_key,'');
 assert.equal(props.draft.has_key,false);
 assert.equal(button('Save settings').props.disabled,false);
 assert.equal(f.calls.filter(c=>c.name==='models_apply').length,0);
});

for(const configured of [false,true])test(`assistant saves without a connection test when configured=${configured}`,async t=>{
 const f=workspaceFixture(t,{native:true});
 const {emptyBinding}=f.load('src/workspace/modelTypes.ts');
 const binding={...emptyBinding(),source:'service',base_url:'https://example.com/v1',model:'new-model',api_key:'fixture-key'};
 const models={revision:'r1',llm:configured?{...binding,model:'old-model'}:null,embedding:emptyBinding(),voice:emptyBinding(),auto_organize:true};
 let view,saved;
 const props={kind:'llm',models,draft:binding,embedding:null,voice:null,onBusy(){},onSaved(value){saved=value},onRefresh:async()=>{},setDraft(draft){props.draft=draft;f.render(view,props)}};
 view=f.mount(f.load('src/workspace/ModelCapability.tsx').default,props);await f.settle();
 const button=text=>f.find(view,n=>n.type==='button'&&f.text(n)===text);
 assert(!f.text(view.tree).includes('Disable AI assistant'));
 assert.equal(button('Save settings').props.disabled,false);
 f.overrides.models_apply=async args=>{assert.equal(args.token,null);assert.equal(args.binding.model,'new-model');return {...models,llm:binding}};
 button('Save settings').props.onClick();await f.settle();
 assert.equal(saved.llm.model,'new-model');
 assert.equal(f.calls.filter(c=>c.name==='models_test').length,0);
 f.overrides.models_test=async()=>{throw new Error('model_test_failed')};
 button('Test connection').props.onClick();await f.settle();
 assert.equal(button('Save settings').props.disabled,false);
});
