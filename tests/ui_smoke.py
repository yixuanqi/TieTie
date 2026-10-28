"""UI smoke test with a Tauri API stub. This validates DOM workflows, not native Windows APIs."""
from pathlib import Path
import threading
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from functools import partial
from playwright.sync_api import sync_playwright

BASE = Path(__file__).resolve().parents[1]
INIT = r'''
(() => {
 let records=[];
 const pending={};
 const clone=x=>JSON.parse(JSON.stringify(x));
 const uid=()=>String(records.length+1)+"-"+Date.now();
 const emitter={};
 window.__testState={get notes(){return clone(records)}, get events(){return emitter}};
 window.__TAURI__={
   core:{invoke:async (name,args={})=>{
     if(name==='list_notes')return clone(records);
     if(name==='get_hotkeys')return {panel:true,capture:true};
     if(name==='get_data_directory')return 'C:\\Users\\Test\\AppData\\Roaming\\tietie';
     if(name==='hide_panel')return;
     if(name==='save_text'){
       let note=records.find(x=>x.id===args.id);
       if(!note){note={id:uid(),kind:'text',text:'',tasks:[],createdAt:Date.now(),updatedAt:Date.now(),visible:false,opacity:.96};records.unshift(note)}
       note.text=args.text;note.updatedAt=Date.now();return clone(note)
     }
     if(name==='save_todo'){
       let note=records.find(x=>x.id===args.id);
       if(!note){note={id:uid(),kind:'todo',text:'',tasks:[],createdAt:Date.now(),updatedAt:Date.now(),visible:false,opacity:.96};records.unshift(note)}
       note.tasks=clone(args.tasks);note.updatedAt=Date.now();return clone(note)
     }
     if(name==='paste_image_data'){
       if (!args.imageData.startsWith('data:image/')) throw Error('Expected image data URI');
       let note=records.find(x=>x.id===args.targetId);
       if(!note){note={id:uid(),kind:'image',text:'',tasks:[],createdAt:Date.now(),updatedAt:Date.now(),visible:false,opacity:.96};records.unshift(note)}
       note.kind='image';return clone(note)
     }
     if(name==='capture_clipboard'){
       let note=records.find(x=>x.id===args.targetId);
       if(!note){note={id:uid(),kind:'image',text:'',tasks:[],createdAt:Date.now(),updatedAt:Date.now(),visible:false,opacity:.96};records.unshift(note)}
       note.kind='image';return clone(note)
     }
     if(name==='get_thumbnail_data')return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4////fwAJ+wP+eYbvyAAAAABJRU5ErkJggg==';
     if(name==='get_image_data')return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4////fwAJ+wP+eYbvyAAAAABJRU5ErkJggg==';
     if(name==='open_float'){let n=records.find(x=>x.id===args.id);n.visible=true;return}
     if(name==='hide_float'){let n=records.find(x=>x.id===args.id);n.visible=false;return}
     if(name==='delete_note'){records=records.filter(n=>n.id!==args.id);return}
     if(name==='restore_note')return;
     throw Error('Unsupported test invoke '+name);
   }},
   event:{listen:async (name,cb)=>{(emitter[name]??=[]).push(cb);return()=>{};}},
   window:{getCurrentWindow:()=>({startDragging:async()=>{},hide:async()=>{}})}
 };
})();
'''
server = ThreadingHTTPServer(("127.0.0.1", 0), partial(SimpleHTTPRequestHandler, directory=str(BASE / "web")))
threading.Thread(target=server.serve_forever, daemon=True).start()
with sync_playwright() as p:
    browser = p.chromium.launch(headless=True, executable_path='/usr/bin/chromium', args=['--no-sandbox'])
    page = browser.new_page(viewport={'width':480, 'height':572}, device_scale_factor=1)
    errors=[]
    page.on('pageerror',lambda e: errors.append(str(e)))
    page.add_init_script(INIT)
    html_text=(BASE/'web/index.html').read_text()
    html_text=html_text.replace('<link rel="stylesheet" href="styles.css">','<style>'+ (BASE/'web/styles.css').read_text()+'</style>')
    html_text=html_text.replace('<script defer src="app.js"></script>','')
    html_text=html_text.replace('</body>','<script>'+ (BASE/'web/app.js').read_text()+'</script></body>')
    page.goto('about:blank')
    page.evaluate(INIT)
    page.set_content(html_text)
    page.locator('#noteText').fill('干涉仪条纹 FFT 测试结果')
    page.wait_for_timeout(700)
    assert len(page.evaluate('window.__testState.notes')) == 1
    page.locator('#historyTab').click()
    page.locator('#searchInput').fill('FFT')
    assert page.locator('.record').count() == 1
    page.screenshot(path=str(BASE/'docs/screenshot_history.png'))
    page.locator('.record-actions button').first.click()
    assert page.evaluate('window.__testState.notes[0].visible') is True
    page.locator('#newNote').click()
    page.locator('#modeTodo').click()
    page.locator('#todoInput').fill('整理测试数据')
    page.locator('#todoInput').press('Enter')
    page.wait_for_timeout(650)
    assert len(page.evaluate('window.__testState.notes')) == 2
    page.locator('.todo-row input').check()
    page.wait_for_timeout(700)
    assert page.evaluate('window.__testState.notes.find(n=>n.kind==="todo").tasks[0].done') is True
    page.screenshot(path=str(BASE/'docs/screenshot_todo.png'))
    # Regression: third button must be a *real image mode*, not a disabled
    # paste action when coming from a checklist.
    page.locator('#modeImage').click()
    assert page.locator('#imageEmpty').is_visible(), 'Todo -> image did not switch'
    page.locator('#toast').evaluate("el=>el.classList.add('hidden')")
    page.screenshot(path=str(BASE/'docs/screenshot_image_mode.png'))
    assert page.locator('#modeImage').get_attribute('class').find('active') >= 0
    assert len(page.evaluate('window.__testState.notes.find(n=>n.kind==="todo").tasks')) == 1
    page.locator('#pasteImageBtn').click()
    page.wait_for_timeout(120)
    assert page.locator('#imageWrap').is_visible(), 'Image-mode native paste did not preview'
    page.locator('#modeText').click()
    page.locator('#modeImage').click()
    page.keyboard.press('Control+v')
    page.wait_for_timeout(120)
    assert page.locator('#imageWrap').is_visible(), 'Image-mode Ctrl+V failed to use native bridge'
    assert page.evaluate('window.__testState.notes.some(n=>n.kind==="image")')
    page.locator('#modeText').click()
    assert page.locator('#textComposer').is_visible() and not page.locator('#imageEmpty').is_visible()
    page.locator('#modeTodo').click()
    assert page.locator('#todoComposer').is_visible()
    print('MODE SWITCH PASS: Todo -> Image -> Text -> Todo; images import in image mode')
    # PixPin/third-party screenshot path: WebView ClipboardEvent -> image Blob -> Rust PNG command.
    page.locator('#newNote').click()
    page.evaluate(r"""() => {
      const b64='iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4////fwAJ+wP+eYbvyAAAAABJRU5ErkJggg==';
      const file=new File([Uint8Array.from(atob(b64),c=>c.charCodeAt(0))], 'screen.png', {type:'image/png'});
      const dt=new DataTransfer(); dt.items.add(file);
      const event=new ClipboardEvent('paste',{clipboardData:dt,bubbles:true,cancelable:true});
      document.querySelector('#noteText').dispatchEvent(event);
    }""")
    page.wait_for_timeout(600)
    assert page.evaluate("window.__testState.notes.some(n=>n.kind==='image')"), 'Image paste did not persist'
    assert page.locator('#imageWrap').is_visible(), 'Pasted image not displayed'
    print('IMAGE PASTE SMOKE PASS: ClipboardEvent image -> native import command -> preview')
    # Native-float page rendering/interaction tested with a separate Tauri bridge stub.
    float_page=browser.new_page(viewport={'width':350,'height':290},device_scale_factor=1)
    float_errors=[]
    float_page.on('pageerror',lambda e:float_errors.append(str(e)))
    float_page.goto('about:blank')
    float_page.evaluate(r"""(() => {
      let note={id:'n1',kind:'todo',text:'',createdAt:Date.now(),opacity:.96,alwaysOnTop:true,tasks:[{id:'a',text:'整理数据',done:false,doneAt:null},{id:'b',text:'准备汇报',done:false,doneAt:null}]};
      window.__floatTest={get note(){return note},hidden:false};
      const w={label:'float_n1',startDragging:async()=>{},hide:async()=>{window.__floatTest.hidden=true},outerPosition:async()=>({x:260,y:170}),innerSize:async()=>({width:350,height:290}),scaleFactor:async()=>1,onMoved:async()=>{},onResized:async()=>{}};
      window.__TAURI__={window:{getCurrentWindow:()=>w},event:{listen:async()=>{}},core:{invoke:async (name,args={})=>{
        if(name==='list_notes')return [structuredClone(note)];
        if(name==='save_todo'){note.tasks=args.tasks;return structuredClone(note)};
        if(name==='hide_float'){window.__floatTest.hidden=true;return};
        if(name==='update_float'){if(args.alwaysOnTop!==null&&args.alwaysOnTop!==undefined)note.alwaysOnTop=args.alwaysOnTop;return structuredClone(note)}
        return null;
      }}};
    })();""")
    fhtml=(BASE/'web/float.html').read_text()
    fhtml=fhtml.replace('<link rel="stylesheet" href="styles.css">','<style>'+ (BASE/'web/styles.css').read_text()+'</style>')
    fhtml=fhtml.replace('<script defer src="float.js"></script>','')
    fhtml=fhtml.replace('</body>','<script>'+ (BASE/'web/float.js').read_text()+'</script></body>')
    float_page.set_content(fhtml)
    float_page.locator('.float-task input').first.check()
    float_page.wait_for_timeout(120)
    assert float_page.evaluate('window.__floatTest.note.tasks[0].done') is True
    float_page.screenshot(path=str(BASE/'docs/screenshot_float.png'))
    float_page.locator('#floatDrag').dblclick(position={'x':30,'y':20})
    assert float_page.evaluate('window.__floatTest.hidden') is True
    assert not float_errors,float_errors
    print('FLOAT SMOKE PASS: separate window, check Todo, double-click hide')
    print('UI SMOKE PASS: text autosave, keyword search, pin, Todo create/check')
    print('JS errors:',errors)
    assert not errors,errors
    browser.close()

server.shutdown()
