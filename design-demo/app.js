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
  const avatar = c => `<span class="source-avatar ${appClass(c)}" title="${esc(c.app)}">${isAgent(c) ? '✳' : c.app === 'Safari' ? '↗' : '我'}</span>`;
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
    $('#list-label').textContent=state.query ? '关键词搜索 · 无需 AI' : '最近更新';
    $('#result-count').textContent=state.query || state.filter !== 'all' ? `${list.length} 条` : '';
    $$('.filter-tabs button').forEach(b=>b.setAttribute('aria-pressed',String(b.dataset.filter===state.filter)));
    $('#memory-list').innerHTML = list.length ? list.map(m=>`<button class="memory-card ${m.id===state.selected?'selected':''}" data-note="${m.id}" aria-current="${m.id===state.selected?'true':'false'}"><div class="card-topline"><span class="project-label"><i class="project-dot ${m.tone}"></i>${esc(m.project)}</span><span>${esc(m.updated)}</span></div><h2>${highlight(m.title)} ${state.drafts[m.id]?'<span class="draft-tag">草稿</span>':''}</h2><p>${preview(m)}</p><div class="card-footer">${icon(m.captures.some(isAgent)?'agent':'note')}<span>${m.captures.length} 段原话${m.captures.some(isAgent)?' · 含 Agent 来源':''}</span>${m.status!=='ready'?`<span class="waiting-label">${{unassigned:'暂未归类',processing:'理解中',failed:'等待理解',unconfigured:'等待配置',paused:'等待恢复'}[m.status]||''}</span>`:''}</div></button>`).join('') : `<div class="list-empty">${icon(state.query?'search':'leaf')}<strong>${state.query?'没有找到这个想法':'这里还很安静'}</strong><p>${state.query?'换一个词，或试试原话里的片段。':'写下一句话，就从这里开始。'}</p>${state.query||state.filter!=='all'?'<button class="button secondary small" data-action="clear-search">清除搜索与筛选</button>':''}</div>`;
  }
  function render() { renderList(); renderDetail(); updateService(); }
  function renderDetail() {
    const m=selected(), list=matched();
    if (!m || !list.length) {
      $('#detail-toolbar').innerHTML=`<div class="breadcrumbs"><button class="icon-button back-button" data-action="back" aria-label="返回记忆列表">${icon('back')}</button>我的记忆</div>`;
      $('#memory-detail').innerHTML=`<div class="empty-state"><div class="empty-art" aria-hidden="true"><div class="empty-paper">${icon(list.length?'note':state.query?'search':'leaf')}</div></div><h2>${state.query?'想法还在，换个词找找。':'给下一个想法，一个落脚点。'}</h2><p>${state.query?'试试「先给结果」「散步」，或你记得的原话片段。关键词搜索不需要连接模型。':'不必想标题，也不用先分类。写下这一刻的想法，其余的可以慢慢来。'}</p><button class="button primary" data-action="${state.query?'clear-search':'capture'}">${icon(state.query?'search':'plus')}${state.query?'清除搜索与筛选':'记下第一个想法'}</button></div>`;
      return;
    }
    const v=current(m), draft=state.drafts[m.id];
    $('#detail-toolbar').innerHTML=`<div class="breadcrumbs"><button class="icon-button back-button" data-action="back" aria-label="返回记忆列表">${icon('back')}</button><span class="crumb-home">我的记忆</span>${icon('chevron-right')}<span>${esc(m.project)}</span></div><div class="toolbar-actions">${draft?'<button class="button ghost small" data-action="cancel-edit">取消</button><button class="button primary small" data-action="save-edit">保存版本 <kbd>⌘ S</kbd></button>':`<button class="button ghost small" data-action="history" ${!v?'disabled':''}>${icon('history')}历史版本</button><button class="icon-button" data-action="delete" aria-label="删除这条记忆" title="删除记忆">${icon('trash')}</button>`}</div>`;
    const body=draft?`<label class="form-field"><span>记忆标题</span><input class="editor-title" id="edit-title" value="${esc(draft.title)}" aria-label="编辑记忆标题"></label><label class="form-field"><span>当前内容</span><textarea class="editor-body" id="edit-body" aria-label="编辑当前记忆">${esc(draft.body)}</textarea></label><p class="editor-info">保存会建立新版本。下方原话保持原样，切换记忆会保留这份编辑草稿。</p>`:`<div class="note-heading"><div class="note-symbol ${m.tone}">${icon(m.project==='日常'?'leaf':'note')}</div><span class="project-chip">${esc(m.project)}</span></div><h1 class="note-title">${highlight(m.title)}</h1><div class="note-meta"><span class="source-avatars">${m.captures.slice(-3).map(avatar).join('')}</span><span>${m.captures.length} 段原话，慢慢长成</span><i class="meta-divider"></i><span>${esc(m.updated)} 更新</span></div>${noticeMarkup(m)}${v?`<section class="current-section" aria-label="当前记忆版本"><div class="section-topline"><h2 class="section-label">当前记忆 <span class="ai-badge">${icon(v.author==='我'?'edit':'spark')}${v.author==='我'?'我编辑的':'AI 整理'}</span></h2><button class="button ghost small" data-action="edit">${icon('edit')}编辑</button></div><div class="memory-prose">${v.body.split(/\n\n+/).map((p,i)=>i===0&&p.includes('。')?`<p class="lead">${highlight(p.slice(0,p.indexOf('。')+1))}</p><p>${highlight(p.slice(p.indexOf('。')+1))}</p>`:`<p>${highlight(p)}</p>`).join('')}</div><p class="memory-footnote">${icon('history')}v${v.number} · 依据 ${v.sourceIds.length} 段原话${v.sourceIds.map(cid=>`<button class="source-ref" data-source="${cid}" aria-label="查看原话 ${m.captures.findIndex(c=>c.id===cid)+1}">${m.captures.findIndex(c=>c.id===cid)+1}</button>`).join('')}</p></section>`:''}`;
    $('#memory-detail').innerHTML=`${body}<section class="sources-section" aria-label="原话与来源"><div class="section-topline"><h2 class="section-label">${icon('link')}原话与来源</h2><span class="source-total">${m.captures.length} 段记录</span></div><p class="sources-subtitle">当时怎么说的，就一直怎么留着。</p><div class="source-list">${[...m.captures].reverse().map(c=>`<section id="source-${c.id}" class="source-card" aria-label="${esc(c.app)} 原话"><div class="source-topline"><span class="source-byline"><span class="source-index">${m.captures.indexOf(c)+1<10?'0':''}${m.captures.indexOf(c)+1}</span>${avatar(c)}${esc(c.app)}${isAgent(c)?' · 用户要求记住':''}</span><time>${esc(c.time)}</time></div><blockquote>${highlight(c.text)}</blockquote><div class="source-origin">${icon(c.url?'globe':isAgent(c)?'agent':'note')}${c.url?`<a href="${esc(c.url)}" target="_blank" rel="noopener noreferrer">${esc(c.sourceLabel)} ↗</a>`:`<span>${esc(c.sourceLabel)}</span>`}${isAgent(c)?`<span>· ${esc(c.project)} 项目</span>`:''}<span>· 原话保留</span>${state.operations[c.id]?`<button data-action="source-receipt" data-capture="${c.id}">查看这次动作</button>`:''}</div></section>`).join('')}</div></section><button class="append-prompt" data-action="append"><span>${icon('plus')}这个想法，还有一句想补充？</span>${icon('arrow')}</button><p class="end-mark">${icon('lock')}每一次变化，都有来处。</p>`;
  }
  function noticeMarkup(m) {
    if (m.status==='ready') return '';
    const map={paused:['pause','原话已记下，等待恢复。','Memivy 已暂停，恢复后再继续理解。记录和搜索仍可用。'],unassigned:['note','原话已记下，暂未判断归属。','现在不用整理，等想法更清楚时再接着写。'],processing:['loading','原话已记下，正在理解。','你可以继续记录或搜索，不必等在这里。'],failed:['alert','原话已记下，等待理解。','模型连接失败。原话、编辑和关键词搜索不受影响。'],unconfigured:['spark','原话已记下，等待配置。','接入自己的模型后，再让 AI 帮你整理。']};
    const n=map[m.status]||map.unassigned;
    return `<div class="notice ${m.status==='failed'?'error':''}">${icon(n[0])}<div><p><strong>${n[1]}</strong></p><p>${n[2]}</p>${m.status==='failed'?'<button class="button secondary small" data-action="retry-note">重试理解（模拟）</button>':m.status==='unconfigured'?'<button class="button secondary small" data-action="settings">配置模型</button>':m.status==='processing'?'<button class="button secondary small" data-action="finish-processing">完成理解（模拟）</button>':''}<button class="button ghost small" data-action="edit">自己整理</button></div></div>`;
  }
  function selectNote(noteId) {
    state.selected=noteId; render(); $('#detail-scroll').scrollTop=0; $('.workspace').classList.add('detail-open');
  }
  function clearSearch(){state.query='';state.filter='all';$('#search').value='';if(!selected())state.selected=state.memories[0]?.id;render();}
  function openDialog(dialogId,html){const d=$(`#${dialogId}`);d.innerHTML=html;d.showModal();return d;}
  function closeButton(){return `<button class="icon-button" data-action="close-dialog" aria-label="关闭">${icon('close')}</button>`;}
  function closeDialog(el){el.closest('dialog')?.close();}
  function openCapture({text, target=null, agent=false}={}) {
    state.captureTarget=target;
    const value=text??state.captureDraft;
    const d=openDialog('capture-dialog',`<div class="dialog-header"><div class="capture-brand"><img src="mark.svg" alt="">memivy<span class="dialog-eyebrow">· 快捷捕捉示意</span></div>${closeButton()}</div><form id="capture-form"><div class="capture-body">${target?`<span class="capture-target">补充到「${esc(state.memories.find(m=>m.id===target)?.title)}」</span>`:''}<h2 id="capture-title">这一刻，想记住什么？</h2><textarea id="capture-input" class="capture-input" placeholder="一个念头、一段话，或者还没想清楚的事…" aria-label="输入想记住的原话" autofocus>${esc(value)}</textarea><div class="capture-context"><span class="context-chip">${icon(agent?'agent':'globe')}来源示例 · ${agent?'Codex':'Safari'}</span>${agent?'<span class="context-chip">用户已明确要求记住 · Memivy</span>':'<label class="attach-toggle"><input type="checkbox" id="attach-source">附带示例网页</label>'}</div><div id="capture-url" class="capture-url" hidden>Bear · https://bear.app/ · 仅在勾选后附带</div></div><div class="dialog-footer"><div class="capture-hint">先留住原话，再慢慢整理。<span>Enter 保存 · Shift + Enter 换行</span></div><button type="submit" class="button primary" id="capture-submit" ${!value.trim()?'disabled':''}>记下 ${icon('arrow')}</button></div></form>`);
    d.dataset.agent=String(agent);$('#capture-input').focus();
    $('#capture-input').addEventListener('input',e=>{state.captureDraft=e.target.value;$('#capture-submit').disabled=!e.target.value.trim();});
    $('#capture-input').addEventListener('keydown',e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.isComposing&&e.keyCode!==229){e.preventDefault();if(e.target.value.trim())$('#capture-form').requestSubmit();}});
    $('#attach-source')?.addEventListener('change',e=>$('#capture-url').hidden=!e.target.checked);
    $('#capture-form').addEventListener('submit',e=>{e.preventDefault();capture(agent);});
  }
  function captureTitle(text){
    if(text===window.MEMIVY_EXAMPLES.append)return '先接住想法，再慢慢配置';
    if(text===window.MEMIVY_EXAMPLES.new)return '把喜欢的句子，手抄一遍';
    if(text===window.MEMIVY_EXAMPLES.uncertain)return '再留一点空白';
    const first=text.trim().split(/[。！？\n]/)[0];return first.length>28?first.slice(0,28)+'…':first||'一个新想法';
  }
  function newMemory(capture, status='processing') {
    const m={id:id('memory'),title:captureTitle(capture.text),project:capture.project||'未归类',tone:'yellow',updated:'刚刚',excerpt:capture.text,keywords:[],status,captures:[capture],versions:[]};
    state.memories.unshift(m);return m;
  }
  function addVersion(m,body,author='Memory Agent') {m.versions.push({number:(current(m)?.number||0)+1,author,time:'刚刚',body,sourceIds:m.captures.map(c=>c.id)});m.updated='刚刚';m.excerpt=body.slice(0,90);m.status='ready';}
  function capture(agent) {
    const text=$('#capture-input').value; if(!text.trim())return;
    const c={id:id('capture'),text,app:agent?'Codex':'Safari',project:agent?'Memivy':state.captureTarget?state.memories.find(m=>m.id===state.captureTarget)?.project:'随想',time:'刚刚',sourceLabel:agent?'示例会话 · 用户明确要求保存':'快捷捕捉 · 仅附带应用名称'};
    if($('#attach-source')?.checked){c.url='https://bear.app/';c.sourceLabel='Bear · 主动附带的网页';}
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
      const impliedTarget=op.scenario==='new'||op.scenario==='uncertain'?null:state.memories.find(n=>n.id==='first-experience' && /首次|配置|上手|先给结果|新用户/.test(op.capture.text))?.id;
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
    const m=state.memories.find(n=>n.id===op.targetId), target=m?.title||'这条记忆';
    const capture=op.capture||{};
    const titles={paused:'原话已记下，等待恢复',processing:'原话已记下，正在理解',append:`已续到「${target}」`,new:`已建立「${target}」`,uncertain:'已记下，暂未判断归属',failed:'已记下，等待理解',unconfigured:'已记下，等待配置',undone:'已撤销归属，原话仍在',separate:'已另存为新记忆',manual:'原话已保留，由你继续整理',deleted:'记忆已移除',restored:'已恢复记忆'};
    const descriptions={paused:'Memivy 已暂停。恢复后可继续理解，原话仍可搜索。',processing:'已保留原话。你可以继续做自己的事。',append:`保留原话 · ${capture.app}${capture.url?' 与主动附带的网页':''} · 形成新版本`,new:`保留原话 · ${capture.app}${capture.url?' 与主动附带的网页':''}`,uncertain:'信息还不够，不勉强分类。原话可直接搜索。',failed:'模型连接失败，记录与关键词搜索照常可用。',unconfigured:'配置自己的模型后，可再尝试整理。',undone:'撤销的是 AI 归属。已放回一条暂未归类记录。',separate:'已另存一条记忆，保留原话和来源。',manual:'AI 没有覆盖你的编辑，原话始终保留。',deleted:'可撤销本次删除，恢复当前内容、原话和历史。',restored:'当前内容、原话和历史都已恢复。'};
    return `<button class="icon-button dismiss-receipt" data-action="dismiss-receipt" aria-label="收起回执">${icon('close')}</button><div class="receipt-top"><span class="receipt-icon ${['processing','uncertain','unconfigured','undone','paused'].includes(op.kind)?'waiting':op.kind==='failed'?'failed':''}">${icon(op.kind==='processing'?'loading':op.kind==='failed'?'alert':op.kind==='uncertain'?'note':'check')}</span><div><p class="receipt-title">${esc(titles[op.kind])}</p><p class="receipt-message">${esc(descriptions[op.kind])}</p></div></div><div class="receipt-actions">${op.kind!=='processing'&&op.kind!=='deleted'?'<button class="button secondary small" data-action="open-receipt-note">打开记忆</button>':''}${['new','append'].includes(op.kind)?'<button class="button ghost small" data-action="undo-action">撤销</button>':''}${['append','uncertain','failed','unconfigured'].includes(op.kind)?'<button class="button ghost small" data-action="separate-action">另存为新记忆</button>':''}${op.kind==='failed'?'<button class="button secondary small" data-action="retry-receipt">重试</button>':''}${op.kind==='unconfigured'?'<button class="button ghost small" data-action="settings">配置模型</button>':''}${op.kind==='deleted'?'<button class="button secondary small" data-action="undo-delete">撤销删除</button>':''}</div>`;
  }
  function scheduleReceipt(){clearTimeout(receiptTimer);if(state.receipt?.kind!=='processing')receiptTimer=setTimeout(()=>{if(!$('#receipt').matches(':hover')&&!$('#receipt').contains(document.activeElement))collapseReceipt();},10000);}
  function showReceipt(op){if(op.capture)state.operations[op.capture.id]=op;state.receipt=op;$('#receipt').innerHTML=receiptContents(op);$('#receipt').hidden=false;$('#last-receipt').hidden=true;scheduleReceipt();}
  function collapseReceipt(){clearTimeout(receiptTimer);$('#receipt').hidden=true;$('#last-receipt').hidden=!state.receipt;}
  function undoPlacement(separate=false){
    const op=state.receipt;if(!op || op.undone)return;
    const m=state.memories.find(n=>n.id===op.targetId||n.id===op.temporaryId);
    if(!m)return;
    // Do not overwrite a newer user edit or a later capture while undoing an older action.
    if(op.kind==='append' && (current(m)?.number!==op.versionNumber || state.drafts[m.id])){notify('这条记忆已有后续编辑，请先在历史版本中核对后再纠正。');return;}
    if(op.kind==='append'){
      const index=state.memories.indexOf(m);state.memories[index]=clone(op.before);
      const fresh=newMemory(op.capture,separate?'ready':'unassigned');
      if(separate)addVersion(fresh,op.capture.text,'我');op.targetId=fresh.id;state.selected=fresh.id;
    }else{
      if(!separate && (current(m)?.number!==op.versionNumber||state.drafts[m.id])){notify('当前内容已有后续编辑，请先查看历史版本。');return;}
      if(separate){if(m.versions.length||state.drafts[m.id]){notify('当前内容已有后续编辑，请先查看历史版本。');return;}addVersion(m,op.capture.text,'我');m.project=op.capture.project;}
      else{m.versions=[];m.status='unassigned';}
      op.targetId=m.id;state.selected=m.id;
    }
    op.kind=separate?'separate':'undone';op.undone=true;render();showReceipt(op);
  }
  function edit(){const m=selected();if(!m)return;state.drafts[m.id]={title:m.title,body:current(m)?.body||m.captures.map(c=>c.text).join('\n\n')};render();$('#edit-body')?.focus();}
  function saveEdit(){const m=selected(),d=m&&state.drafts[m.id];if(!d)return;if(!d.title.trim()||!d.body.trim()){notify('标题和当前内容都需要保留。');return;}m.title=d.title.trim();addVersion(m,d.body,'我');delete state.drafts[m.id];render();notify(`已保存 v${current(m).number}，原话保持原样。`);}
  function openHistory(){const m=selected();if(!m?.versions.length)return;state.historyVersion=current(m).number;openDialog('history-dialog',`<div class="dialog-header"><h2 id="history-title">历史版本</h2>${closeButton()}</div><div class="history-layout"><nav class="version-list" id="version-list" aria-label="历史版本列表"></nav><article class="version-preview" id="version-preview"></article></div><div class="dialog-footer"><button class="button secondary" data-action="close-dialog">返回当前版本</button></div>`);renderHistory();}
  function renderHistory(){const m=selected(),v=m.versions.find(v=>v.number===state.historyVersion);$('#version-list').innerHTML=[...m.versions].reverse().map(v=>`<button class="version-option ${v.number===state.historyVersion?'selected':''}" data-version="${v.number}" aria-current="${v.number===state.historyVersion}"><strong>v${v.number}${v===current(m)?' · 当前':''}</strong><span>${esc(v.time)}</span><span>${esc(v.author)}</span></button>`).join('');$('#version-preview').innerHTML=`<span class="ai-badge">正在查看 v${v.number} · 只读</span><h3 style="margin-top:17px">${esc(m.title)}</h3><p class="version-caption">${esc(v.author)} · 依据 ${v.sourceIds.length} 段原话</p><div class="memory-prose">${v.body.split(/\n\n+/).map(p=>`<p>${esc(p)}</p>`).join('')}</div>`;}
  function deleteNote(){const m=selected();if(!m)return;state.pendingDeleteId=m.id;openDialog('confirm-dialog',`<div class="dialog-header"><h2 id="confirm-title">删除这条记忆？</h2>${closeButton()}</div><div class="dialog-content">「${esc(m.title)}」的当前内容、${m.captures.length} 段原话和历史版本将一起移除。<br>你可以在回执中撤销本次删除。</div><div class="dialog-footer"><button class="button secondary" data-action="close-dialog" autofocus>取消</button><button class="button danger" data-action="confirm-delete">删除记忆</button></div>`);}
  function confirmDelete(){const m=state.memories.find(n=>n.id===state.pendingDeleteId);if(!m){$('#confirm-dialog').close();return;}state.pendingDeleteId=null;state.memories=state.memories.filter(n=>n.id!==m.id);state.selected=matched()[0]?.id;$('#confirm-dialog').close();render();showReceipt({kind:'deleted',deleted:clone(m)});}
  function openSettings(){
    state.tested=state.configured;state.testedConfig=state.configured?{endpoint:state.endpoint,model:state.model}:null;
    openDialog('settings-dialog',`<div class="dialog-header"><h2 id="settings-title">让 Memivy 按你的方式工作</h2>${closeButton()}</div><div class="dialog-content"><p class="settings-intro">你的记忆在本机。用自己的模型，决定 AI 如何参与。</p><section class="settings-section"><h3>${icon('spark')}你的模型 <span class="ai-badge">连接演示</span></h3><div class="form-grid"><label class="form-field full">API Base URL<input id="model-endpoint" type="url" value="${esc(state.endpoint)}" spellcheck="false"></label><label class="form-field">模型 ID<input id="model-id" value="${esc(state.model)}" spellcheck="false"></label><label class="form-field">API Key<input type="password" placeholder="演示中禁用，请勿输入真实密钥" disabled autocomplete="off"></label></div><p id="endpoint-note" class="settings-note"></p><div class="connection-row"><button class="button secondary small" data-action="test-connection">测试连接（模拟）</button><span id="connection-status" class="connection-status">${state.configured?'已模拟连接 · 不发送请求':'等待配置'}</span></div></section><section class="settings-section setting-row"><div><h3>${icon('agent')}供其他 Agent 使用</h3><p>允许本机 Agent 搜索少量相关记忆；只有你明确要求记住时，才会保存内容。</p></div><button id="mcp-switch" class="switch" role="switch" aria-label="允许本机 MCP" aria-checked="${state.mcp}" data-action="toggle-mcp"></button></section><p class="settings-note">此开关为示意。仅提供 memory_capture 与 memory_search。暂停 Memivy 后，本机 MCP 也暂停接收请求。</p><section class="settings-section setting-row"><div><h3>${icon('download')}带走你的记忆</h3><p>导出当前内容、原话、来源和历史。模型配置与密钥不在其中。</p></div><button class="button secondary small" data-action="export">导出示例 Markdown</button></section></div><div class="dialog-footer"><span class="capture-hint" style="margin-right:auto">仅演示设置，刷新后恢复</span><button class="button primary" data-action="save-settings">完成</button></div>`);
    updateEndpointNote();['model-endpoint','model-id'].forEach(field=>$(`#${field}`).addEventListener('input',()=>{state.tested=false;$('#connection-status').textContent='配置已修改，请先测试';$('#connection-status').className='connection-status';updateEndpointNote();}));
  }
  function updateEndpointNote(){let local=false;try{const u=new URL($('#model-endpoint').value);local=['localhost','127.0.0.1','[::1]'].includes(u.hostname);}catch{}$('#endpoint-note').textContent=local?'本地端点示例 · 正式版会把本次原话与少量候选记忆发给该端点。Demo 不发送任何内容。':'远程端点示例 · 正式版会把本次原话与少量候选记忆发送到这个地址。Demo 不发送任何内容。';}
  function validConfig(endpoint,model){try{return ['http:','https:'].includes(new URL(endpoint).protocol)&&Boolean(model.trim());}catch{return false;}}
  function testConnection(){
    const status=$('#connection-status'),endpoint=$('#model-endpoint').value,model=$('#model-id').value;
    state.tested=false;state.testedConfig=null;
    if(!validConfig(endpoint,model)){status.textContent='请填写有效的 http(s) 地址与模型 ID';status.className='connection-status error';return;}
    status.textContent='正在测试（模拟）…';const button=$('[data-action="test-connection"]');button.disabled=true;
    const job=setTimeout(()=>{state.jobs.delete(job);if(!$('#settings-dialog').open||!status.isConnected)return;button.disabled=false;
      if($('#model-endpoint').value!==endpoint||$('#model-id').value!==model){status.textContent='配置已修改，请重新测试';return;}
      state.tested=state.scenario!=='failed';state.testedConfig=state.tested?{endpoint,model}:null;
      status.className=`connection-status ${state.tested?'success':'error'}`;status.textContent=state.tested?'模拟连接成功 · 未发送请求':'模拟连接失败 · 记录和搜索仍可用';
    },950);state.jobs.add(job);
  }
  function saveSettings(){const endpoint=$('#model-endpoint').value,model=$('#model-id').value;
    if(!state.tested||!validConfig(endpoint,model)||state.testedConfig?.endpoint!==endpoint||state.testedConfig?.model!==model){notify('模型尚未通过模拟测试。可以关闭设置，继续记录和搜索。');return;}
    state.endpoint=endpoint;state.model=model;state.configured=true;$('#settings-dialog').close();notify('演示设置已应用。');
  }
  function updateService(){$('#service-label').textContent=state.paused?'Memivy 已暂停':'Memivy 可用';$('#service-dot').classList.toggle('paused',state.paused);}
  function openMenu(){openDialog('menu-dialog',`<div class="dialog-header"><h2 id="menu-title">memivy <span class="dialog-eyebrow">菜单栏示意</span></h2>${closeButton()}</div><div class="menu-content"><div class="menu-status"><span class="status-dot ${state.paused?'paused':''}"></span>${state.paused?'已暂停 · MCP 不接受请求':'可用 · 随时接住一个想法'}</div><button class="menu-item" data-action="menu-capture">${icon('plus')}记下一刻<kbd>⌥ Space</kbd></button><button class="menu-item" data-action="close-dialog">${icon('window')}打开主窗口</button><hr class="menu-divider"><button class="menu-item" data-action="toggle-pause">${icon(state.paused?'play':'pause')}${state.paused?'恢复 Memivy':'暂停 Memivy'}</button><button class="menu-item" data-action="menu-settings">${icon('settings')}设置</button><p class="menu-note">仅浏览器示意，不读取系统状态。</p></div>`);}
  function exportMarkdown(){const content=state.memories.map(m=>`# ${m.title}\n\n${current(m)?.body||'（暂未整理）'}\n\n## 原话与来源\n\n${m.captures.map(c=>`### ${c.app} · ${c.time}\n\n${c.text}\n\n来源：${c.sourceLabel}${c.url?' · '+c.url:''}\n项目：${c.project}`).join('\n\n')}\n\n## 历史版本\n\n${m.versions.map(v=>`### v${v.number} · ${v.author} · ${v.time}\n\n${v.body}\n\n来源 ID：${v.sourceIds.join(', ')}`).join('\n\n')}`).join('\n\n---\n\n');const url=URL.createObjectURL(new Blob(['# Memivy Demo 示例导出\n\n此文件来自阶段 0 模拟数据。\n\n',content],{type:'text/markdown;charset=utf-8'}));const link=document.createElement('a');link.href=url;link.download='memivy-demo-memories.md';link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);notify('已导出示例 Markdown，包含原话与历史。');}
  function openAbout(){openDialog('about-dialog',`<div class="dialog-header"><h2 id="about-title">这一版，为什么这样设计</h2>${closeButton()}</div><div class="dialog-content"><span class="ai-badge">阶段 0 · v0.1 · 待你试用确认</span><h3>安静地记录，清楚地记得。</h3><p>保留 Miro 的白色画布、黑色胶囊按钮、黄色品牌和浅色提示。把视觉重心留给文字：列表方便扫读，当前记忆在上，原话来源紧接其后。</p><div class="about-color-row" aria-label="沿用 Miro 色板"><i style="background:#1c1c1e"></i><i style="background:#ffd02f"></i><i style="background:#fff4c4"></i><i style="background:#c3faf5"></i><i style="background:#fde0f0"></i><i style="background:#ffc6c6"></i></div><h3>借鉴好用的部分</h3><p>flomo 的轻输入、mymind 的留白、Bear 的列表与正文层级、Mem 的版本状态和纠正入口、Capacities 的关键词命中预览。不同于图片收藏墙，Memivy 需要让长中文和原话都容易阅读。</p><div class="reference-links"><a href="https://help.flomoapp.com/basic/quick-input.html" target="_blank" rel="noopener">flomo · 快捷记录 ↗</a><a href="https://mymind.com/the-new-quick-note" target="_blank" rel="noopener">mymind · Quick Note ↗</a><a href="https://bear.app/" target="_blank" rel="noopener">Bear · 正文排版 ↗</a><a href="https://help.mem.ai/features/clean-up" target="_blank" rel="noopener">Mem · AI 纠正 ↗</a><a href="https://docs.capacities.io/reference/search" target="_blank" rel="noopener">Capacities · 搜索 ↗</a></div><h3>信任放在动作里</h3><p>回执展示做了什么、放到了哪里、如何撤销。搜索不用 AI；原话与可编辑的当前版本分开。按钮保留文字，键盘焦点可见，减少动态效果的系统设置也会生效。</p><div class="reference-links"><a href="https://www.microsoft.com/en-us/research/project/guidelines-for-human-ai-interaction/" target="_blank" rel="noopener">Microsoft · 人与 AI 的交互 ↗</a><a href="https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/" target="_blank" rel="noopener">W3C · 对话框与焦点 ↗</a></div><h3>本次能验证什么</h3><p>可点击的布局、阅读、捕捉、回执、纠正、历史与设置。固定示例与关键词规则模拟 AI；新建与续接按原文展示，不代表模型理解质量。数据仅保留在当前页面，刷新重置；不连接模型、数据库、MCP 或系统快捷键。</p><p>字体优先使用规范中的 Roobert PRO；本机没有该字体时使用 macOS 系统字体与苹方中文。窄窗用于查看 Mac 缩窄时的表现，不代表支持手机端。</p></div><div class="dialog-footer"><button class="button primary" data-action="close-dialog">继续体验</button></div>`);}
  function reset(){state.jobs.forEach(clearTimeout);state.jobs.clear();clearTimeout(receiptTimer);state.memories=clone(window.MEMIVY_SEED);state.selected='first-experience';state.query='';state.filter='all';state.scenario='everyday';state.drafts={};state.captureDraft='';state.receipt=null;state.paused=false;state.configured=true;state.tested=true;state.mcp=false;state.emptyBackup=null;state.operations={};state.pendingDeleteId=null;state.testedConfig=null;state.endpoint='http://localhost:11434/v1';state.model='your-model';$('#search').value='';$('#scenario').value='everyday';$('#receipt').hidden=true;$('#last-receipt').hidden=true;$$('dialog[open]').forEach(d=>d.close());render();$('#detail-scroll').scrollTop=0;notify('已恢复初始示例。');}
  function setScenario(value){
    if(state.emptyBackup&&value!=='empty'){state.memories=state.emptyBackup;state.emptyBackup=null;}
    state.scenario=value;
    if(value==='empty'){state.jobs.forEach(clearTimeout);state.jobs.clear();state.emptyBackup=state.memories;state.memories=[];state.selected=null;clearSearch();collapseReceipt();return;}
    if(value==='no-results'){state.query='火星上的咖啡馆';$('#search').value=state.query;render();return;}
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
      'toggle-pause':()=>{state.paused=!state.paused;updateService();$('#menu-dialog').close();if(!state.paused)Object.values(state.operations).filter(op=>op.kind==='paused').forEach(retry);notify(state.paused?'Memivy 已暂停，MCP 不接受请求。手动记录和搜索仍可用。':'Memivy 已恢复。');},
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
