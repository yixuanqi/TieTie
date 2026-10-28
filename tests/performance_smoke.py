"""Repeatable browser-level card/render tests, NOT a Windows WebView2 benchmark."""
from pathlib import Path
from playwright.sync_api import sync_playwright
import json, statistics
BASE=Path(__file__).resolve().parents[1]
HTML=(BASE/'web/index.html').read_text(encoding='utf-8')
HTML=HTML.replace('<link rel="stylesheet" href="styles.css">','<style>'+(BASE/'web/styles.css').read_text(encoding='utf-8')+'</style>')
HTML=HTML.replace('<script defer src="app.js"></script>','')
HTML=HTML.replace('</body>','<script>'+(BASE/'web/app.js').read_text(encoding='utf-8')+'</script></body>')
JS=r'''(() => {
  let notes=[], calls={list:0,image:0};
  const cb={};
  const clone=(x)=>JSON.parse(JSON.stringify(x));
  const now=Date.now();
  notes=Array.from({length:500},(_,i)=>({
    id:'n'+i,kind:i%5===0?'image':i%5===1?'todo':'text',
    text:'干涉仪 FFT 测试笔记编号 '+i, imageName:'img-'+i+'.png',
    tasks:i%5===1?[{id:'t',text:'验证第 '+i+' 项',done:false}]:[],
    createdAt:now-i*60000,updatedAt:now-i*60000,visible:false,opacity:.96
  }));
  window.__perf={get notes(){return notes;},get calls(){return calls;},emit(name){(cb[name]||[]).forEach(fn=>fn({payload:null}));}, mutate(){notes[0].visible=!notes[0].visible;},makeNote(){notes.unshift({id:'new'+Date.now(),kind:'text',text:'新增',tasks:[],createdAt:Date.now(),updatedAt:Date.now(),visible:false})}};
  window.__TAURI__={
    core:{invoke:async(n,args={})=>{
      if(n==='list_notes'){calls.list++;return clone(notes);}
      if(n==='get_hotkeys')return{panel:true,capture:true};
      if(n==='get_thumbnail_data'){calls.image++;return 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4////fwAJ+wP+eYbvyAAAAABJRU5ErkJggg=='};
      if(n==='get_data_directory')return'local';
      if(n==='hide_panel')return null;
      throw Error('Unexpected '+n);
    }},
    event:{listen:async(n,fn)=>{(cb[n]||=[]).push(fn);return()=>{};},emit:async()=>{}},
    window:{getCurrentWindow:()=>({startDragging:async()=>{}})}
  };
})();'''

with sync_playwright() as p:
    browser=p.chromium.launch(headless=True,executable_path='/usr/bin/chromium',args=['--no-sandbox'])
    page=browser.new_page(viewport={'width':480,'height':572})
    errors=[];page.on('pageerror',lambda err:errors.append(str(err)))
    page.goto('about:blank');page.evaluate(JS);page.set_content(HTML)
    page.locator('#historyTab').click()
    page.wait_for_function('document.querySelectorAll(".record").length === 250')
    # first window of image elements
    page.wait_for_timeout(100)
    initial=page.evaluate('''() => ({first:document.querySelector('.record'),firstImg:document.querySelector('.thumb img'),imageCalls:window.__perf.calls.image})''')
    # On an unrelated event, original first node and image MUST be retained.
    results=page.evaluate(r'''async()=>{
      const list=document.querySelector('#historyList');
      const first=list.querySelector('.record');
      const firstImg=list.querySelector('.thumb img');
      const t0=performance.now();
      for(let i=0;i<40;i++){
        window.__perf.emit('notes-changed');
        await new Promise(r=>setTimeout(r,0));
      }
      await new Promise(r=>setTimeout(r,80));
      const t1=performance.now();
      return {unchangedSameNode:first===list.querySelector('.record'),
        unchangedSameImage:firstImg===list.querySelector('.thumb img'),
        rows:list.querySelectorAll('.record').length,
        imageReads:window.__perf.calls.image,
        totalEventWallMs:Math.round((t1-t0)*10)/10};
    }''')
    assert results['unchangedSameNode'] and results['unchangedSameImage'],results
    assert results['rows']==250,results
    # Changing a single row must not destroy the previously decoded thumbnail.
    result2=page.evaluate(r'''async()=>{
      const list=document.querySelector('#historyList');
      const first=list.querySelector('.record');
      const img=list.querySelector('.thumb img');
      window.__perf.mutate(); window.__perf.emit('notes-changed');
      await new Promise(r=>setTimeout(r,90));
      return {sameNode:first===list.querySelector('.record'),sameImage:img===list.querySelector('.thumb img'),
        rowCount:list.querySelectorAll('.record').length,
        imageReads:window.__perf.calls.image};
    }''')
    assert result2['sameNode'] and result2['sameImage'],result2
    # 20 query changes must keep app responsive and bound DOM to current results.
    searches=page.evaluate(r'''()=>{
      const el=document.querySelector('#searchInput'),a=[];
      for(let i=0;i<20;i++){
        const t=performance.now();el.value='编号 '+i;el.dispatchEvent(new Event('input',{bubbles:true}));
        a.push(performance.now()-t);
      }
      return {samples:a,rows:document.querySelectorAll('.record').length};
    }''')
    searches['avgMs']=round(statistics.mean(searches.pop('samples')),2)
    browser.close()
    print(json.dumps({'unchanged_events':results,'one_row_change':result2,'search20':searches,'js_errors':errors},ensure_ascii=False,indent=2))
    assert not errors,errors
