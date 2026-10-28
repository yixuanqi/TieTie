/* Each desktop float is its own native WebView2-backed Tauri window. */
(() => {
  'use strict';
  const api=window.__TAURI__;
  const $=s=>document.querySelector(s);
  const invoke=(name,args={})=>api.core.invoke(name,args);
  const windowRef=api.window.getCurrentWindow();
  const noteId=windowRef.label.replace(/^float_/, '');
  let note=null,geometryTimer=null, toastTimer=null;
  function toast(msg) { const node=$('#floatToast');node.textContent=msg;node.classList.remove('hidden');clearTimeout(toastTimer);toastTimer=setTimeout(()=>node.classList.add('hidden'),2400); }
  function praise() {
    const messages=['待办清单正在连夜申请工伤。','NASA 发来贺电：这效率是真实存在的吗？','剩下的待办已进入紧急戒备状态。','你的执行力已经触发宇宙级警报。'];
    toast('✦ '+messages[Math.floor(Math.random()*messages.length)]);
  }
  async function refresh() {
    const notes=await invoke('list_notes');
    const found=notes.find(n=>n.id===noteId);
    if(!found) { await windowRef.hide();return; }
    note=found;
    const card=$('#floatCard'), area=$('#floatContent');
    card.classList.toggle('kind-image',note.kind==='image');card.classList.toggle('kind-todo',note.kind==='todo');
    card.style.setProperty('--note-opacity',String(note.opacity));
    $('#topBtn').textContent=note.alwaysOnTop?'♧':'◇';$('#topBtn').title=note.alwaysOnTop?'已置顶，点击取消':'点击置顶';
    $('#opacityValue').textContent=Math.round(note.opacity*100)+'%';
    $('#opacityRange').value=Math.round(note.opacity*100);
    $('#floatDate').textContent=new Date(note.createdAt).toLocaleDateString('zh-CN');
    area.replaceChildren();
    if(note.kind==='image') {
      const wrap=document.createElement('div');wrap.className='image-holder';
      const img=document.createElement('img');img.alt='收藏的截图';
      invoke('get_image_data',{id:note.id}).then(src=>{img.src=src}).catch(()=>{img.alt='图片加载失败';});
      wrap.append(img);
      if(note.text.trim()){const caption=document.createElement('div');caption.className='caption';caption.textContent=note.text;wrap.append(caption);}
      area.append(wrap);
    } else if(note.kind==='todo') {
      if(!note.tasks.length)area.textContent='还没有待办';
      note.tasks.forEach((task,i)=>{
        const row=document.createElement('label');row.className='float-task';
        const chk=document.createElement('input');chk.type='checkbox';chk.checked=task.done;
        const desc=document.createElement('span');desc.textContent=task.text;desc.classList.toggle('done',task.done);
        chk.onchange=async()=>{
          const tasks=structuredClone(note.tasks);tasks[i].done=chk.checked;tasks[i].doneAt=chk.checked?Date.now():null;
          try{await invoke('save_todo',{id:note.id,tasks});await refresh();if(chk.checked)praise();}catch(e){toast(String(e));}
        };
        row.append(chk,desc);area.append(row);
      });
    } else {
      const div=document.createElement('div');div.className='text-body';div.textContent=note.text;area.append(div);
    }
  }
  async function hide() { try {await invoke('hide_float',{id:noteId});}catch(e){toast(String(e));} }
  function saveGeometry() {
    clearTimeout(geometryTimer);
    geometryTimer=setTimeout(async()=>{
      try{
        const pos=await windowRef.outerPosition(),size=await windowRef.innerSize();
        const factor=await windowRef.scaleFactor();
        await invoke('update_float',{id:noteId,x:Math.round(pos.x/factor),y:Math.round(pos.y/factor),w:Math.round(size.width/factor),h:Math.round(size.height/factor),opacity:null,alwaysOnTop:null});
      }catch(e){console.error(e);}
    },700);
  }
  $('#floatDrag').addEventListener('pointerdown',e=>{
    if(e.button!==0||e.target.closest('button'))return;
    const sx=e.clientX,sy=e.clientY;
    function move(ev){if(Math.abs(ev.clientX-sx)+Math.abs(ev.clientY-sy)>6){cleanup();windowRef.startDragging().catch(()=>{});}}
    function cleanup(){window.removeEventListener('pointermove',move);window.removeEventListener('pointerup',cleanup);}
    window.addEventListener('pointermove',move);window.addEventListener('pointerup',cleanup,{once:true});
  });
  $('#floatDrag').addEventListener('dblclick',e=>{if(!e.target.closest('button'))hide();});
  $('#floatContent').addEventListener('dblclick',e=>{if(!e.target.closest('input,label,button'))hide();});
  $('#hideFloat').onclick=hide;
  $('#topBtn').onclick=async()=>{try{note=await invoke('update_float',{id:noteId,x:null,y:null,w:null,h:null,opacity:null,alwaysOnTop:!note.alwaysOnTop});await refresh();}catch(e){toast(String(e));}};
  $('#opacityBtn').onclick=()=>$('#opacityPopover').classList.toggle('hidden');
  $('#opacityRange').oninput=e=>{
    const value=Number(e.target.value)/100;
    $('#opacityValue').textContent=e.target.value+'%';
    $('#floatCard').style.setProperty('--note-opacity',String(value));
    note.opacity=value;
  };
  $('#opacityRange').onchange=e=>{
    invoke('update_float',{id:noteId,x:null,y:null,w:null,h:null,opacity:Number(e.target.value)/100,alwaysOnTop:null}).catch(err=>toast(String(err)));
  };
  $('#floatContent').addEventListener('contextmenu',e=>{
    e.preventDefault();
    const menu=$('#floatMenu');menu.classList.remove('hidden');
    menu.style.left=Math.min(e.clientX,Math.max(5,document.body.clientWidth-183))+'px';
    menu.style.top=Math.min(e.clientY,Math.max(5,document.body.clientHeight-176))+'px';
  });
  document.addEventListener('pointerdown',e=>{if(!e.target.closest('#floatMenu'))$('#floatMenu').classList.add('hidden');});
  $('#menuEdit').onclick=()=>invoke('show_note_in_panel',{id:noteId}).catch(e=>toast(String(e)));
  $('#menuPin').onclick=()=>$('#topBtn').click();
  $('#menuOpacity').onclick=()=>$('#opacityBtn').click();
  $('#menuHide').onclick=hide;
  document.addEventListener('keydown',e=>{if(e.key==='Escape')hide();});
  windowRef.onMoved(saveGeometry);windowRef.onResized(saveGeometry);
  api.event.listen('notes-changed',()=>refresh().catch(console.error));
  refresh().catch(e=>toast(String(e)));
})();
