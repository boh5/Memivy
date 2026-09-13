import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
const enter={key:'Enter',metaKey:true,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,keyCode:13,nativeEvent:{isComposing:false},preventDefault(){}};
const props=f=>({topic:f.topic,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
const response=extra=>({seq:2,id:'answer',turn_id:'logical-input',role:'assistant',status:'complete',text:'已经考虑预算。',citations:[],followups:[],receipts:[],record_only:false,progress:null,...extra});

test('one composer accepts records, questions and mixed expressions without classifying or rewriting them',async t=>{
 const f=workspaceFixture(t),sent=[];
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async input=>{sent.push(input);}});await f.settle();
 for(const text of ['想到一个新产品。','我之前为什么暂停？','我考虑先收费，你觉得呢？']){
  const input=()=>f.find(view,n=>n.type==='textarea');input().props.onChange({target:{value:text}});await f.settle();
  input().props.onKeyDown({...enter,metaKey:false});await f.settle();assert.equal(sent.length,['想到一个新产品。','我之前为什么暂停？','我考虑先收费，你觉得呢？'].indexOf(text));
  input().props.onKeyDown(enter);input().props.onKeyDown(enter);await f.settle();assert.equal(sent.at(-1).text,text);assert.equal(input().props.value,'');
 }
 assert.equal(sent.length,3);assert.equal(new Set(sent.map(v=>v.id)).size,3);
});
test('transport rejection preserves the submitted expression and request id for retry',async t=>{
 const f=workspaceFixture(t),sent=[];
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async input=>{sent.push(input);throw '断线';}});await f.settle();
 const input=()=>f.find(view,n=>n.type==='textarea');input().props.onChange({target:{value:'新想法与原来的数字 5000'}});await f.settle();
 for(let i=0;i<2;i++){input().props.onKeyDown(enter);await f.settle();}
 assert.equal(sent[0].id,sent[1].id);assert.equal(f.db.get('input').body,'新想法与原来的数字 5000');
});
test('the top composer keeps conversation focus after an acknowledged continuation',async t=>{
 const f=workspaceFixture(t);const source={kind:'version',id:'v-a'};
 f.db.set('discussion:topic-a',{key:'discussion:topic-a',request_id:'request-one',title:'',body:'继续讨论',context:[source],expected_version:null});
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{draftKey:'discussion:topic-a',presentation:'query',onSubmit:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='textarea').props.onKeyDown(enter);await f.settle();
 assert.equal(f.db.get('discussion:topic-a').body,'');assert.deepEqual(f.db.get('discussion:topic-a').context,[source]);
 assert.notEqual(f.db.get('discussion:topic-a').request_id,'request-one');
});
test('a follow-up sends its exact visible text once and preserves the current draft',async t=>{
 const f=workspaceFixture(t);let finish;
 f.messages(()=>[response({followups:['这与之前的预算冲突吗？','我们还缺哪些事实？']})]);
 f.overrides.discussion_submit=()=>new Promise(resolve=>{finish=resolve;});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:'我自己还在写的内容'}});await f.settle();
 const follow=f.find(view,n=>n.type==='button'&&f.text(n)==='这与之前的预算冲突吗？');follow.props.onClick();follow.props.onClick();await f.settle();
 const calls=f.calls.filter(c=>c.name==='discussion_submit');assert.equal(calls.length,1);assert.equal(calls[0].args.text,'这与之前的预算冲突吗？');assert.equal(calls[0].args.topicId,f.topic.id);
 finish(f.topic);await f.settle();assert.equal(f.db.get('discussion:topic-a').body,'我自己还在写的内容');
});
test('retry resumes the logical input without rewriting or consuming the composer draft',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({status:'failed'})]);f.overrides.discussion_retry=async()=>f.topic;
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:'另一个未发送的想法'}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='重试').props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_retry').args.inputId,'logical-input');
 assert.equal(f.db.get('discussion:topic-a').body,'另一个未发送的想法');
});
test('a failed answer preserves its committed write through viewing changes, retry and undo',async t=>{
 const f=workspaceFixture(t,{native:true});
 const original='周末预算改为 80 元，另一个计划稍后继续。',draft='还没有发出的下一条想法';
 const receipt={request_id:'committed-budget-write',memory_id:'budget',status:'applied',before_version:'budget-before',after_version:'budget-after'};
 const user={seq:1,id:'original-expression',turn_id:'logical-input',role:'user',status:'complete',text:original,citations:[],followups:[],receipts:[],record_only:false,progress:null};
 let answer=response({status:'failed',error_code:'network',text:'已把周末预算更新为 80 元。',receipts:[receipt]});
 f.messages(()=>[user,answer]);
 f.db.set('discussion:topic-a',{key:'discussion:topic-a',request_id:'unsent-next-input',title:'',body:draft,context:[],expected_version:null});
 f.overrides.discussion_changes=async()=>[{receipt,before:{id:'budget-before',memory_id:'budget',title:'周末预算',body:'预算不超过 100 元。'},after:{id:'budget-after',memory_id:'budget',title:'周末预算',body:'预算不超过 80 元。'}}];
 f.overrides.discussion_retry=async()=>{answer={...answer,status:'processing',error_code:null};return f.topic;};
 f.overrides.discussion_undo=async()=>{answer={...answer,status:'cancelled',error_code:'changes_undone',receipts:[{...receipt,status:'undone'}]};return {receipt:{status:'applied'},conflicts:[]};};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const Changes=f.load('src/workspace/MemoryChanges.tsx').default;
 const group=()=>f.find(view,n=>n.type===Changes);
 const changes=f.mount(Changes,group().props);await f.settle();
 const statusTexts=surface=>f.nodes(surface.tree).filter(n=>n.props?.role==='status').map(n=>f.text(n));
 assert.deepEqual(statusTexts(view),['无法连接模型服务，请重试。']);
 assert.deepEqual(statusTexts(changes),['已更新 1 条记忆查看变化撤销本次']);
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='已把周末预算更新为 80 元。'));
 assert.equal(f.nodes(view.tree).filter(n=>n.type==='article'&&n.props.className==='discussion-message user').length,1);
 assert(f.text(view.tree).includes(original));
 f.find(changes,n=>n.type==='button'&&f.text(n)==='查看变化').props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_changes').args.inputId,'logical-input');
 const comparison=f.find(changes,n=>n.props?.title==='记忆修改');
 assert.deepEqual(f.nodes(comparison).filter(n=>n.type==='Markdown').map(n=>n.props.text),['预算不超过 100 元。','预算不超过 80 元。']);
 comparison.props.onClose();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==='重试').props.onClick();await f.settle();f.render(changes,group().props);
 assert.equal(f.calls.filter(c=>c.name==='discussion_retry').length,1);
 assert.equal(f.calls.find(c=>c.name==='discussion_retry').args.inputId,'logical-input');
 assert.equal(f.calls.filter(c=>c.name==='discussion_submit').length,0);
 assert.deepEqual(statusTexts(changes),['已更新 1 条记忆查看变化撤销本次']);
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==='重试'));
 assert.equal(f.db.get('discussion:topic-a').body,draft);
 answer={...answer,status:'failed',error_code:'network'};
 f.emit('discussion-updated',{topicId:'topic-a',inputId:'logical-input'});await f.settle();f.render(changes,group().props);
 assert.deepEqual(statusTexts(view),['无法连接模型服务，请重试。']);
 assert.deepEqual(statusTexts(changes),['已更新 1 条记忆查看变化撤销本次']);
 f.find(changes,n=>n.type==='button'&&f.text(n)==='撤销本次').props.onClick();await f.settle();f.render(changes,group().props);
 assert.equal(f.calls.filter(c=>c.name==='discussion_undo').length,1);
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'logical-input');
 assert.deepEqual(statusTexts(changes),['本次修改已撤销查看变化']);
 assert(f.text(view.tree).includes(original));
 assert.equal(f.nodes(view.tree).filter(n=>n.type==='article'&&n.props.className==='discussion-message user').length,1);
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==='重试'));
 assert.equal(f.db.get('discussion:topic-a').body,draft);
});
test('a follow-up with an unknown acknowledgement reuses its request id',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({followups:['再看一下预算。']})]);
 f.overrides.discussion_submit=async()=>{throw '断线';};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 for(let i=0;i<2;i++){f.find(view,n=>n.type==='button'&&f.text(n)==='再看一下预算。').props.onClick();await f.settle();}
 const requests=f.calls.filter(c=>c.name==='discussion_submit');assert.equal(requests.length,2);assert.equal(requests[0].args.id,requests[1].args.id);
});
test('an undone input retains its text and has no action that replays the changes',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({status:'cancelled',error_code:'changes_undone'})]);
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 assert(f.text(view.tree).includes('本次修改已撤销'));
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='已经考虑预算。'));
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==='重试'));
});
test('streamed Markdown stays visible during execution and after cancellation',async t=>{
 const f=workspaceFixture(t,{native:true});let current=response({status:'processing',text:'先看 **预算**',progress:'正在查看相关记忆'});
 f.messages(()=>[current]);f.overrides.discussion_cancel=async()=>{current={...current,status:'cancelled'};};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='先看 **预算**'));
 current={...current,text:'先看 **预算**，再看时间。'};f.emit('discussion-updated',{topicId:'topic-a',inputId:'logical-input'});await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text===current.text));
 const composer=f.composer(view);composer.props.onCancel();await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==='先看 **预算**，再看时间。'));
 assert(f.text(view.tree).includes('已停止'));
});
test('group undo shows conflicts without claiming success or changing receipts',async t=>{
 const f=workspaceFixture(t);const receipts=['a','b'].map(id=>({request_id:id,memory_id:id,status:'applied'}));
 f.messages(()=>[response({receipts})]);f.overrides.discussion_undo=async()=>({receipt:null,conflicts:['b']});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const node=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges'),group=f.mount(node.type,node.props);await f.settle();
 f.find(group,n=>n.type==='button'&&f.text(n)==='撤销本次').props.onClick();await f.settle();
 assert(f.text(group.tree).includes('本次未撤销'));assert(f.text(group.tree).includes('已更新 2 条记忆'));
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'logical-input');
});
test('memory sources expose a whole input receipt group after its conversation is deleted',async t=>{
 const f=workspaceFixture(t);let groups=[{input_id:'deleted-conversation-input',receipts:[{request_id:'first-write',memory_id:'a',status:'applied'},{request_id:'second-write',memory_id:'b',status:'applied'}]}];
 f.overrides.library_agent_changes=async()=>groups;
 f.overrides.discussion_undo=async()=>{groups=[{...groups[0],receipts:groups[0].receipts.map(r=>({...r,status:'undone'}))}];return {receipt:{status:'applied'},conflicts:[]};};
 const view=f.mount(f.load('src/workspace/MemoryChanges.tsx').MemoryChangeHistory,{memoryId:'a',onOpenRecord(){},onRefresh(){}});await f.settle();
 assert.equal(f.calls.find(c=>c.name==='library_agent_changes').args.memoryId,'a');
 const group=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges'),receipt=f.mount(group.type,group.props);await f.settle();
 assert(f.text(receipt.tree).includes('已更新 2 条记忆'));
 f.find(receipt,n=>n.type==='button'&&f.text(n)==='撤销本次').props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'deleted-conversation-input');
 assert(f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges').props.receipts.every(r=>r.status==='undone'));
});
test('source provenance opens a live conversation and labels a deleted one without hiding the original text',async t=>{
 for(const available of [true,false]) {
  const f=workspaceFixture(t);let opened;
  f.overrides.library_detail=async()=>({key:f.keyA,state:'active',title:'散步计划',body:'计划',current:{id:'v-a',capture_ids:['raw'],created_at:1,actor:'ai'},history:[],sources:[{id:'raw',conversation_available:available,capture:{id:'raw',text:'原话：预算不超过80元。',created_at:1,origin:{kind:'discussion',conversation_id:'original-topic',message_id:'user-message',app:'Memivy'}}}]});
  const view=f.mount(f.load('src/workspace/MemoryDetail.tsx').default,{record:f.keyA,initialReceipt:null,query:'',onChanged(){},onBack(){},onDiscuss:async()=>{},onOpenDiscussion:async id=>{opened=id;}});await f.settle();
  f.find(view,n=>n.type==='button'&&n.props.role==='tab'&&f.text(n).startsWith('输入归档与来源')).props.onClick();await f.settle();
  assert(f.nodes(view.tree).some(n=>n.props?.text==='原话：预算不超过80元。'));
  if(available){f.find(view,n=>n.type==='button'&&f.text(n)==='查看原会话').props.onClick();await f.settle();assert.equal(opened,'original-topic');}
  else assert(f.text(view.tree).includes('原会话已不可用'));
  f.unmount(view);
 }
});
test('memory focus can be added and removed without changing the unsent expression',async t=>{
 const f=workspaceFixture(t),view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='textarea').props.onChange({target:{value:'保持这段草稿'}});await f.settle();
 f.find(view,n=>n.type==='button'&&n.props['aria-label']==='添加记忆').props.onClick();await f.settle();
 const node=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryPicker'),picker=f.mount(node.type,node.props);await new Promise(r=>setTimeout(r,150));await f.settle();
 f.find(picker,n=>n.type==='button'&&f.text(n)==='atest').props.onClick();await f.settle();
 assert.deepEqual(f.db.get('input').context,[{kind:'version',id:'v-a'}]);
 f.find(view,n=>n.type==='button'&&n.props['aria-label']==='移除这条讨论依据').props.onClick();await f.settle();
 assert.equal(f.db.get('input').context.length,0);assert.equal(f.db.get('input').body,'保持这段草稿');
});

test('reading earlier replies exposes an icon jump without moving the reader or consuming the draft', async t=>{
 const f=workspaceFixture(t);let answer=response({status:'processing',text:'第一段'});
 f.messages(()=>[answer]);
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();
 f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:'还在写的想法'}});await f.settle();
 const list=f.find(view,n=>n.props.className==='discussion-messages');
 list.props.onScroll({currentTarget:{scrollHeight:1800,scrollTop:300,clientHeight:500}});await f.settle();
 view.dom.scrolls=0;answer={...answer,text:'第一段，新的第二段'};
 f.render(view,{...props(f),revision:1});await f.settle();
 assert.equal(view.dom.scrolls,0);
 const jump=f.find(view,n=>n.props.className==='discussion-jump-button');
 assert.equal(jump.props.icon,'arrow');assert.equal(jump.props.children,undefined);
 jump.props.onClick();await f.settle();
 assert.equal(view.dom.scrolls,1);
 assert(!f.nodes(view.tree).some(n=>n.props.className==='discussion-jump-button'));
 assert.equal(f.db.get('discussion:topic-a').body,'还在写的想法');
 assert(!f.calls.some(c=>c.name==='discussion_submit'));
});
