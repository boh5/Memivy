import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

const key=(extra={})=>({key:'m',code:'KeyM',altKey:true,nativeEvent:{isComposing:false},preventDefault(){},stopPropagation(){},...extra});
test('shared recorder focuses, ignores composition/repeat, and saves one complete combination immediately',async t=>{
 const f=workspaceFixture(t),changes=[];
 const view=f.mount(f.load('src/workspace/ShortcutSetting.tsx').default,{label:"Shortcut",value:'Alt+KeyR',onChange:value=>changes.push(value)});
 const recorder=()=>f.find(view,n=>n.props.className?.includes('shortcut-recorder'));
 let focused=false;recorder().props.onClick({currentTarget:{focus(){focused=true}}});await f.settle();assert(focused);
 for(const event of [key({key:'Alt'}),key({repeat:true}),key({nativeEvent:{isComposing:true}})])recorder().props.onKeyDown(event);
 assert.deepEqual(changes,[]);
 recorder().props.onKeyDown(key());await f.settle();assert.deepEqual(changes,['Alt+KeyM']);
 // Until its owner accepts a save, the control keeps the previous binding.
 assert.equal(f.text(recorder()),'⌥R');
 f.find(view,n=>n.props.className?.includes('shortcut-remove')).props.onClick();assert.deepEqual(changes,['Alt+KeyM','']);
});
for(const exit of ['Escape','Tab','blur'])test(`recorder ${exit} exits without changing the binding`,async t=>{
 const f=workspaceFixture(t),changes=[];
 const view=f.mount(f.load('src/workspace/ShortcutSetting.tsx').default,{label:"Shortcut",value:'Alt+KeyM',onChange:value=>changes.push(value)});
 const recorder=()=>f.find(view,n=>n.props.className?.includes('shortcut-recorder'));
 recorder().props.onClick({currentTarget:{focus(){}}});await f.settle();
 if(exit==='blur')recorder().props.onBlur();else recorder().props.onKeyDown(key({key:exit,preventDefault(){assert.notEqual(exit,'Tab')}}));
 await f.settle();assert.deepEqual(changes,[]);assert.equal(f.text(recorder()),'⌥M');
});

test('voice settings route the shared recorder to voice persistence and retain the last value on failure',async t=>{
 const f=workspaceFixture(t,{native:true}),Shortcut=f.load('src/workspace/ShortcutSetting.tsx').default;
 let value='Alt+KeyR',fail=false;
 f.overrides.voice_status=async()=>({shortcut:value,state:'unloaded'});
 f.overrides.voice_control=async args=>{assert.equal(args.action,'shortcut');if(fail)throw 'save failed';value=args.value};
 const view=f.mount(f.load('src/workspace/VoiceSettings.tsx').default,{shortcutOnly:true});await f.settle();
 const control=()=>f.find(view,n=>n.type===Shortcut).props;
 control().onChange('Alt+KeyV');await f.settle();assert.equal(control().value,'Alt+KeyV');
 fail=true;control().onChange('');await f.settle();assert.equal(control().value,'Alt+KeyV');
 assert(f.nodes(view.tree).some(n=>n.props?.text==='save failed'));
});
