import test from 'node:test';
import assert from 'node:assert/strict';
import {workspaceFixture} from './helpers/workspace.mjs';
const enter={key:'Enter',metaKey:true,ctrlKey:false,altKey:false,shiftKey:false,repeat:false,keyCode:13,nativeEvent:{isComposing:false},preventDefault(){}};
const props=f=>({topic:f.topic,configured:true,onSettings(){},onRefresh(){},onOpenRecord(){}});
const response=extra=>({seq:2,id:'answer',turn_id:'logical-input',role:'assistant',status:'complete',text:"The budget has been considered.",citations:[],followups:[],receipts:[],record_only:false,progress:null,...extra});

test('one composer accepts records, questions and mixed expressions without classifying or rewriting them',async t=>{
 const f=workspaceFixture(t),sent=[];
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async input=>{sent.push(input);}});await f.settle();
 for(const text of ["I have a new product idea.","Why did I pause earlier?","I am considering charging first. What do you think?"]){
  const input=()=>f.find(view,n=>n.type==='textarea');input().props.onChange({target:{value:text}});await f.settle();
  input().props.onKeyDown({...enter,metaKey:false});await f.settle();assert.equal(sent.length,["I have a new product idea.","Why did I pause earlier?","I am considering charging first. What do you think?"].indexOf(text));
  input().props.onKeyDown(enter);input().props.onKeyDown(enter);await f.settle();assert.equal(sent.at(-1).text,text);assert.equal(input().props.value,'');
 }
 assert.equal(sent.length,3);assert.equal(new Set(sent.map(v=>v.id)).size,3);
});
test('transport rejection preserves the submitted expression and request id for retry',async t=>{
 const f=workspaceFixture(t),sent=[];
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async input=>{sent.push(input);throw "Disconnected";}});await f.settle();
 const input=()=>f.find(view,n=>n.type==='textarea');input().props.onChange({target:{value:"A new idea and the original number 5000"}});await f.settle();
 for(let i=0;i<2;i++){input().props.onKeyDown(enter);await f.settle();}
 assert.equal(sent[0].id,sent[1].id);assert.equal(f.db.get('input').body,"A new idea and the original number 5000");
});
test('the top composer keeps conversation focus after an acknowledged continuation',async t=>{
 const f=workspaceFixture(t);const source={kind:'version',id:'v-a'};
 f.db.set('discussion:topic-a',{key:'discussion:topic-a',request_id:'request-one',title:'',body:"Continue discussion",context:[source],expected_version:null});
 const view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{draftKey:'discussion:topic-a',presentation:'query',onSubmit:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='textarea').props.onKeyDown(enter);await f.settle();
 assert.equal(f.db.get('discussion:topic-a').body,'');assert.deepEqual(f.db.get('discussion:topic-a').context,[source]);
 assert.notEqual(f.db.get('discussion:topic-a').request_id,'request-one');
});
test('a follow-up sends its exact visible text once and preserves the current draft',async t=>{
 const f=workspaceFixture(t);let finish;
 f.messages(()=>[response({followups:["Does this conflict with the previous budget?","What facts are still missing?"]})]);
 f.overrides.discussion_submit=()=>new Promise(resolve=>{finish=resolve;});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:"Content I am still writing"}});await f.settle();
 const follow=f.find(view,n=>n.type==='button'&&f.text(n)==="Does this conflict with the previous budget?");follow.props.onClick();follow.props.onClick();await f.settle();
 const calls=f.calls.filter(c=>c.name==='discussion_submit');assert.equal(calls.length,1);assert.equal(calls[0].args.text,"Does this conflict with the previous budget?");assert.equal(calls[0].args.topicId,f.topic.id);
 finish(f.topic);await f.settle();assert.equal(f.db.get('discussion:topic-a').body,"Content I am still writing");
});
test('retry resumes the logical input without rewriting or consuming the composer draft',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({status:'failed'})]);f.overrides.discussion_retry=async()=>f.topic;
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:"Another unsent idea"}});await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Retry").props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_retry').args.inputId,'logical-input');
 assert.equal(f.db.get('discussion:topic-a').body,"Another unsent idea");
});
test('a failed answer preserves its committed write through viewing changes, retry and undo',async t=>{
 const f=workspaceFixture(t,{native:true});
 const original="Change the weekend budget to 80 yuan and revisit the other plan later.",draft="Next unsent idea";
 const receipt={request_id:'committed-budget-write',memory_id:'budget',status:'applied',before_version:'budget-before',after_version:'budget-after'};
 const user={seq:1,id:'original-expression',turn_id:'logical-input',role:'user',status:'complete',text:original,citations:[],followups:[],receipts:[],record_only:false,progress:null};
 let answer=response({status:'failed',error_code:'network',text:"Updated the weekend budget to 80 yuan.",receipts:[receipt]});
 f.messages(()=>[user,answer]);
 f.db.set('discussion:topic-a',{key:'discussion:topic-a',request_id:'unsent-next-input',title:'',body:draft,context:[],expected_version:null});
 f.overrides.discussion_changes=async()=>[{receipt,before:{id:'budget-before',memory_id:'budget',title:"Weekend budget",body:"Keep the budget within 100 yuan."},after:{id:'budget-after',memory_id:'budget',title:"Weekend budget",body:"Keep the budget within 80 yuan."}}];
 f.overrides.discussion_retry=async()=>{answer={...answer,status:'processing',error_code:null};return f.topic;};
 f.overrides.discussion_undo=async()=>{answer={...answer,status:'cancelled',error_code:'changes_undone',receipts:[{...receipt,status:'undone'}]};return {receipt:{status:'applied'},conflicts:[]};};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const Changes=f.load('src/workspace/MemoryChanges.tsx').default;
 const group=()=>f.find(view,n=>n.type===Changes);
 const changes=f.mount(Changes,group().props);await f.settle();
 const statusTexts=surface=>f.nodes(surface.tree).filter(n=>n.props?.role==='status').map(n=>f.text(n));
 assert.deepEqual(statusTexts(view),["Could not reach the model service. Please try again."]);
 assert.deepEqual(statusTexts(changes),["Updated 1 memoryView changesUndo this turn"]);
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==="Updated the weekend budget to 80 yuan."));
 assert.equal(f.nodes(view.tree).filter(n=>n.type==='article'&&n.props.className==='discussion-message user').length,1);
 assert(f.text(view.tree).includes(original));
 f.find(changes,n=>n.type==='button'&&f.text(n)==="View changes").props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_changes').args.inputId,'logical-input');
 const comparison=f.find(changes,n=>n.props?.title==="Memory changes");
 assert.deepEqual(f.nodes(comparison).filter(n=>n.type==='Markdown').map(n=>n.props.text),["Keep the budget within 100 yuan.","Keep the budget within 80 yuan."]);
 comparison.props.onClose();await f.settle();
 f.find(view,n=>n.type==='button'&&f.text(n)==="Retry").props.onClick();await f.settle();f.render(changes,group().props);
 assert.equal(f.calls.filter(c=>c.name==='discussion_retry').length,1);
 assert.equal(f.calls.find(c=>c.name==='discussion_retry').args.inputId,'logical-input');
 assert.equal(f.calls.filter(c=>c.name==='discussion_submit').length,0);
 assert.deepEqual(statusTexts(changes),["Updated 1 memoryView changesUndo this turn"]);
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==="Retry"));
 assert.equal(f.db.get('discussion:topic-a').body,draft);
 answer={...answer,status:'failed',error_code:'network'};
 f.emit('discussion-updated',{topicId:'topic-a',inputId:'logical-input'});await f.settle();f.render(changes,group().props);
 assert.deepEqual(statusTexts(view),["Could not reach the model service. Please try again."]);
 assert.deepEqual(statusTexts(changes),["Updated 1 memoryView changesUndo this turn"]);
 f.find(changes,n=>n.type==='button'&&f.text(n)==="Undo this turn").props.onClick();await f.settle();f.render(changes,group().props);
 assert.equal(f.calls.filter(c=>c.name==='discussion_undo').length,1);
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'logical-input');
 assert.deepEqual(statusTexts(changes),["Changes undoneView changes"]);
 assert(f.text(view.tree).includes(original));
 assert.equal(f.nodes(view.tree).filter(n=>n.type==='article'&&n.props.className==='discussion-message user').length,1);
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==="Retry"));
 assert.equal(f.db.get('discussion:topic-a').body,draft);
});
test('a follow-up with an unknown acknowledgement reuses its request id',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({followups:["Review the budget again."]})]);
 f.overrides.discussion_submit=async()=>{throw "Disconnected";};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 for(let i=0;i<2;i++){f.find(view,n=>n.type==='button'&&f.text(n)==="Review the budget again.").props.onClick();await f.settle();}
 const requests=f.calls.filter(c=>c.name==='discussion_submit');assert.equal(requests.length,2);assert.equal(requests[0].args.id,requests[1].args.id);
});
test('an undone input retains its text and has no action that replays the changes',async t=>{
 const f=workspaceFixture(t);f.messages(()=>[response({status:'cancelled',error_code:'changes_undone'})]);
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 assert(f.text(view.tree).includes("Changes undone"));
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==="The budget has been considered."));
 assert(!f.nodes(view.tree).some(n=>n.type==='button'&&f.text(n)==="Retry"));
});
test('streamed Markdown stays visible during execution and after cancellation',async t=>{
 const f=workspaceFixture(t,{native:true});let current=response({status:'processing',text:"Review the **budget**",progress:"Reading related memories"});
 f.messages(()=>[current]);f.overrides.discussion_cancel=async()=>{current={...current,status:'cancelled'};};
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==="Review the **budget**"));
 current={...current,text:"Review the **budget**, then the schedule."};f.emit('discussion-updated',{topicId:'topic-a',inputId:'logical-input'});await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text===current.text));
 const composer=f.composer(view);composer.props.onCancel();await f.settle();
 assert(f.nodes(view.tree).some(n=>n.type==='Markdown'&&n.props.text==="Review the **budget**, then the schedule."));
 assert(f.text(view.tree).includes("Stopped"));
});
test('group undo shows conflicts without claiming success or changing receipts',async t=>{
 const f=workspaceFixture(t);const receipts=['a','b'].map(id=>({request_id:id,memory_id:id,status:'applied'}));
 f.messages(()=>[response({receipts})]);f.overrides.discussion_undo=async()=>({receipt:null,conflicts:['b']});
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const node=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges'),group=f.mount(node.type,node.props);await f.settle();
 f.find(group,n=>n.type==='button'&&f.text(n)==="Undo this turn").props.onClick();await f.settle();
 assert(f.text(group.tree).includes("Nothing was undone"));assert(f.text(group.tree).includes("Updated 2 memories"));
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'logical-input');
});
test('memory sources expose a whole input receipt group after its conversation is deleted',async t=>{
 const f=workspaceFixture(t);let groups=[{input_id:'deleted-conversation-input',receipts:[{request_id:'first-write',memory_id:'a',status:'applied'},{request_id:'second-write',memory_id:'b',status:'applied'}]}];
 f.overrides.library_agent_changes=async()=>groups;
 f.overrides.discussion_undo=async()=>{groups=[{...groups[0],receipts:groups[0].receipts.map(r=>({...r,status:'undone'}))}];return {receipt:{status:'applied'},conflicts:[]};};
 const view=f.mount(f.load('src/workspace/MemoryChanges.tsx').MemoryChangeHistory,{memoryId:'a',onOpenRecord(){},onRefresh(){}});await f.settle();
 assert.equal(f.calls.find(c=>c.name==='library_agent_changes').args.memoryId,'a');
 const group=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges'),receipt=f.mount(group.type,group.props);await f.settle();
 assert(f.text(receipt.tree).includes("Updated 2 memories"));
 f.find(receipt,n=>n.type==='button'&&f.text(n)==="Undo this turn").props.onClick();await f.settle();
 assert.equal(f.calls.find(c=>c.name==='discussion_undo').args.inputId,'deleted-conversation-input');
 assert(f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryChanges').props.receipts.every(r=>r.status==='undone'));
});
test('source provenance opens a live conversation and labels a deleted one without hiding the original text',async t=>{
 for(const available of [true,false]) {
  const f=workspaceFixture(t);let opened;
  f.overrides.library_detail=async()=>({key:f.keyA,state:'active',title:"Walking plan",body:"Plan",current:{id:'v-a',capture_ids:['raw'],created_at:1,actor:'ai'},history:[],sources:[{id:'raw',conversation_available:available,capture:{id:'raw',text:"Original: keep the budget within 80 yuan.",created_at:1,origin:{kind:'discussion',conversation_id:'original-topic',message_id:'user-message',app:'Memivy'}}}]});
  const view=f.mount(f.load('src/workspace/MemoryDetail.tsx').default,{record:f.keyA,initialReceipt:null,query:'',onChanged(){},onBack(){},onDiscuss:async()=>{},onOpenDiscussion:async id=>{opened=id;}});await f.settle();
  f.find(view,n=>n.type==='button'&&n.props.role==='tab'&&f.text(n).startsWith("Sources")).props.onClick();await f.settle();
  assert(f.nodes(view.tree).some(n=>n.props?.text==="Original: keep the budget within 80 yuan."));
  if(available){f.find(view,n=>n.type==='button'&&f.text(n)==="Open original discussion").props.onClick();await f.settle();assert.equal(opened,'original-topic');}
  else assert(f.text(view.tree).includes("Original discussion is no longer available"));
  f.unmount(view);
 }
});
test('memory focus can be added and removed without changing the unsent expression',async t=>{
 const f=workspaceFixture(t),view=f.mount(f.load('src/workspace/CaptureForm.tsx').default,{onSubmit:async()=>{}});await f.settle();
 f.find(view,n=>n.type==='textarea').props.onChange({target:{value:"Preserve this draft"}});await f.settle();
 f.find(view,n=>n.type==='button'&&n.props['aria-label']==="Add memory").props.onClick();await f.settle();
 const node=f.find(view,n=>typeof n.type==='function'&&n.type.name==='MemoryPicker'),picker=f.mount(node.type,node.props);await new Promise(r=>setTimeout(r,150));await f.settle();
 f.find(picker,n=>n.type==='button'&&f.text(n)==='atest').props.onClick();await f.settle();
 assert.deepEqual(f.db.get('input').context,[{kind:'version',id:'v-a'}]);
 f.find(view,n=>n.type==='button'&&n.props['aria-label']==="Remove this discussion evidence").props.onClick();await f.settle();
 assert.equal(f.db.get('input').context.length,0);assert.equal(f.db.get('input').body,"Preserve this draft");
});

test('reading earlier replies exposes an icon jump without moving the reader or consuming the draft', async t=>{
 const f=workspaceFixture(t);let answer=response({status:'processing',text:"First paragraph"});
 f.messages(()=>[answer]);
 const view=f.mount(f.load('src/workspace/Discussion.tsx').default,props(f));await f.settle();
 const composer=f.composer(view);await f.settle();
 f.find(composer,n=>n.type==='textarea').props.onChange({target:{value:"Idea still being written"}});await f.settle();
 const list=f.find(view,n=>n.props.className==='discussion-messages');
 list.props.onScroll({currentTarget:{scrollHeight:1800,scrollTop:300,clientHeight:500}});await f.settle();
 view.dom.scrolls=0;answer={...answer,text:"First paragraph, new second paragraph"};
 f.render(view,{...props(f),revision:1});await f.settle();
 assert.equal(view.dom.scrolls,0);
 const jump=f.find(view,n=>n.props.className==='discussion-jump-button');
 assert.equal(jump.props.icon,'arrow');assert.equal(jump.props.children,undefined);
 jump.props.onClick();await f.settle();
 assert.equal(view.dom.scrolls,1);
 assert(!f.nodes(view.tree).some(n=>n.props.className==='discussion-jump-button'));
 assert.equal(f.db.get('discussion:topic-a').body,"Idea still being written");
 assert(!f.calls.some(c=>c.name==='discussion_submit'));
});
