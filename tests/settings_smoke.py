"""Settings/UI-level regression tests with mock native bridge (not Windows hardware validation)."""
from pathlib import Path
from playwright.sync_api import sync_playwright

base=Path(__file__).resolve().parents[1]
styles=(base/'web/styles.css').read_text()
settings=(base/'web/settings.html').read_text().replace('<link rel="stylesheet" href="styles.css">',f'<style>{styles}</style>').replace('<script defer src="settings.js"></script>','')
settings=settings.replace('</body>',f'<script>{(base/"web/settings.js").read_text()}</script></body>')
main=(base/'web/index.html').read_text().replace('<link rel="stylesheet" href="styles.css">',f'<style>{styles}</style>').replace('<script defer src="app.js"></script>','')
main=main.replace('</body>',f'<script>{(base/"web/app.js").read_text()}</script></body>')
prefs='''(() => {
 let settings={theme:'light',panelShortcut:'Ctrl+Alt+N',captureShortcut:'Ctrl+Alt+V',panelWidth:520,panelHeight:670};
 let listeners={};
 window.__native={get prefs(){return structuredClone(settings)},get listeners(){return listeners},resizeCalls:[], settingsOpened:false, resized:null};
 window.__TAURI__={core:{invoke:async (command,arg={})=>{
  if(command==='get_preferences')return structuredClone(settings);
  if(command==='save_preferences'){
   if(arg.panelShortcut===arg.captureShortcut)throw Error('快捷键不能相同');
   settings={...settings,...arg};
   for(let cb of listeners['preferences-changed']||[])cb({payload:structuredClone(settings)});
   return structuredClone(settings);
  }
  if(command==='hide_settings')return;
  if(command==='open_settings'){window.__native.settingsOpened=true;return;}
  if(command==='save_panel_size'){window.__native.resizeCalls.push(arg);return;}
  if(command==='list_notes')return [];
  if(command==='get_hotkeys')return {panel:true,capture:true};
  if(command==='hide_panel')return;
  throw Error('unknown '+command);
 }},event:{listen:async (k,fn)=>{(listeners[k]??=[]).push(fn)}},window:{getCurrentWindow:()=>({
 startDragging:async()=>{},startResizeDragging:async(direction)=>{window.__native.resizeDirection=direction},
 onResized:async(cb)=>{window.__native.resized=cb},
 innerSize:async()=>({width:1020,height:920}),scaleFactor:async()=>2
 })}};
})();'''
with sync_playwright() as p:
  browser=p.chromium.launch(headless=True,executable_path='/usr/bin/chromium',args=['--no-sandbox'])
  page=browser.new_page(viewport={'width':450,'height':530},device_scale_factor=1)
  errors=[]
  page.on('pageerror',lambda e: errors.append(str(e)))
  page.goto('about:blank')
  page.evaluate(prefs)
  page.set_content(settings)
  page.wait_for_timeout(160)
  assert page.locator('#panelHotkey').inner_text()=='Ctrl + Alt + N'
  page.locator('#panelHotkey').click()
  page.keyboard.down('Control');page.keyboard.down('Alt');page.keyboard.press('Y');page.keyboard.up('Alt');page.keyboard.up('Control')
  assert 'Y' in page.locator('#panelHotkey').inner_text()
  page.locator('#themeDark').click()
  assert page.locator('html').get_attribute('data-theme') == 'dark'
  page.screenshot(path=str(base/'docs/screenshot_settings_dark.png'))
  page.locator('#saveSettings').click()
  page.wait_for_timeout(150)
  assert page.evaluate('window.__native.prefs.panelShortcut')=='Ctrl+Alt+Y'
  assert page.evaluate('window.__native.prefs.theme')=='dark'
  assert '已保存' in page.locator('#settingsFeedback').inner_text()
  # Theme preview and cancel must not persist.
  page.locator('#themeLight').click()
  page.locator('#cancelSettings').click()
  assert page.evaluate('window.__native.prefs.theme')=='dark'
  assert not errors,errors
  print('SETTINGS PASS: record hotkey; change theme; save; cancel does not persist; no JS errors')

  other=browser.new_page(viewport={'width':520,'height':670})
  other_errors=[]
  other.on('pageerror',lambda e: other_errors.append(str(e)))
  other.goto('about:blank');other.evaluate(prefs);other.set_content(main)
  other.wait_for_timeout(150)
  assert other.locator('#panelShortcutHint').inner_text() == 'Ctrl Alt N'
  assert other.locator('#resizeGrip').count() == 1
  other.locator('#resizeGrip').dispatch_event('pointerdown',{'button':0})
  other.wait_for_timeout(30)
  assert other.evaluate('window.__native.resizeDirection')=='SouthEast'
  other.evaluate('window.__native.resized()')
  other.wait_for_timeout(800)
  calls=other.evaluate('window.__native.resizeCalls')
  assert calls and calls[-1]=={'width':510,'height':460},calls
  other.locator('#settingsBtn').click()
  assert other.evaluate('window.__native.settingsOpened') is True
  assert not other_errors,other_errors
  print('MAIN PASS: resize handle, debounced size persistence, settings entry point, no JS errors')
  browser.close()
