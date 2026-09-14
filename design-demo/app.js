/* Phase 0 only: fixed examples and in-memory simulation. No persistence or network. */
(() => {
  'use strict';
  const $ = (s, root = document) => root.querySelector(s);
  const $$ = (s, root = document) => [...root.querySelectorAll(s)];
  const esc = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const clone = value => JSON.parse(JSON.stringify(value));
  const icons = {
    plus:'M12 5v14M5 12h14', search:'m21 21-4.4-4.4M19 10.5a8.5 8.5 0 1 1-17 0 8.5 8.5 0 0 1 17 0',
    note:'M8 3h9l4 4v14H5V3h3M16 3v5h5M9 12h8M9 16h6', spark:'m12 3 2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5L12 3Z',
    check:'m5 12 4 4L19 6', clock:'M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0', close:'m6 6 12 12M6 18 18 6',
    'chevron-up':'m7 14 5-5 5 5', 'chevron-right':'m9 6 6 6-6 6', arrow:'M5 12h14m-5-5 5 5-5 5', back:'M19 12H5m5-5-5 5 5 5',
    edit:'m15 5 4 4M4 20l4-1L20 7a2.1 2.1 0 0 0-3-3L5 16l-1 4Z', history:'M3 11a9 9 0 1 1 2 7M3 4v7h7M12 7v5l4 2',
    settings:'m9 3-1 3-3 1-2 3 2 2v3l3 2 1 3h5l1-3 3-1 2-4-2-2V7l-3-1-1-3H9ZM15.5 12a3.5 3.5 0 1 1-7 0 3.5 3.5 0 0 1 7 0',
    link:'m10 14 4-4M8 16l-2 2a3 3 0 0 1-4-4l5-5a3 3 0 0 1 4 0m2 6a3 3 0 0 0 4 0l5-5a3 3 0 0 0-4-4l-2 2',
    globe:'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M3 12h18M12 3c5 5 5 13 0 18-5-5-5-13 0-18',
    lock:'M7 10V7a5 5 0 0 1 10 0v3M5 10h14v11H5V10ZM12 14v3', trash:'M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7M14 10v7',
    info:'M12 11v6M12 7h.01M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0', download:'M12 3v12m-5-5 5 5 5-5M4 16v5h16v-5',
    pause:'M8 5v14M16 5v14', play:'m7 4 13 8-13 8V4Z', agent:'M5 8h14v11H5V8ZM12 3v5M9 12h.01M15 12h.01M9 16h6M2 11v5M22 11v5',
    leaf:'M5 20c1-7 5-11 12-14M5 16C1 6 10 2 21 3c0 11-4 18-14 15', loading:'M21 12a9 9 0 1 1-6-8.5',
    window:'M3 4h18v16H3V4ZM3 8h18', copy:'M9 8h11v13H9V8ZM5 16H2V2h11v3', undo:'M3 10h11a6 6 0 0 1 0 12M3 10l6-6M3 10l6 6',
    alert:'M12 3 1 21h22L12 3ZM12 9v5M12 18h.01'
  };
  const icon = name => `<svg class="icon ${name === 'loading' ? 'spin' : ''}" viewBox="0 0 24 24" aria-hidden="true"><path d="${icons[name] || icons.note}"/></svg>`;
  function hydrate(root = document) { $$('[data-icon]', root).forEach(el => { el.innerHTML = icon(el.dataset.icon); }); }
  const state = { memories:clone(window.MEMIVY_SEED), selected:'first-experience', filter:'all', query:'', scenario:'everyday', drafts:{}, captureDraft:'', captureTarget:null, receipt:null, paused:false, mcp:false, configured:true, tested:true, model:'your-model', endpoint:'http://localhost:11434/v1', jobs:new Set(), historyVersion:null, serial:0, emptyBackup:null, pendingDeleteId:null, testedConfig:null, operations:{} };
  let receiptTimer, toastTimer;
  const id = prefix => `${prefix}-${Date.now()}-${++state.serial}`;
  const current = m => m.versions.at(-1);
  const selected = () => state.memories.find(m => m.id === state.selected);
  const isAgent = c => ['Codex','Claude Code'].includes(c.app);
  const appClass = c => isAgent(c) ? 'agent' : c.app === 'Safari' ? 'web' : 'mine';
  const avatar = c => `<span class="source-avatar ${appClass(c)}" title="${esc(c.app)}">${isAgent(c) ? '✳' : c.app === 'Safari' ? '↗' : 'Me'}</span>`;
  const allText = m => [m.title,m.project,current(m)?.body,...m.keywords,...m.captures.map(c => [c.text,c.app,c.project,c.url].join(' '))].join(' ').toLowerCase();
  const terms = () => state.query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  function matched() { return state.memories.filter(m => (state.filter === 'all' || m.captures.some(c => state.filter === 'agent' ? isAgent(c) : !isAgent(c))) && terms().every(t => allText(m).includes(t))); }
  function highlight(text) {
    if (!terms().length) return esc(text);
    const escapedTerms = terms().map(t => t.replace(/[.*+?^${}()|[\]\\]/g,'\\$&')).sort((a,b)=>b.length-a.length);
    return String(text).split(new RegExp(`(${escapedTerms.join('|')})`,'gi')).map(part => terms().includes(part.toLowerCase()) ? `<mark>${esc(part)}</mark>` : esc(part)).join('');
  }
  function preview(m) {
    if (!terms().length) return esc(m.excerpt);
    const fields = [current(m)?.body,...m.captures.map(c=>c.text),m.keywords.join(' ')].filter(Boolean);
    const field = fields.find(f=>terms().some(t=>f.toLowerCase().includes(t))) || m.excerpt;
    const first = Math.max(0,field.toLowerCase().indexOf(terms().find(t=>field.toLowerCase().includes(t)) || ''));
    return `${first > 18 ? '…' : ''}${highlight(field.slice(Math.max(0,first-18),first+90))}`;
  }
  function notify(message) { clearTimeout(toastTimer); $('#toast').textContent=message; $('#toast').hidden=false; toastTimer=setTimeout(()=>$('#toast').hidden=true,4200); }
  function renderList() {
    const list = matched();
    $('#memory-count').textContent=state.memories.length;
    $('#list-label').textContent=state.query ? 'Keyword search · No AI' : 'Recently updated';
    $('#result-count').textContent=state.query || state.filter !== 'all' ? `${list.length} items` : '';
    $$('.filter-tabs button').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.filter===state.filter)));
    $('#memory-list').innerHTML = list.length ? list.map(m=>`<button class="memory-card ${m.id===state.selected?'selected':''}" data-note="${m.id}" aria-current="${m.id===state.selected?'true':'false'}"><div class="card-topline"><span class="project-label"><i class="project-dot ${m.tone}"></i>${esc(m.project)}</span><span>${esc(m.updated)}</span></div><h2>${highlight(m.title)} ${state.drafts[m.id]?'<span class="draft-tag">Draft</span>':''}</h2><p>${preview(m)}</p><div class="card-footer">${icon(m.captures.some(isAgent)?'agent':'note')}<span>${m.captures.length} ${m.captures.length === 1 ? 'original note' : 'original notes'}${m.captures.some(isAgent)?' · includes Agent sources':''}</span>${m.status!=='ready'?`<span class="waiting-label">${{unassigned:'Unassigned',processing:'Understanding',failed:'Waiting to organize',unconfigured:'Awaiting setup',paused:'Paused'}[m.status]||''}</span>`:''}</div></button>`).join('') : `<div class="list-empty">${icon(state.query?'search':'leaf')}<strong>${state.query?'No matching idea':'A quiet space for now'}</strong><p>${state.query?'Try another word or a phrase from your original note.':'Start with a single sentence.'}</p>${state.query||state.filter!=='all'?'<button class="button secondary small" data-action="clear-search">Clear search and filters</button>':''}</div>`;
  }
  function render() { renderList(); renderDetail(); updateService(); }
  function renderDetail() {
    const m=selected(), list=matched();
    if (!m || !list.length) {
      $('#detail-toolbar').innerHTML=`<div class="breadcrumbs"><button class="icon-button back-button" data-action="back" aria-label="Back to memories">${icon('back')}</button>My memories</div>`;
      $('#memory-detail').innerHTML=`<div class="empty-state"><div class="empty-art" aria-hidden="true"><div class="empty-paper">${icon(list.length?'note':state.query?'search':'leaf')}</div></div><h2>${state.query?'Try another word to find that idea.':'A place for your next idea.'}</h2><p>${state.query?'Try walking, first use, or a phrase you remember. Keyword search works without a model.':'No title or folder needed. Save this thought and work out the rest later.'}</p><button class="button primary" data-action="${state.query?'clear-search':'capture'}">${icon(state.query?'search':'plus')}${state.query?'Clear search and filters':'Save your first idea'}</button></div>`;
      return;
    }
    const v=current(m), draft=state.drafts[m.id];
    $('#detail-toolbar').innerHTML=`<div class="breadcrumbs"><button class="icon-button back-button" data-action="back" aria-label="Back to memories">${icon('back')}</button><span class="crumb-home">My memories</span>${icon('chevron-right')}<span>${esc(m.project)}</span></div><div class="toolbar-actions">${draft?'<button class="button ghost small" data-action="cancel-edit">Cancel</button><button class="button primary small" data-action="save-edit">Save version <kbd>⌘ S</kbd></button>':`<button class="button ghost small" data-action="history" ${!v?'disabled':''}>${icon('history')}Version history</button><button class="icon-button" data-action="delete" aria-label="Delete this memory" title="Delete memory">${icon('trash')}</button>`}</div>`;
    const body=draft?`<label class="form-field"><span>Memory title</span><input class="editor-title" id="edit-title" value="${esc(draft.title)}" aria-label="Edit memory title"></label><label class="form-field"><span>Current content</span><textarea class="editor-body" id="edit-body" aria-label="Edit current memory">${esc(draft.body)}</textarea></label><p class="editor-info">Saving creates a new version. Original notes stay intact, and your draft remains when switching memories.</p>`:`<div class="note-heading"><div class="note-symbol ${m.tone}">${icon(m.project==='Everyday'?'leaf':'note')}</div><span class="project-chip">${esc(m.project)}</span></div><h1 class="note-title">${highlight(m.title)}</h1><div class="note-meta"><span class="source-avatars">${m.captures.slice(-3).map(avatar).join('')}</span><span>${m.captures.length} ${m.captures.length === 1 ? 'original note' : 'original notes'}, growing over time</span><i class="meta-divider"></i><span>${esc(m.updated)} updated</span></div>${noticeMarkup(m)}${v?`<section class="current-section" aria-label="Current memory version"><div class="section-topline"><h2 class="section-label">Current memory <span class="ai-badge">${icon(v.author==='Me'?'edit':'spark')}${v.author==='Me'?'Edited by me':'AI organized'}</span></h2><button class="button ghost small" data-action="edit">${icon('edit')}Edit</button></div><div class="memory-prose">${v.body.split(/\n\n+/).map((p,i)=>i===0&&p.includes('.')?`<p class="lead">${highlight(p.slice(0,p.indexOf('.')+1))}</p><p>${highlight(p.slice(p.indexOf('.')+1))}</p>`:`<p>${highlight(p)}</p>`).join('')}</div><p class="memory-footnote">${icon('history')}v${v.number} · Based on ${v.sourceIds.length} ${v.sourceIds.length === 1 ? 'original note' : 'original notes'}${v.sourceIds.map(cid=>`<button class="source-ref" data-source="${cid}" aria-label="View original note ${m.captures.findIndex(c=>c.id===cid)+1}">${m.captures.findIndex(c=>c.id===cid)+1}</button>`).join('')}</p></section>`:''}`;
    $('#memory-detail').innerHTML=`${body}<section class="sources-section" aria-label="Original notes and sources"><div class="section-topline"><h2 class="section-label">${icon('link')}Original notes and sources</h2><span class="source-total">${m.captures.length} ${m.captures.length === 1 ? 'note' : 'notes'}</span></div><p class="sources-subtitle">Your words stay exactly as you wrote them.</p><div class="source-list">${[...m.captures].reverse().map(c=>`<section id="source-${c.id}" class="source-card" aria-label="${esc(c.app)} original notes"><div class="source-topline"><span class="source-byline"><span class="source-index">${m.captures.indexOf(c)+1<10?'0':''}${m.captures.indexOf(c)+1}</span>${avatar(c)}${esc(c.app)}${isAgent(c)?' · Explicitly requested by the user':''}</span><time>${esc(c.time)}</time></div><blockquote>${highlight(c.text)}</blockquote><div class="source-origin">${icon(c.url?'globe':isAgent(c)?'agent':'note')}${c.url?`<a href="${esc(c.url)}" target="_blank" rel="noopener noreferrer">${esc(c.sourceLabel)} ↗</a>`:`<span>${esc(c.sourceLabel)}</span>`}${isAgent(c)?`<span>· ${esc(c.project)} project</span>`:''}<span>· Original preserved</span>${state.operations[c.id]?`<button data-action="source-receipt" data-capture="${c.id}">View action</button>`:''}</div></section>`).join('')}</div></section><button class="append-prompt" data-action="append"><span>${icon('plus')}Anything else to add to this idea?</span>${icon('arrow')}</button><p class="end-mark">${icon('lock')}Every change has a source.</p>`;
  }
  function noticeMarkup(m) {
    if (m.status==='ready') return '';
    const map={paused:['pause','Original saved. Waiting to resume.','Memivy is paused. Capture and search still work; organization can resume later.'],unassigned:['note','Original saved. Left unassigned.','No need to organize yet. Add more when the idea becomes clearer.'],processing:['loading','Original saved. Organizing.','Keep capturing or searching while this runs.'],failed:['alert','Original saved. Waiting to organize.','The model connection failed. Original notes, editing, and keyword search still work.'],unconfigured:['spark','Original saved. Awaiting setup.','Connect your model so AI can help organize.']};
    const n=map[m.status]||map.unassigned;
    return `<div class="notice ${m.status==='failed'?'error':''}">${icon(n[0])}<div><p><strong>${n[1]}</strong></p><p>${n[2]}</p>${m.status==='failed'?'<button class="button secondary small" data-action="retry-note">Retry organization (demo)</button>':m.status==='unconfigured'?'<button class="button secondary small" data-action="settings">Set up model</button>':m.status==='processing'?'<button class="button secondary small" data-action="finish-processing">Finish organization (demo)</button>':''}<button class="button ghost small" data-action="edit">Organize manually</button></div></div>`;
  }
  function selectNote(noteId) {
    state.selected=noteId; render(); $('#detail-scroll').scrollTop=0; $('.workspace').classList.add('detail-open');
  }
  function clearSearch(){state.query='';state.filter='all';$('#search').value='';if(!selected())state.selected=state.memories[0]?.id;render();}
  function openDialog(dialogId,html){const d=$(`#${dialogId}`);d.innerHTML=html;d.showModal();return d;}
  function closeButton(){return `<button class="icon-button" data-action="close-dialog" aria-label="Close">${icon('close')}</button>`;}
  function closeDialog(el){el.closest('dialog')?.close();}
  function openCapture({text, target=null, agent=false}={}) {
    state.captureTarget=target;
    const value=text??state.captureDraft;
    const d=openDialog('capture-dialog',`<div class="dialog-header"><div class="capture-brand"><img src="mark.svg" alt="">memivy<span class="dialog-eyebrow">· Quick capture preview</span></div>${closeButton()}</div><form id="capture-form"><div class="capture-body">${target?`<span class="capture-target">Add to: ${esc(state.memories.find(m=>m.id===target)?.title)}</span>`:''}<h2 id="capture-title">What would you like to remember?</h2><textarea id="capture-input" class="capture-input" placeholder="An idea, a sentence, or something still taking shape…" aria-label="Enter your original note" autofocus>${esc(value)}</textarea><div class="capture-context"><span class="context-chip">${icon(agent?'agent':'globe')}Sample source · ${agent?'Codex':'Safari'}</span>${agent?'<span class="context-chip">User explicitly requested saving · Memivy</span>':'<label class="attach-toggle"><input type="checkbox" id="attach-source">Attach sample webpage</label>'}</div><div id="capture-url" class="capture-url" hidden>Bear · https://bear.app/ · Only attached when selected</div></div><div class="dialog-footer"><div class="capture-hint">Save your words first. Organize later.<span>Enter Save · Shift + Enter New line</span></div><button type="submit" class="button primary" id="capture-submit" ${!value.trim()?'disabled':''}>Capture ${icon('arrow')}</button></div></form>`);
    d.dataset.agent=String(agent);$('#capture-input').focus();
    $('#capture-input').addEventListener('input',e=>{state.captureDraft=e.target.value;$('#capture-submit').disabled=!e.target.value.trim();});
    $('#capture-input').addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&e.keyCode!==229){e.preventDefault();if(e.target.value.trim())$('#capture-form').requestSubmit();}});
    $('#attach-source')?.addEventListener('change',e=>$('#capture-url').hidden=!e.target.checked);
    $('#capture-form').addEventListener('submit',e=>{e.preventDefault();capture(agent);});
  }
  function captureTitle(text){
    if(text===window.MEMIVY_EXAMPLES.append)return 'Capture first, configure later';
    if(text===window.MEMIVY_EXAMPLES.new)return 'Copy a favorite sentence by hand';
    if(text===window.MEMIVY_EXAMPLES.uncertain)return 'Leave a little more space';
    const first=text.trim().split(/[.!?\n]/)[0];return first.length>28?first.slice(0,28)+'…':first||'A new idea';
  }
  function newMemory(capture, status='processing') {
    const m={id:id('memory'),title:captureTitle(capture.text),project:capture.project||'Unassigned',tone:'yellow',updated:'Just now',excerpt:capture.text,keywords:[],status,captures:[capture],versions:[]};
    state.memories.unshift(m);return m;
  }
  function addVersion(m,body,author='Memory Agent') {m.versions.push({number:(current(m)?.number||0)+1,author,time:'Just now',body,sourceIds:m.captures.map(c=>c.id)});m.updated='Just now';m.excerpt=body.slice(0,90);m.status='ready';}
  function capture(agent) {
    const text=$('#capture-input').value; if(!text.trim())return;
    const c={id:id('capture'),text,app:agent?'Codex':'Safari',project:agent?'Memivy':state.captureTarget?state.memories.find(m=>m.id===state.captureTarget)?.project:'Thoughts',time:'Just now',sourceLabel:agent?'Sample conversation · User explicitly requested saving':'Quick capture · App name only'};
    if($('#attach-source')?.checked){c.url='https://bear.app/';c.sourceLabel='Bear · Attached webpage';}
    const temporary=newMemory(c); state.captureDraft='';$('#capture-dialog').close();
    const op={capture:c,temporaryId:temporary.id,targetId:state.captureTarget,kind:'processing',scenario:state.scenario,undone:false};
    state.selected=temporary.id;state.query='';state.filter='all';$('#search').value=''; render(); $('.workspace').classList.add('detail-open');
    showReceipt(op);
    if(state.scenario==='processing') return;
    const job=setTimeout(()=>{state.jobs.delete(job);finishCapture(op);},1250);state.jobs.add(job);
  }
  function finishCapture(op, force=false) {
    const m=state.memories.find(n=>n.id===op.temporaryId);
    if(!m||op.kind!=='processing')return;
    if(state.drafts[m.id] || m.versions.length){op.kind='manual';m.status=m.versions.length?'ready':'unassigned';showReceipt(op);render();return;}
    if(!force && (!state.configured||op.scenario==='unconfigured')){m.status='unconfigured';op.kind='unconfigured';}
    else if(!force && state.paused){m.status='paused';op.kind='paused';}
    else if(!force && op.scenario==='failed'){m.status='failed';op.kind='failed';}
    else {
      const appendScenario=['append','agent'].includes(op.scenario);
      const impliedTarget=op.scenario==='new'||op.scenario==='uncertain'?null:state.memories.find(n=>n.id==='first-experience' && /first|configuration|getting started|show results first|new user/i.test(op.capture.text))?.id;
      const target=state.memories.find(n=>n.id===(op.targetId||(appendScenario?'first-experience':impliedTarget)) && n.id!==m.id && n.status==='ready');
      if(target){
        op.kind='append';op.targetId=target.id;op.before=clone(target);target.captures.push(op.capture);
        addVersion(target,`${current(target)?.body||''}\n\n${op.capture.text}`.trim());op.versionNumber=current(target).number;
        state.memories=state.memories.filter(n=>n.id!==m.id);state.memories=state.memories.filter(n=>n.id!==target.id);state.memories.unshift(target);if(state.selected===m.id)state.selected=target.id;
      } else if(op.scenario==='uncertain'||op.capture.text.trim().length<12){m.status='unassigned';op.kind='uncertain';op.targetId=m.id;}
      else {m.project=op.capture.project;addVersion(m,op.capture.text);op.kind='new';op.targetId=m.id;op.versionNumber=current(m).number;}
    }
    render();showReceipt(op);
  }
  function receiptContents(op){
    const m=state.memories.find(n=>n.id===op.targetId), target=m?.title||'this memory';
    const capture=op.capture||{};
    const titles={paused:'Original saved. Waiting to resume',processing:'Original saved. Organizing',append:`Added to: ${target}`,new:`Created: ${target}`,uncertain:'Saved without assignment',failed:'Saved. Waiting to organize',unconfigured:'Saved. Awaiting setup',undone:'Assignment undone; original preserved',separate:'Saved as a new memory',manual:'Original preserved for manual editing',deleted:'Memory removed',restored:'Memory restored'};
    const descriptions={paused:'Memivy is paused. Original notes remain searchable; organization can resume later.',processing:'Original saved. You can carry on.',append:`Original preserved · ${capture.app}${capture.url?' and attached webpage':''} · New version created`,new:`Original preserved · ${capture.app}${capture.url?' and attached webpage':''}`,uncertain:'There is not enough information to assign this yet. The original is searchable.',failed:'The model connection failed. Capture and keyword search still work.',unconfigured:'Set up your model, then try organizing again.',undone:'Undid the AI assignment. The original is now an unassigned note.',separate:'Saved a separate memory with its original words and sources.',manual:'AI did not overwrite your edits. The original stays intact.',deleted:'Undo this deletion to restore current content, original notes, and history.',restored:'Current content, original notes, and history restored.'};
    return `<button class="icon-button dismiss-receipt" data-action="dismiss-receipt" aria-label="Dismiss action">${icon('close')}</button><div class="receipt-top"><span class="receipt-icon ${['processing','uncertain','unconfigured','undone','paused'].includes(op.kind)?'waiting':op.kind==='failed'?'failed':''}">${icon(op.kind==='processing'?'loading':op.kind==='failed'?'alert':op.kind==='uncertain'?'note':'check')}</span><div><p class="receipt-title">${esc(titles[op.kind])}</p><p class="receipt-message">${esc(descriptions[op.kind])}</p></div></div><div class="receipt-actions">${op.kind!=='processing'&&op.kind!=='deleted'?'<button class="button secondary small" data-action="open-receipt-note">Open memory</button>':''}${['new','append'].includes(op.kind)?'<button class="button ghost small" data-action="undo-action">Undo</button>':''}${['append','uncertain','failed','unconfigured'].includes(op.kind)?'<button class="button ghost small" data-action="separate-action">Save as new memory</button>':''}${op.kind==='failed'?'<button class="button secondary small" data-action="retry-receipt">Retry</button>':''}${op.kind==='unconfigured'?'<button class="button ghost small" data-action="settings">Set up model</button>':''}${op.kind==='deleted'?'<button class="button secondary small" data-action="undo-delete">Undo deletion</button>':''}</div>`;
  }
  function scheduleReceipt(){clearTimeout(receiptTimer);if(state.receipt?.kind!=='processing')receiptTimer=setTimeout(()=>{if(!$('#receipt').matches(':hover')&&!$('#receipt').contains(document.activeElement))collapseReceipt();},10000);}
  function showReceipt(op){if(op.capture)state.operations[op.capture.id]=op;state.receipt=op;$('#receipt').innerHTML=receiptContents(op);$('#receipt').hidden=false;$('#last-receipt').hidden=true;scheduleReceipt();}
  function collapseReceipt(){clearTimeout(receiptTimer);$('#receipt').hidden=true;$('#last-receipt').hidden=!state.receipt;}
  function undoPlacement(separate=false){
    const op=state.receipt;if(!op || op.undone)return;
    const m=state.memories.find(n=>n.id===op.targetId||n.id===op.temporaryId);
    if(!m)return;
    // Do not overwrite a newer user edit or a later capture while undoing an older action.
    if(op.kind==='append' && (current(m)?.number!==op.versionNumber || state.drafts[m.id])){notify('This memory has later edits. Check its history before correcting it.');return;}
    if(op.kind==='append'){
      const index=state.memories.indexOf(m);state.memories[index]=clone(op.before);
      const fresh=newMemory(op.capture,separate?'ready':'unassigned');
      if(separate)addVersion(fresh,op.capture.text,'Me');op.targetId=fresh.id;state.selected=fresh.id;
    }else{
      if(!separate && (current(m)?.number!==op.versionNumber||state.drafts[m.id])){notify('The content has later edits. Check version history first.');return;}
      if(separate){if(m.versions.length||state.drafts[m.id]){notify('The content has later edits. Check version history first.');return;}addVersion(m,op.capture.text,'Me');m.project=op.capture.project;}
      else{m.versions=[];m.status='unassigned';}
      op.targetId=m.id;state.selected=m.id;
    }
    op.kind=separate?'separate':'undone';op.undone=true;render();showReceipt(op);
  }
  function edit(){const m=selected();if(!m)return;state.drafts[m.id]={title:m.title,body:current(m)?.body||m.captures.map(c=>c.text).join('\n\n')};render();$('#edit-body')?.focus();}
  function saveEdit(){const m=selected(),d=m&&state.drafts[m.id];if(!d)return;if(!d.title.trim()||!d.body.trim()){notify('Both a title and content are required.');return;}m.title=d.title.trim();addVersion(m,d.body,'Me');delete state.drafts[m.id];render();notify(`Saved v${current(m).number}. Original notes unchanged.`);}
  function openHistory(){const m=selected();if(!m?.versions.length)return;state.historyVersion=current(m).number;openDialog('history-dialog',`<div class="dialog-header"><h2 id="history-title">Version history</h2>${closeButton()}</div><div class="history-layout"><nav class="version-list" id="version-list" aria-label="Version list"></nav><article class="version-preview" id="version-preview"></article></div><div class="dialog-footer"><button class="button secondary" data-action="close-dialog">Back to current version</button></div>`);renderHistory();}
  function renderHistory(){const m=selected(),v=m.versions.find(v=>v.number===state.historyVersion);$('#version-list').innerHTML=[...m.versions].reverse().map(v=>`<button class="version-option ${v.number===state.historyVersion?'selected':''}" data-version="${v.number}" aria-current="${v.number===state.historyVersion}"><strong>v${v.number}${v===current(m)?' · Current':''}</strong><span>${esc(v.time)}</span><span>${esc(v.author)}</span></button>`).join('');$('#version-preview').innerHTML=`<span class="ai-badge">Viewing v${v.number} · Read only</span><h3 style="margin-top:17px">${esc(m.title)}</h3><p class="version-caption">${esc(v.author)} · Based on ${v.sourceIds.length} ${v.sourceIds.length === 1 ? 'original note' : 'original notes'}</p><div class="memory-prose">${v.body.split(/\n\n+/).map(p=>`<p>${esc(p)}</p>`).join('')}</div>`;}
  function deleteNote(){const m=selected();if(!m)return;state.pendingDeleteId=m.id;openDialog('confirm-dialog',`<div class="dialog-header"><h2 id="confirm-title">Delete this memory?</h2>${closeButton()}</div><div class="dialog-content">The current content of “${esc(m.title)}”, ${m.captures.length} ${m.captures.length === 1 ? 'original note' : 'original notes'}, and history will be removed together.<br>You can undo this deletion from the action card.</div><div class="dialog-footer"><button class="button secondary" data-action="close-dialog" autofocus>Cancel</button><button class="button danger" data-action="confirm-delete">Delete memory</button></div>`);}
  function confirmDelete(){const m=state.memories.find(n=>n.id===state.pendingDeleteId);if(!m){$('#confirm-dialog').close();return;}state.pendingDeleteId=null;state.memories=state.memories.filter(n=>n.id!==m.id);state.selected=matched()[0]?.id;$('#confirm-dialog').close();render();showReceipt({kind:'deleted',deleted:clone(m)});}
  function openSettings(){
    state.tested=state.configured;state.testedConfig=state.configured?{endpoint:state.endpoint,model:state.model}:null;
    openDialog('settings-dialog',`<div class="dialog-header"><h2 id="settings-title">Make Memivy work your way</h2>${closeButton()}</div><div class="dialog-content"><p class="settings-intro">Your memories stay on this computer. Choose your model and how AI helps.</p><section class="settings-section"><h3>${icon('spark')}Your model <span class="ai-badge">Connection demo</span></h3><div class="form-grid"><label class="form-field full">API Base URL<input id="model-endpoint" type="url" value="${esc(state.endpoint)}" spellcheck="false"></label><label class="form-field">Model ID<input id="model-id" value="${esc(state.model)}" spellcheck="false"></label><label class="form-field">API Key<input type="password" placeholder="Disabled in the demo. Do not enter real keys" disabled autocomplete="off"></label></div><p id="endpoint-note" class="settings-note"></p><div class="connection-row"><button class="button secondary small" data-action="test-connection">Test connection (demo)</button><span id="connection-status" class="connection-status">${state.configured?'Simulated connection · No request sent':'Awaiting setup'}</span></div></section><section class="settings-section setting-row"><div><h3>${icon('agent')}For other Agent tools</h3><p>Allow local AI tools to search a few relevant memories and save only when you explicitly request it.</p></div><button id="mcp-switch" class="switch" role="switch" aria-label="Allow local MCP" aria-checked="${state.mcp}" data-action="toggle-mcp"></button></section><p class="settings-note">This switch is a preview. Only memory_capture and memory_search are available. Pausing Memivy also stops local MCP requests.</p><section class="settings-section setting-row"><div><h3>${icon('download')}Take your memories with you</h3><p>Export current content, original notes, sources, and history. Model settings and keys are excluded.</p></div><button class="button secondary small" data-action="export">Export sample Markdown</button></section></div><div class="dialog-footer"><span class="capture-hint" style="margin-right:auto">Demo settings reset on refresh</span><button class="button primary" data-action="save-settings">Done</button></div>`);
    updateEndpointNote();['model-endpoint','model-id'].forEach(field=>$(`#${field}`).addEventListener('input',()=>{state.tested=false;$('#connection-status').textContent='Settings changed. Test the connection first';$('#connection-status').className='connection-status';updateEndpointNote();}));
  }
  function updateEndpointNote(){let local=false;try{const u=new URL($('#model-endpoint').value);local=['localhost','127.0.0.1','[::1]'].includes(u.hostname);}catch{}$('#endpoint-note').textContent=local?'Sample local endpoint · The app sends the current input and a few relevant memories to this endpoint. This demo sends nothing.':'Sample remote endpoint · The app sends the current input and a few relevant memories to this address. This demo sends nothing.';}
  function validConfig(endpoint,model){try{return ['http:','https:'].includes(new URL(endpoint).protocol)&&Boolean(model.trim());}catch{return false;}}
  function testConnection(){
    const status=$('#connection-status'),endpoint=$('#model-endpoint').value,model=$('#model-id').value;
    state.tested=false;state.testedConfig=null;
    if(!validConfig(endpoint,model)){status.textContent='Enter a valid http(s) address and model ID';status.className='connection-status error';return;}
    status.textContent='Testing (demo)…';const button=$('[data-action="test-connection"]');button.disabled=true;
    const job=setTimeout(()=>{state.jobs.delete(job);if(!$('#settings-dialog').open||!status.isConnected)return;button.disabled=false;
      if($('#model-endpoint').value!==endpoint||$('#model-id').value!==model){status.textContent='Settings changed. Test again';return;}
      state.tested=state.scenario!=='failed';state.testedConfig=state.tested?{endpoint,model}:null;
      status.className=`connection-status ${state.tested?'success':'error'}`;status.textContent=state.tested?'Simulated connection succeeded · No request sent':'Simulated connection failed · Capture and search still work';
    },950);state.jobs.add(job);
  }
  function saveSettings(){const endpoint=$('#model-endpoint').value,model=$('#model-id').value;
    if(!state.tested||!validConfig(endpoint,model)||state.testedConfig?.endpoint!==endpoint||state.testedConfig?.model!==model){notify('The simulated connection has not passed. You can close settings and keep capturing and searching.');return;}
    state.endpoint=endpoint;state.model=model;state.configured=true;$('#settings-dialog').close();notify('Demo settings applied.');
  }
  function updateService(){$('#service-label').textContent=state.paused?'Memivy paused':'Memivy available';$('#service-dot').classList.toggle('paused',state.paused);}
  function openMenu(){openDialog('menu-dialog',`<div class="dialog-header"><h2 id="menu-title">memivy <span class="dialog-eyebrow">Menu bar preview</span></h2>${closeButton()}</div><div class="menu-content"><div class="menu-status"><span class="status-dot ${state.paused?'paused':''}"></span>${state.paused?'Paused · MCP is not accepting requests':'Available · Ready for your next idea'}</div><button class="menu-item" data-action="menu-capture">${icon('plus')}Capture a moment<kbd>⌥ Space</kbd></button><button class="menu-item" data-action="close-dialog">${icon('window')}Open main window</button><hr class="menu-divider"><button class="menu-item" data-action="toggle-pause">${icon(state.paused?'play':'pause')}${state.paused?'Resume Memivy':'Pause Memivy'}</button><button class="menu-item" data-action="menu-settings">${icon('settings')}Settings</button><p class="menu-note">Browser preview only. No system state is read.</p></div>`);}
  function exportMarkdown(){const content=state.memories.map(m=>`# ${m.title}\n\n${current(m)?.body||'(Not organized)'}\n\n## Original notes and sources\n\n${m.captures.map(c=>`### ${c.app} · ${c.time}\n\n${c.text}\n\nSource: ${c.sourceLabel}${c.url?' · '+c.url:''}\nProject: ${c.project}`).join('\n\n')}\n\n## Version history\n\n${m.versions.map(v=>`### v${v.number} · ${v.author} · ${v.time}\n\n${v.body}\n\nSource IDs: ${v.sourceIds.join(', ')}`).join('\n\n')}`).join('\n\n---\n\n');const url=URL.createObjectURL(new Blob(['# Memivy demo sample export\n\nThis file contains phase 0 sample data.\n\n',content],{type:'text/markdown;charset=utf-8'}));const link=document.createElement('a');link.href=url;link.download='memivy-demo-memories.md';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);notify('Exported sample Markdown, including original notes and history.');}
  function openAbout(){openDialog('about-dialog',`<div class="dialog-header"><h2 id="about-title">Why this design</h2>${closeButton()}</div><div class="dialog-content"><span class="ai-badge">Phase 0 · v0.1 · Awaiting user feedback</span><h3>Capture quietly. Remember clearly.</h3><p>Keep the Miro white canvas, dark pill buttons, yellow branding, and subtle hints. Give the words space: a scannable list, current content first, and original sources below.</p><div class="about-color-row" aria-label="Using the Miro palette"><i style="background:#1c1c1e"></i><i style="background:#ffd02f"></i><i style="background:#fff4c4"></i><i style="background:#c3faf5"></i><i style="background:#fde0f0"></i><i style="background:#ffc6c6"></i></div><h3>Borrow useful patterns</h3><p>Use lightweight input from flomo, whitespace from mymind, list and reading hierarchy from Bear, version states and corrections from Mem, and keyword previews from Capacities. Unlike an image collection wall, Memivy needs readable long-form content and original notes.</p><div class="reference-links"><a href="https://help.flomoapp.com/basic/quick-input.html" target="_blank" rel="noopener">flomo · Quick capture ↗</a><a href="https://mymind.com/the-new-quick-note" target="_blank" rel="noopener">mymind · Quick Note ↗</a><a href="https://bear.app/" target="_blank" rel="noopener">Bear · Typography ↗</a><a href="https://help.mem.ai/features/clean-up" target="_blank" rel="noopener">Mem · AI Corrections ↗</a><a href="https://docs.capacities.io/reference/search" target="_blank" rel="noopener">Capacities · Search ↗</a></div><h3>Build trust through actions</h3><p>Action cards explain what changed, where it went, and how to undo it. Search works without AI; original notes stay separate from editable current versions. Buttons have labels, focus remains visible, and reduced-motion settings are respected.</p><div class="reference-links"><a href="https://www.microsoft.com/en-us/research/project/guidelines-for-human-ai-interaction/" target="_blank" rel="noopener">Microsoft · People and AI interaction ↗</a><a href="https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/" target="_blank" rel="noopener">W3C · Dialogs and focus ↗</a></div><h3>What this demo shows</h3><p>Clickable layout, reading, capture, action cards, corrections, history, and settings. Fixed samples and keyword rules simulate AI; new and appended content show the original text, not real model quality. Data lasts only for this page session. No model, database, MCP or system shortcut is connected.</p><p>The preferred font is Roobert PRO; if unavailable, it falls back to macOS and system fonts. Narrow windows show how the Mac layout adapts, not mobile support.</p></div><div class="dialog-footer"><button class="button primary" data-action="close-dialog">Keep exploring</button></div>`);}
  function reset(){state.jobs.forEach(clearTimeout);state.jobs.clear();clearTimeout(receiptTimer);state.memories=clone(window.MEMIVY_SEED);state.selected='first-experience';state.query='';state.filter='all';state.scenario='everyday';state.drafts={};state.captureDraft='';state.receipt=null;state.paused=false;state.configured=true;state.tested=true;state.mcp=false;state.emptyBackup=null;state.operations={};state.pendingDeleteId=null;state.testedConfig=null;state.endpoint='http://localhost:11434/v1';state.model='your-model';$('#search').value='';$('#scenario').value='everyday';$('#receipt').hidden=true;$('#last-receipt').hidden=true;$$('dialog[open]').forEach(d=>d.close());render();$('#detail-scroll').scrollTop=0;notify('Sample data restored.');}
  function setScenario(value){
    if(state.emptyBackup&&value!=='empty'){state.memories=state.emptyBackup;state.emptyBackup=null;}
    state.scenario=value;
    if(value==='empty'){state.jobs.forEach(clearTimeout);state.jobs.clear();state.emptyBackup=state.memories;state.memories=[];state.selected=null;clearSearch();collapseReceipt();return;}
    if(value==='no-results'){state.query='A cafe on Mars';$('#search').value=state.query;render();return;}
    if(value==='long'){clearSearch();selectNote('walking');return;}
    if(value==='everyday'){clearSearch();if(!selected())state.selected=state.memories[0]?.id;render();return;}
    if(value==='unconfigured')state.configured=false;
    else state.configured=true;
    clearSearch();openCapture({text:window.MEMIVY_EXAMPLES[value==='new'?'new':value==='uncertain'?'uncertain':'append'],agent:value==='agent'});
  }
  function retry(op){if(!op)return;const m=state.memories.find(n=>n.id===op.temporaryId);if(!m)return;op.kind='processing';op.scenario='new';m.status='processing';showReceipt(op);render();const job=setTimeout(()=>{state.jobs.delete(job);finishCapture(op,true);},900);state.jobs.add(job);}
  function retryNote(){const m=selected();if(!m)return;retry({capture:m.captures[0],temporaryId:m.id,targetId:m.id,kind:'processing',scenario:'new'});}
  document.addEventListener('click',e=>{
    const note=e.target.closest('[data-note]');if(note){selectNote(note.dataset.note);return;}
    const filter=e.target.closest('[data-filter]');if(filter){state.filter=filter.dataset.filter;state.selected=matched()[0]?.id;render();return;}
    const version=e.target.closest('[data-version]');if(version){state.historyVersion=Number(version.dataset.version);renderHistory();return;}
    const source=e.target.closest('[data-source]');if(source){const card=$(`#source-${source.dataset.source}`);card?.scrollIntoView({behavior:'smooth',block:'center'});card?.classList.add('highlight');setTimeout(()=>card?.classList.remove('highlight'),2000);return;}
    const b=e.target.closest('[data-action]');if(!b)return;
    const actions={
      capture:()=>openCapture(),append:()=>openCapture({target:state.selected}),back:()=>$('.workspace').classList.remove('detail-open'),
      'clear-search':clearSearch,edit,'save-edit':saveEdit,'cancel-edit':()=>{delete state.drafts[state.selected];render();},history:openHistory,
      delete:deleteNote,'confirm-delete':confirmDelete,'undo-delete':()=>{const op=state.receipt;state.memories.unshift(op.deleted);state.selected=op.deleted.id;op.targetId=op.deleted.id;op.kind='restored';render();showReceipt(op);},
      'close-dialog':()=>closeDialog(b),settings:()=>openSettings(),menu:openMenu,'menu-capture':()=>{$('#menu-dialog').close();openCapture();},'menu-settings':()=>{$('#menu-dialog').close();openSettings();},
      'toggle-pause':()=>{state.paused=!state.paused;updateService();$('#menu-dialog').close();if(!state.paused)Object.values(state.operations).filter(op=>op.kind==='paused').forEach(retry);notify(state.paused?'Memivy is paused; MCP is not accepting requests. Manual capture and search still work.':'Memivy resumed.');},
      'toggle-mcp':()=>{state.mcp=!state.mcp;$('#mcp-switch').setAttribute('aria-checked',String(state.mcp));},'test-connection':testConnection,'save-settings':saveSettings,export:exportMarkdown,
      'source-receipt':()=>showReceipt(state.operations[b.dataset.capture]),'dismiss-receipt':collapseReceipt,'last-receipt':()=>showReceipt(state.receipt),'open-receipt-note':()=>{const op=state.receipt;clearSearch();selectNote(op.targetId||op.temporaryId);collapseReceipt();},'undo-action':()=>undoPlacement(false),'separate-action':()=>undoPlacement(true),
      'retry-receipt':()=>retry(state.receipt),'retry-note':retryNote,'finish-processing':()=>{const m=selected();if(state.receipt?.temporaryId===m?.id)finishCapture(state.receipt,true);else retryNote();},reset,about:openAbout
    };actions[b.dataset.action]?.();
  });
  document.addEventListener('input',e=>{if(e.target.id==='edit-title')state.drafts[state.selected].title=e.target.value;if(e.target.id==='edit-body')state.drafts[state.selected].body=e.target.value;});
  $('#search').addEventListener('input',e=>{state.query=e.target.value;const list=matched();if(!list.some(m=>m.id===state.selected))state.selected=list[0]?.id;render();});
  $('#search').addEventListener('keydown',e=>{if(e.key==='Escape'){e.preventDefault();clearSearch();}if(e.key==='ArrowDown'){e.preventDefault();$('.memory-card')?.focus();}});
  $('#memory-list').addEventListener('keydown',e=>{const cards=$$('.memory-card'),i=cards.indexOf(e.target);if(i>=0&&['ArrowDown','ArrowUp'].includes(e.key)){e.preventDefault();cards[Math.max(0,Math.min(cards.length-1,i+(e.key==='ArrowDown'?1:-1)))].focus();}});
  $('#scenario').addEventListener('change',e=>setScenario(e.target.value));
  $('#receipt').addEventListener('mouseenter',()=>clearTimeout(receiptTimer));$('#receipt').addEventListener('mouseleave',scheduleReceipt);$('#receipt').addEventListener('focusin',()=>clearTimeout(receiptTimer));$('#receipt').addEventListener('focusout',scheduleReceipt);
  document.addEventListener('keydown',e=>{if(e.isComposing||e.keyCode===229)return;if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'&&!$('dialog[open]')){e.preventDefault();$('.workspace').classList.remove('detail-open');$('#search').focus();}if(e.altKey&&e.code==='Space'&&!$('dialog[open]')){e.preventDefault();openCapture();}if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='s'&&state.drafts[state.selected]&&!$('dialog[open]')){e.preventDefault();saveEdit();}});
  hydrate();render();
})();
