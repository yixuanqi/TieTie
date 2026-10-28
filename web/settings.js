/* Small independent settings window, backed by native Rust preferences. */
(() => {
  'use strict';
  const api = window.__TAURI__;
  const $ = id => document.getElementById(id);
  const invoke = (command, args={}) => api.core.invoke(command, args);
  const nativeWindow = api.window.getCurrentWindow();
  let initial = null;
  let draft = null;
  let recording = null;
  const shortcutText = str => str.replaceAll('+', ' + ');
  function feedback(message, error=false) {
    $('settingsFeedback').textContent = message;
    $('settingsFeedback').classList.toggle('error', error);
  }
  function paint() {
    if (!draft) return;
    document.documentElement.dataset.theme = draft.theme;
    $('themeLight').classList.toggle('selected', draft.theme==='light');
    $('themeDark').classList.toggle('selected', draft.theme==='dark');
    $('panelHotkey').textContent = recording==='panel' ? '请按组合键…' : shortcutText(draft.panelShortcut);
    $('captureHotkey').textContent = recording==='capture' ? '请按组合键…' : shortcutText(draft.captureShortcut);
    document.querySelectorAll('.key-capture').forEach(el=>el.classList.toggle('recording',el.dataset.target===recording));
  }
  async function load() {
    try {
      initial = await invoke('get_preferences');
      draft = {...initial}; paint();
      feedback('修改后点击保存');
    } catch(err) { feedback(String(err),true); }
  }
  function stopRecording() { recording=null;paint(); }
  document.querySelectorAll('.key-capture').forEach(button=>button.addEventListener('click',()=>{
    if(!draft) return;
    recording=button.dataset.target;
    feedback('正在录制：请同时按住 Ctrl / Alt 和一个按键');
    paint();
  }));
  document.addEventListener('keydown', event=>{
    if(!recording) return;
    event.preventDefault();event.stopPropagation();
    if(event.key==='Escape'){stopRecording();feedback('已取消录制');return;}
    if(['Shift','Control','Alt','Meta'].includes(event.key))return;
    if(!event.ctrlKey && !event.altKey){feedback('至少按住 Ctrl 或 Alt',true);return;}
    if(event.metaKey){feedback('暂不支持 Win 键，避免系统快捷键冲突',true);return;}
    const k=event.key.toUpperCase();
    if(!(/^[A-Z0-9]$/.test(k) || /^F([1-9]|1[0-2])$/.test(k))) {
      feedback('仅支持 A-Z、0-9 和 F1-F12',true);return;
    }
    const combo=[event.ctrlKey?'Ctrl':null,event.altKey?'Alt':null,event.shiftKey?'Shift':null,k].filter(Boolean).join('+');
    if(recording==='panel')draft.panelShortcut=combo;
    else draft.captureShortcut=combo;
    stopRecording(); feedback('快捷键已录入，点击保存才会生效');
  },true);
  document.querySelectorAll('.theme-choice').forEach(button=>button.addEventListener('click',()=>{
    if(!draft)return;
    draft.theme=button.dataset.theme;
    paint();feedback('主题预览中，保存后应用到所有便签');
  }));
  $('saveSettings').onclick=async()=>{
    if(!draft)return;
    if(draft.panelShortcut===draft.captureShortcut){feedback('两个快捷键不能相同',true);return;}
    $('saveSettings').disabled=true;
    try {
      initial=await invoke('save_preferences', {
        theme:draft.theme,panelShortcut:draft.panelShortcut,captureShortcut:draft.captureShortcut
      });
      draft={...initial};feedback('✓ 已保存，立即生效');paint();
    } catch(err) {feedback(String(err).replace(/^Error:\s*/,''),true);}
    finally {$('saveSettings').disabled=false;}
  };
  async function close() { try {await invoke('hide_settings');}catch(e){console.error(e);} }
  $('closeSettings').onclick=close;
  $('cancelSettings').onclick=()=>{if(initial){draft={...initial};paint();}close();};
  document.addEventListener('keydown', e=>{if(e.key==='Escape' && !recording)close();});
  $('settingsDrag').addEventListener('pointerdown', e=>{
    if(e.button!==0 || e.target.closest('button'))return;
    const sx=e.clientX,sy=e.clientY;
    function move(ev){if(Math.abs(ev.clientX-sx)+Math.abs(ev.clientY-sy)>5){cleanup();nativeWindow.startDragging().catch(console.error);}}
    function cleanup(){window.removeEventListener('pointermove',move);window.removeEventListener('pointerup',cleanup);}
    window.addEventListener('pointermove',move);window.addEventListener('pointerup',cleanup,{once:true});
  });
  api.event.listen('preferences-changed', e=>{initial=e.payload;draft={...initial};paint();}).catch(console.error);
  load();
})();
