"""Source-level regression contract for the Windows rounded HWND, not a native test."""
from pathlib import Path
import json,re
root=Path(__file__).resolve().parents[1]
config=json.loads((root/'src-tauri/tauri.conf.json').read_text())
main=config['app']['windows'][0]
assert main['decorations'] is False
assert main['transparent'] is False, 'Transparent WebView2 caused prior bleed-through'
assert main['resizable'] is True
css=(root/'web/styles.css').read_text()
assert re.search(r'\.main-body\s*\{[^}]*background:#faf8f2',css)
assert re.search(r':root\[data-theme="dark"\]\s+\.main-body\s*\{background:#202a32',css)
assert '.main-body .panel,.settings-panel { border-radius:21px; }' in css
code=(root/'src-tauri/src/lib.rs').read_text()
for sym in ['CreateRoundRectRgn', 'SetWindowRgn', 'DeleteObject', 'window.hwnd()', 'window.outer_size()', 'window.scale_factor()', 'WindowEvent::Resized(_)']:
    assert sym in code,sym
assert 'app.get_window("main")' in code
assert 'app.get_window("settings")' in code
assert 'LAST_REGION' in code
assert '.transparent(false).decorations(false).shadow(false)' in code
print('ROUNDING CONTRACT PASS: HWND clip + dark/bright opaque backgrounds + resizing + settings window')
