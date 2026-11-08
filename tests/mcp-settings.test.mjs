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
  f.find(view,n=>n.type==='button'&&f.text(n)==="Check local MCP").props.onClick();await f.settle();
  assert(f.text(view.tree).includes("ask your other AI app whether it can see Memivy’s memory tools"));
  assert(f.text(view.tree).includes("Saving and searching memories are unavailable"));
  const diagnostic=f.find(view,n=>n.props.role==='status'&&f.text(n).includes("Local MCP check passed"));
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
  test(`MCP setup copies the server-provided prompt and reports feedback at its button in ${language}`, async t => {
    const f = workspaceFixture(t, {native:true});
    const setup_prompt = 'Configure the bundled server using its current settings.';
    let finishCopy;
    f.overrides.mcp_settings = async()=>({enabled:true,executable_available:true,configuration:'{}',setup_prompt});
    f.overrides.mcp_copy_setup = () => new Promise(resolve => { finishCopy = resolve; });
    const view = f.mount(f.load('src/workspace/McpSettings.tsx').default);
    await f.settle(); await f.language(language);
    const prompt = f.find(view, n=>n.type==='textarea'&&n.props.className==='mcp-installation-prompt');
    assert.equal(prompt.props.readOnly, true);
    const instructions = f.find(view, n=>n.props.id===prompt.props['aria-describedby']);
    const copy = f.find(view, n=>n.type==='button'&&f.text(n)===(language==='en'?'Copy setup prompt':'复制安装提示词'));
    const elements = f.nodes(view.tree);
    assert.equal(prompt.props.value, setup_prompt);
    assert(elements.indexOf(instructions)<elements.indexOf(copy));
    assert(elements.indexOf(copy)<elements.indexOf(prompt));
    copy.props.onClick(); await f.settle();
    assert.deepEqual(structuredClone(f.calls.find(c=>c.name==='mcp_copy_setup')?.args), {kind:'prompt'});
    f.find(view,n=>n.type==='button'&&f.text(n)===(language==='en'?'Copying…':'复制中…'));
    f.find(view,n=>n.type==='button'&&f.text(n)===(language==='en'?'Check local MCP':'检查本机 MCP'));
    finishCopy(); await f.settle();
    f.find(view,n=>n.type==='button'&&f.text(n)===(language==='en'?'Copied':'已复制'));
    assert(f.text(view.tree).includes(language==='en'?'Copied. Send the prompt to your agent':'已复制。把提示词发给你的 AI 助手'));
    assert(!f.calls.some(c=>c.name==='mcp_diagnose'||c.name==='mcp_set_enabled'));
  });
}

test('MCP setup requires enabled access and a server configuration before copying', async t => {
  const f = workspaceFixture(t, {native:true});
  let state = {enabled:false,executable_available:true,configuration:'{}',setup_prompt:'Setup instructions'};
  f.overrides.mcp_settings = async()=>state;
  const view = f.mount(f.load('src/workspace/McpSettings.tsx').default); await f.settle();
  const copy = ()=>f.find(view, n=>n.type==='button'&&f.text(n)==='Copy setup prompt');
  assert.equal(copy().props.disabled, true);
  state = {...state,enabled:true}; f.focus(); await f.settle();
  assert.equal(copy().props.disabled, false);
  state = {...state,executable_available:false,configuration:null,setup_prompt:null}; f.focus(); await f.settle();
  assert.equal(copy().props.disabled, true);
  assert(!f.nodes(view.tree).some(n=>n.props.className==='mcp-installation-prompt'));
});

test('MCP copy failure persists through focus refresh and offers the original text for manual copy', async t => {
  const busy = [];
  const f = workspaceFixture(t, {native:true});
  f.overrides.mcp_settings = async()=>({enabled:true,executable_available:true,configuration:'{}',setup_prompt:'Setup instructions'});
  f.overrides.mcp_copy_setup = async()=>{throw 'Clipboard unavailable';};
  const view = f.mount(f.load('src/workspace/McpSettings.tsx').default, {onBusyChange:value=>busy.push(value)});
  await f.settle();
  f.find(view, n=>n.type==='button'&&f.text(n)==='Copy setup prompt').props.onClick(); await f.settle();
  f.focus(); await f.settle();
  assert(f.find(view, n=>n.props.text?.includes('press ⌘C to copy it manually')));
  assert(!f.text(view.tree).includes('Copied.'));
  assert.deepEqual(busy, [true,false]);
  const textarea = f.find(view,n=>n.type==='textarea'&&n.props.className==='mcp-installation-prompt');
  const preview = f.find(view,n=>n.type==='details'&&f.nodes(n).includes(textarea));
  const actions = [];
  textarea.props.ref.current = {focus(){actions.push('focus');},select(){actions.push('select');}};
  f.find(view,n=>n.type==='button'&&f.text(n)==='Select text').props.onClick();
  assert.equal(preview.props.ref.current.open,true);
  assert.deepEqual(actions,['focus','select']);
});

test('manual MCP configuration copies independently from the agent prompt', async t => {
  const f = workspaceFixture(t, {native:true});
  f.overrides.mcp_settings = async()=>({enabled:false,executable_available:true,configuration:'{}',setup_prompt:'Setup instructions'});
  f.overrides.mcp_copy_setup = async()=>{};
  const view = f.mount(f.load('src/workspace/McpSettings.tsx').default);
  await f.settle();
  f.find(view,n=>n.type==='button'&&f.text(n)==='Copy configuration').props.onClick(); await f.settle();
  assert.deepEqual(structuredClone(f.calls.find(c=>c.name==='mcp_copy_setup')?.args), {kind:'configuration'});
  assert(f.text(view.tree).includes('Configuration copied.'));
  assert(f.find(view,n=>n.type==='button'&&f.text(n)==='Copy setup prompt').props.disabled);
});
