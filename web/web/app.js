/* 贴贴 Windows v0.4.1: UI state is ephemeral; every saved record is stored by Rust. */
(() => {
  'use strict';
  const api = window.__TAURI__;
  const $ = (sel) => document.querySelector(sel);
  const invoke = (name, args = {}) => api.core.invoke(name, args);
  const html = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({'&':'&amp;', '<':'&lt;', '>':'&gt;', '"':'&quot;', "'":'&#39;'}[c]));
  const id = () => (crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`);
  const fmtTime = (n) => new Date(n).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12:false });
  const fmtDate = (n) => new Date(n).toLocaleDateString('zh-CN', {year:'numeric',month:'2-digit',day:'2-digit'});
  const titleOf = (n) => n.kind === 'todo' ? (n.tasks[0]?.text || '待办清单') : (n.text?.split('\n').find(x=>x.trim())?.trim().slice(0,40) || (n.kind === 'image' ? '未命名图片' : '一条文字便签'));
  const normal = ['待办清单正在连夜申请工伤。', '任务已被你无情消灭，建议剩下的待办主动投降。', '你完成的不是任务，是对拖延症的一次精准打击。', '老板看了都想给你颁发一头生产队的驴。'];
  const epic = ['NASA 刚打来电话，想借用你的执行力去推火箭。', '联合国紧急开会，讨论今天要不要设成你的纪念日。', '这效率，生产队的驴看了都沉默。', '你负责创造奇迹，待办清单负责瑟瑟发抖。'];
  const rare = ['警告：生产力突破本软件理论上限，开发者请求停火。', '生产力 +999，工资 +0。建议与老板友好协商。', '检测到超越 99.99% 的碳基生物。剩下的 0.01% 是蚂蚁。'];

  let notes = [];
  let currentView = 'capture';
  let kind = 'text';
  let currentId = null;
  let currentTasks = [];
  let draftToken = 0;
  let autoTimer = null;
  let chain = Promise.resolve();
  let searchKind = 'all';
  let searchRange = 'all';
  let toastTimer = null;
  const imageCache = new Map();

  function toast(message, action) {
    const el = $('#toast'); clearTimeout(toastTimer); el.replaceChildren();
    el.append(document.createTextNode(message));
    if (action) { const button = document.createElement('button'); button.textContent = action.label; button.onclick = action.callback; el.append(button); }
    el.classList.remove('hidden');
    toastTimer = setTimeout(() => el.classList.add('hidden'), action ? 5500 : 2600);
  }
  function fail(error) { console.error(error); toast(String(error).replace(/^Error:\s*/, '')); }
  function setStatus(status) { $('#status').textContent = status; }
  function queue(action) { chain = chain.catch(() => {}).then(action); return chain; }
  async function load() {
    notes = await invoke('list_notes');
    if (currentView === 'history') renderHistory();
  }
  function setKind(next) {
    kind = next;
    $('#textComposer').classList.toggle('hidden', next === 'todo');
    $('#todoComposer').classList.toggle('hidden', next !== 'todo');
    $('#modeText').classList.toggle('active', next !== 'todo');
    $('#modeTodo').classList.toggle('active', next === 'todo');
    $('#pasteBtn').disabled = next === 'todo';
    if (next === 'todo') renderTasks();
  }
  function resetDraft() {
    draftToken++; clearTimeout(autoTimer); currentId = null; currentTasks = [];
    $('#noteText').value = ''; $('#imageWrap').classList.add('hidden'); $('#previewImage').removeAttribute('src');
    $('#sheetLabel').textContent = '一张新的小纸片'; $('#todoInput').value = '';
    setStatus('自动保存'); setKind('text');
  }
  function scheduleSave() { setStatus('正在写入…'); clearTimeout(autoTimer); autoTimer = setTimeout(() => { flushDraft().catch(fail); }, 550); }
  async function flushDraft() {
    clearTimeout(autoTimer);
    const token = draftToken;
    if (kind === 'todo') {
      const tasks = structuredClone(currentTasks);
      if (!currentId && !tasks.length) return;
      await queue(async () => {
        const saved = await invoke('save_todo', {id: currentId, tasks});
        if (token === draftToken) {currentId = saved.id; setStatus('✓ 已存到本地');}
      });
    } else {
      const text = $('#noteText').value;
      if (!currentId && !text.trim()) return;
      await queue(async () => {
        const saved = await invoke('save_text', {id: currentId, text});
        if (token === draftToken) {currentId = saved.id; setStatus('✓ 已存到本地');}
      });
    }
  }
  async function commitAndReset() {
    try { await flushDraft(); if (!currentId) {toast('写一点内容或者粘贴图片吧 ✎'); return;} resetDraft(); await load(); toast('已轻轻收好 ✧'); $('#noteText').focus(); }
    catch(e) { fail(e); }
  }
  async function switchView(next) {
    try { if (currentView === 'capture') await flushDraft(); }
    catch (e) {fail(e);return;}
    currentView = next;
    $('#captureView').classList.toggle('hidden', next !== 'capture');
    $('#historyView').classList.toggle('hidden', next !== 'history');
    $('#captureTab').classList.toggle('selected', next === 'capture');
    $('#historyTab').classList.toggle('selected', next === 'history');
    if (next === 'history') {await load();$('#searchInput').focus();} else $('#noteText').focus();
  }
  async function openNote(note) {
    try {
      if (currentView === 'capture') await flushDraft();
      resetDraft(); currentId = note.id;
      $('#noteText').value = note.text || '';
      if (note.kind === 'todo') {
        currentTasks = structuredClone(note.tasks); setKind('todo'); $('#sheetLabel').textContent = '待办事项';
      } else {
        setKind('text'); $('#sheetLabel').textContent = note.kind === 'image' ? '图片和随手备注' : '继续编辑这张纸片';
        if (note.kind === 'image') {
          $('#imageWrap').classList.remove('hidden');
          const img = $('#previewImage');
          img.removeAttribute('src');
          imageData(note.id).then(src=> {if(currentId===note.id) img.src=src;}).catch(fail);
        }
      }
      currentView = 'history';
      await switchView('capture');
    } catch(e) {fail(e);}
  }
  async function applyPastedNote(note) {
    currentId = note.id;
    $('#noteText').value = note.text || '';
    if (note.kind === 'image') {
      $('#sheetLabel').textContent = '图片和随手备注';
      $('#imageWrap').classList.remove('hidden');
      imageCache.delete(note.id);
      $('#previewImage').src = await imageData(note.id);
      toast('图片已长期保存到本地 ✨');
    } else {
      toast('文字已收藏 ✧');
    }
    setStatus('✓ 已存到本地');
    await load();
  }
  async function pasteClipboard() {
    if (kind === 'todo') {toast('在待办输入框中直接 Ctrl+V 粘贴文字');return;}
    try {
      await flushDraft();
      const note = await invoke('capture_clipboard', { targetId: currentId });
      await applyPastedNote(note);
    } catch (e) {fail(e);}
  }
  function imageFileAsDataUrl(file) {
    return new Promise((resolve,reject)=>{
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.onerror = () => reject(new Error('无法读取粘贴的图片数据'));
      reader.readAsDataURL(file);
    });
  }
  // When the user actually presses Ctrl+V inside WebView2, prefer the
  // ClipboardEvent's image Blob. This works even if Rust arboard rejects the
  // source app's CF_DIB/BI_BITFIELDS image representation.
  async function pasteImageFromEvent(file) {
    try {
      if (file.size > 45_000_000) throw Error('图片超过 45MB 限制');
      const data = await imageFileAsDataUrl(file);
      await flushDraft();
      const note = await invoke('paste_image_data', { targetId: currentId, imageData: data });
      await applyPastedNote(note);
    } catch(e) {
      fail(e);
    }
  }
  async function imageData(id) {
    if (!imageCache.has(id)) imageCache.set(id, invoke('get_image_data', {id}).catch(e => {imageCache.delete(id);throw e;}));
    return imageCache.get(id);
  }
  function renderTasks() {
    const container=$('#todoItems'); container.replaceChildren();
    if (!currentTasks.length) {
      const hint=document.createElement('div');hint.style.cssText='color:#adb2ad;font-size:12px;padding:18px 0';hint.textContent='先写下一件想完成的小事…';container.append(hint);return;
    }
    currentTasks.forEach((task,i)=>{
      const row=document.createElement('div');row.className='todo-row';
      const input=document.createElement('input');input.type='checkbox';input.checked=task.done;input.title='标记完成';
      const label=document.createElement('span');label.textContent=task.text;label.classList.toggle('done',task.done);
      input.onchange=()=>{
        task.done=input.checked;task.doneAt=input.checked?Date.now():null;
        renderTasks();scheduleSave();if (input.checked) celebrate();
      };
      const remove=document.createElement('button');remove.className='remove-task';remove.title='移除这项';remove.textContent='×';
      remove.onclick=()=>{currentTasks.splice(i,1);renderTasks();scheduleSave();};
      row.append(input,label,remove);container.append(row);
    });
  }
  function celebrate() {
    try { if (localStorage.getItem('tietie-celebrate') === 'off') return; } catch (_) {}
    const p=Math.random(), collection=p<.02?rare:p<.25?epic:normal;
    toast('✦ '+collection[Math.floor(Math.random()*collection.length)]);
    const fx=$('#confetti');fx.classList.remove('hidden');void fx.offsetWidth;fx.style.animation='none';void fx.offsetWidth;fx.style.animation='';
    setTimeout(()=>fx.classList.add('hidden'),850);
  }
  function addTask() {
    const value=$('#todoInput').value.trim();if (!value)return;
    currentTasks.push({id:id(),text:value,done:false,doneAt:null});
    $('#todoInput').value='';renderTasks();scheduleSave();$('#todoInput').focus();
  }
  function localDay(ms) {
    const d=new Date(ms);return `${d.getFullYear()}-${String(d.getMonth()+1).padStart(2,'0')}-${String(d.getDate()).padStart(2,'0')}`;
  }
  function matchDate(n) {
    const day = new Date(n.createdAt), today=new Date(), start = new Date(today.getFullYear(),today.getMonth(),today.getDate());
    if (searchRange === 'today') return day >= start;
    if (searchRange === 'week') return day >= new Date(start.getTime()-6*86400000);
    if (searchRange === 'month') return day >= new Date(today.getFullYear(),today.getMonth(),1);
    if (searchRange === 'custom') {
      const from = $('#startDate').value, to = $('#endDate').value, date=localDay(n.createdAt);
      return (!from || date >= from) && (!to || date <= to);
    }
    return true;
  }
  function renderHistory() {
    const term=$('#searchInput').value.trim().toLocaleLowerCase();
    const filtered=notes.filter(n=>{
      if (searchKind!=='all' && n.kind!==searchKind) return false;
      if (!matchDate(n))return false;
      const searchable=[n.text||'',...(n.tasks||[]).map(t=>t.text),titleOf(n)].join(' ').toLocaleLowerCase();
      return searchable.includes(term);
    });
    $('#resultCount').textContent=filtered.length+' 条';
    const list=$('#historyList');list.replaceChildren();
    if (!filtered.length) {list.innerHTML='<div class="empty"><span class="empty-symbol">✧</span>还没找到这张纸片。<br>试试换个关键词或时间范围吧。</div>';return;}
    let lastDay='';
    for(const note of filtered.slice(0,250)) {
      const day=localDay(note.createdAt);
      if (day!==lastDay) {
        const heading=document.createElement('div');heading.className='day-title';
        const today=localDay(Date.now());
        heading.textContent=(day===today?'今天 · ':day===localDay(Date.now()-86400000)?'昨天 · ':'')+day;
        list.append(heading);lastDay=day;
      }
      const row=document.createElement('div');row.className='record';row.tabIndex=0;row.title='点击继续编辑';
      const thumb=document.createElement('div');thumb.className='thumb '+note.kind;
      if(note.kind==='image') {
        const img=document.createElement('img');img.alt='截图缩略图';thumb.append(img);
        imageData(note.id).then(src=>{if(img.isConnected)img.src=src;}).catch(()=>{if(thumb.isConnected)thumb.textContent='▧';});
      } else thumb.textContent=note.kind==='todo'?'☑':'✎';
      const desc=document.createElement('div');desc.className='record-copy';
      const title=document.createElement('div');title.className='record-title';title.textContent=titleOf(note);
      const sub=document.createElement('div');sub.className='record-sub';
      sub.textContent=(note.kind==='todo'?`${note.tasks.filter(t=>t.done).length}/${note.tasks.length} 已完成`:note.kind==='image'?'图片':'文字')+' · '+fmtTime(note.createdAt);
      desc.append(title,sub);
      const actions=document.createElement('div');actions.className='record-actions';
      const pin=document.createElement('button');pin.title=note.visible?'收起桌面浮贴':'贴到桌面';pin.textContent=note.visible?'◈':'♧';pin.classList.toggle('pinned',note.visible);
      pin.onclick=async e=>{e.stopPropagation();try {if (note.visible)await invoke('hide_float',{id:note.id});else await invoke('open_float',{id:note.id});await load();}catch(err){fail(err);}};
      const remove=document.createElement('button');remove.title='移到回收站（可撤销）';remove.textContent='×';
      remove.onclick=async e=>{
        e.stopPropagation();if(!confirm('把这张纸片放入回收站吗？可以立即撤销。'))return;
        try{await invoke('delete_note',{id:note.id});await load();toast('已放入回收站 · ',{label:'撤销',callback:async()=>{try{await invoke('restore_note',{id:note.id});await load();toast('已找回来 ✦');}catch(err){fail(err);}}});}catch(err){fail(err);}
      };
      actions.append(pin,remove);row.append(thumb,desc,actions);
      row.addEventListener('click',()=>openNote(note));
      row.addEventListener('keydown',e=>{if(e.key==='Enter')openNote(note)});
      list.append(row);
    }
    if(filtered.length>250){let hint=document.createElement('div');hint.className='day-title';hint.textContent='仅显示前 250 条，请缩小搜索范围';list.append(hint);}
  }
  async function hidePanel() {
    try {await flushDraft(); resetDraft(); await invoke('hide_panel');}catch(e){fail(e);}
  }
  function onBlankDblClick(e) {
    if(e.target.closest('button,input,textarea,.record,.sheet,.shortcut-card'))return;
    hidePanel();
  }

  $('#noteText').addEventListener('input',scheduleSave);
  document.addEventListener('paste', e => {
    if (currentView !== 'capture') return;
    const imageItem = Array.from(e.clipboardData?.items || []).find(item => item.type.startsWith('image/'));
    if (imageItem && kind !== 'todo') {
      e.preventDefault();
      const file = imageItem.getAsFile();
      if (file) pasteImageFromEvent(file);
      else pasteClipboard(); // Image is advertised but WebView did not provide bytes.
    } else if (kind !== 'todo' && !e.target.closest('textarea, input, [contenteditable]')) {
      // Allow Ctrl+V even if the user clicked the note's non-editable area.
      e.preventDefault();
      pasteClipboard();
    }
    // Editing text and Todo retains the standard selection/caret paste behavior.
  });
  $('#todoInput').addEventListener('keydown',e=>{if(e.key==='Enter'){e.preventDefault();addTask();}});
  $('#todoItems').addEventListener('dblclick',e=>e.stopPropagation());
  $('#captureTab').onclick=()=>switchView('capture').catch(fail);
  $('#historyTab').onclick=()=>switchView('history').catch(fail);
  $('#newNote').onclick=async()=>{try{await flushDraft();resetDraft();await switchView('capture');}catch(e){fail(e);}};
  $('#hideBtn').onclick=hidePanel;
  $('#dragRegion').addEventListener('pointerdown',e=>{
    if(e.button!==0||e.target.closest('button,.kbd'))return;
    const startX=e.clientX,startY=e.clientY;
    function move(ev){if(Math.abs(ev.clientX-startX)+Math.abs(ev.clientY-startY)>6){cleanup();api.window.getCurrentWindow().startDragging().catch(()=>{});}}
    function cleanup(){window.removeEventListener('pointermove',move);window.removeEventListener('pointerup',cleanup);}
    window.addEventListener('pointermove',move);window.addEventListener('pointerup',cleanup,{once:true});
  });
  $('#panel').addEventListener('dblclick',onBlankDblClick);
  $('#modeText').onclick=async()=>{
    if(kind==='todo') {try{await flushDraft();resetDraft();}catch(e){fail(e);return;}}setKind('text');$('#noteText').focus();
  };
  $('#modeTodo').onclick=async()=>{
    if(kind!=='todo') {try{await flushDraft();resetDraft();}catch(e){fail(e);return;}}setKind('todo');$('#todoInput').focus();
  };
  $('#pasteBtn').onclick=pasteClipboard;
  $('#finishBtn').onclick=commitAndReset;
  $('#searchInput').oninput=renderHistory;
  $('#startDate').oninput=renderHistory;$('#endDate').oninput=renderHistory;
  $('#dateChips').onclick=e=>{
    const chip=e.target.closest('button[data-range]');if(!chip)return;
    searchRange=chip.dataset.range;
    $('#dateChips').querySelectorAll('button').forEach(b=>b.classList.toggle('selected',b===chip));
    $('#customDates').classList.toggle('hidden',searchRange!=='custom');renderHistory();
  };
  $('#typeChips').onclick=e=>{
    const chip=e.target.closest('button[data-type]');if(!chip)return;
    searchKind=chip.dataset.type;
    $('#typeChips').querySelectorAll('button').forEach(b=>b.classList.toggle('selected',b===chip));renderHistory();
  };
  $('#dataPathBtn').onclick=async()=>{try{const path=await invoke('get_data_directory');toast('本地数据目录：'+path);}catch(e){fail(e);}};
  document.addEventListener('visibilitychange',()=>{if(document.hidden)flushDraft().catch(console.error);});
  window.addEventListener('blur',()=>flushDraft().catch(console.error));
  document.addEventListener('keydown',e=>{
    if(e.key==='Escape') {e.preventDefault();hidePanel();}
    if(e.ctrlKey&&e.key==='Enter'&&currentView==='capture'){e.preventDefault();commitAndReset();}
  });
  // Main window is not the user's only way into the app: global shortcuts and tray live in Rust.
  if(!api?.core?.invoke) {toast('请通过 Windows 桌面程序启动贴贴（浏览器中无法调用本地功能）');return;}
  api.event.listen('notes-changed',()=>load().catch(fail));
  api.event.listen('panel-shown',()=>{if(currentView==='capture')$('#noteText').focus();else $('#searchInput').focus();load().catch(fail);});
  api.event.listen('capture-feedback',event=>{toast(event.payload);load().catch(fail);});
  api.event.listen('open-note',async event=>{try{await load();const n=notes.find(n=>n.id===event.payload);if(n)await openNote(n);}catch(e){fail(e);}});
  load().then(async()=>{
    const status=await invoke('get_hotkeys');
    if(!status.panel||!status.capture)toast('部分快捷键被其他应用占用。可用托盘右键菜单操作。');
    $('#noteText').focus();
  }).catch(fail);
})();
