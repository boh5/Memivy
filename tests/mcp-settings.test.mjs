import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';

test('MCP settings only report an acknowledged switch; failed changes preserve state', async t => {
  const f=workspaceFixture(t,{native:true});
  f.overrides.mcp_settings=async()=>({enabled:false,executable_available:true,configuration:'{}'});
  f.overrides.mcp_set_enabled=async()=>{throw "Database busy";};
  const view=f.mount(f.load('src/workspace/McpSettings.tsx').default);await f.settle();
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  assert.equal(f.find(view,n=>n.type==='input').props.checked,false);
  assert(f.find(view,n=>n.props?.text==="Database busy"));
  f.overrides.mcp_set_enabled=async()=>{f.overrides.mcp_settings=async()=>({enabled:true,executable_available:true,configuration:'{}'});};
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
});

test('local diagnostic does not claim an Agent connection and switch changes invalidate it', async t => {
  const f=workspaceFixture(t,{native:true});
  f.overrides.mcp_settings=async()=>({enabled:false,executable_available:true,configuration:'{}'});
  f.overrides.mcp_diagnose=async()=>({server_version:'0.1.0',protocol_version:'2025-11-25',tools:['memory_capture','memory_search'],enabled:false,scope:'local_stdio_only'});
  f.overrides.mcp_set_enabled=async()=>{};
  const view=f.mount(f.load('src/workspace/McpSettings.tsx').default);await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==="Check connection").props.onClick();await f.settle();
  assert(f.text(view.tree).includes("Try saving and searching from your other AI app to check its connection"));
  assert(f.text(view.tree).includes("Saving and searching memories are unavailable"));
  const diagnostic=f.find(view,n=>n.props.role==='status'&&f.text(n).includes("MCP check passed"));
  const diagnosticText=f.text(diagnostic);
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  assert(!f.nodes(view.tree).some(n=>n.props.role==='status'&&f.text(n)===diagnosticText));
});

test('the settings modal cannot close while the MCP section has an unfinished request', async t => {
  const f=workspaceFixture(t,{native:true,timers:{setTimeout,clearTimeout,setInterval:()=>1,clearInterval:()=>{}}});let closed=0;
  f.overrides.workspace_settings=async()=>({configured:false,base_url:'',model:'',has_key:false,disable_reasoning:false});
  const view=f.mount(f.load('src/workspace/Settings.tsx').default,{onClose(){closed++;},onChanged(){}});await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==="External access").props.onClick();await f.settle();
  const section=f.find(view,n=>n.type?.name==='McpSettings');
  assert.equal(typeof section.props.onBusyChange,'function');
  section.props.onBusyChange(true);await f.settle();
  view.tree.props.onClose();assert.equal(closed,0);
  section.props.onBusyChange(false);await f.settle();
  view.tree.props.onClose();assert.equal(closed,1);
});

test('a write error reconciles a switch that already reached disk and releases the modal', async t => {
  const f=workspaceFixture(t,{native:true}),busy=[];let enabled=false;
  f.overrides.mcp_settings=async()=>({enabled,executable_available:true,configuration:'{}'});
  f.overrides.mcp_set_enabled=async()=>{enabled=true;throw "Synchronization failed";};
  const view=f.mount(f.load('src/workspace/McpSettings.tsx').default,{onBusyChange:v=>busy.push(v)});await f.settle();
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
  assert(f.find(view,n=>n.props?.text==="Synchronization failed"));
  assert.deepEqual(busy,[true,false]);
});

test('an old focus read cannot replace a newly acknowledged MCP switch', async t => {
  const f=workspaceFixture(t,{native:true});let enabled=false,finish;
  f.overrides.mcp_settings=async()=>({enabled,executable_available:true,configuration:'{}'});
  f.overrides.mcp_set_enabled=async()=>{enabled=true;};
  const view=f.mount(f.load('src/workspace/McpSettings.tsx').default);await f.settle();
  const current=f.overrides.mcp_settings;
  f.overrides.mcp_settings=()=>new Promise(resolve=>finish=resolve);
  f.focus();await f.settle();f.overrides.mcp_settings=current;
  f.find(view,n=>n.type==='input').props.onChange({target:{checked:true}});await f.settle();
  finish({enabled:false,executable_available:true,configuration:'{}'});await f.settle();
  assert.equal(f.find(view,n=>n.type==='input').props.checked,true);
});

for (const language of ['en', 'zh-CN']) {
  test(`MCP setup copies the visible agent prompt with the exact library configuration in ${language}`, async t => {
    const copied = [];
    const f = workspaceFixture(t, {native:true, clipboard:{writeText:async value=>{copied.push(value);}}});
    const server = {
      command:'/Users/test/Applications/Memory 测试.app/Contents/MacOS/memivy-mcp',
      args:[],
      env:{MEMIVY_DATA_DIR:'/private/tmp/memivy test/记忆库'}
    };
    const configuration = JSON.stringify({mcpServers:{memivy:server}}, null, 2);
    f.overrides.mcp_settings = async()=>({enabled:true,executable_available:true,configuration});
    const view = f.mount(f.load('src/workspace/McpSettings.tsx').default);
    await f.settle(); await f.language(language);
    const prompt = f.find(view, n=>n.type==='textarea'&&n.props.className==='mcp-installation-prompt');
    assert.equal(prompt.props.readOnly, true);
    const instructions = f.find(view, n=>n.props.id===prompt.props['aria-describedby']);
    const copy = f.find(view, n=>n.type==='button'&&f.text(n)===(language==='en'?'Copy setup prompt':'复制安装提示词'));
    const elements = f.nodes(view.tree);
    assert(elements.indexOf(instructions)<elements.indexOf(prompt));
    assert(elements.indexOf(prompt)<elements.indexOf(copy));
    copy.props.onClick(); await f.settle();
    assert.deepEqual(copied, [prompt.props.value]);
    const embedded = copied[0].match(/```json\n([\s\S]*?)\n```/);
    assert(embedded, 'The prompt must include usable server configuration');
    assert.deepEqual(JSON.parse(embedded[1]), {mcpServers:{memivy:server}});
    assert.match(copied[0], /memory_capture/);
    assert.match(copied[0], /memory_search/);
    assert.match(copied[0], /Preserve all other servers and settings/);
    assert.match(copied[0], /Only report success after verifying/);
    assert(f.text(view.tree).includes(language==='en'?'Copied. Send the prompt to your agent':'已复制。发给你的 Agent'));
    assert(!f.calls.some(c=>c.name==='mcp_diagnose'||c.name==='mcp_set_enabled'));
  });
}

test('MCP setup requires enabled access and a server configuration before copying', async t => {
  const f = workspaceFixture(t, {native:true});
  let state = {enabled:false,executable_available:true,configuration:'{}'};
  f.overrides.mcp_settings = async()=>state;
  const view = f.mount(f.load('src/workspace/McpSettings.tsx').default); await f.settle();
  const copy = ()=>f.find(view, n=>n.type==='button'&&f.text(n)==='Copy setup prompt');
  assert.equal(copy().props.disabled, true);
  state = {...state,enabled:true}; f.focus(); await f.settle();
  assert.equal(copy().props.disabled, false);
  state = {...state,executable_available:false,configuration:null}; f.focus(); await f.settle();
  assert.equal(copy().props.disabled, true);
  assert(!f.nodes(view.tree).some(n=>n.props.className==='mcp-installation-prompt'));
});

test('a clipboard failure does not report a successful MCP setup copy and releases the section', async t => {
  const busy = [];
  const f = workspaceFixture(t, {native:true,clipboard:{writeText:async()=>{throw 'Clipboard unavailable';}}});
  f.overrides.mcp_settings = async()=>({enabled:true,executable_available:true,configuration:'{}'});
  const view = f.mount(f.load('src/workspace/McpSettings.tsx').default, {onBusyChange:value=>busy.push(value)});
  await f.settle();
  f.find(view, n=>n.type==='button'&&f.text(n)==='Copy setup prompt').props.onClick(); await f.settle();
  assert(f.find(view, n=>n.props.text==='Clipboard unavailable'));
  assert(!f.text(view.tree).includes('Copied.'));
  assert.deepEqual(busy, [true,false]);
});
