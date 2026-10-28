mod storage;

use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{ImageFormat, RgbaImage, ImageReader};
use storage::{now_ms, DataStore, Note, Task};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};
use std::{fs, io::Cursor, path::PathBuf};
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all="camelCase")]
struct HotkeyStatus { panel: bool, capture: bool }

#[tauri::command]
fn list_notes(store: State<'_, DataStore>) -> Result<Vec<Note>, String> {
    store.read(|notes| {
        let mut result: Vec<Note> = notes.iter().filter(|n| n.deleted_at.is_none()).cloned().collect();
        result.sort_by(|a,b| b.created_at.cmp(&a.created_at));
        result
    })
}

#[tauri::command]
fn save_text(app: AppHandle, store: State<'_, DataStore>, id: Option<String>, text: String) -> Result<Note, String> {
    if text.trim().is_empty() && id.is_none() { return Err("不能保存空白便签".into()); }
    if text.len() > 1_000_000 { return Err("文字长度超过 1MB 限制".into()); }
    let saved = store.write(|notes| {
        if let Some(ref id) = id {
            let note = notes.iter_mut().find(|n| n.id == *id && n.deleted_at.is_none()).ok_or("找不到记录")?;
            if note.kind == "todo" { return Err("不能用文字模式覆盖待办清单".into()); }
            note.text = text; note.updated_at = now_ms();
            Ok(note.clone())
        } else {
            let mut note = Note::new("text"); note.text = text;
            notes.push(note.clone()); Ok(note)
        }
    })?;
    let _ = app.emit("notes-changed", ());
    Ok(saved)
}

#[tauri::command]
fn save_todo(app: AppHandle, store: State<'_, DataStore>, id: Option<String>, tasks: Vec<Task>) -> Result<Note, String> {
    if tasks.len() > 150 { return Err("单张待办最多 150 项".into()); }
    if tasks.iter().any(|t| t.text.len() > 2000) { return Err("单条待办不能超过 2000 字".into()); }
    if tasks.is_empty() && id.is_none() { return Err("请先添加至少一项待办".into()); }
    let saved = store.write(|notes| {
        if let Some(ref id) = id {
            let note = notes.iter_mut().find(|n| n.id == *id && n.deleted_at.is_none() && n.kind == "todo").ok_or("找不到待办记录")?;
            note.tasks = tasks; note.updated_at = now_ms(); Ok(note.clone())
        } else {
            let mut note = Note::new("todo"); note.tasks = tasks;
            notes.push(note.clone()); Ok(note)
        }
    })?;
    let _ = app.emit("notes-changed", ());
    Ok(saved)
}

const MAX_IMAGE_PIXELS: usize = 60_000_000;
const MAX_IMAGE_BYTES: usize = 45_000_000;

fn check_dimensions(width: usize, height: usize) -> Result<(), String> {
    if width == 0 || height == 0 || width.checked_mul(height).unwrap_or(usize::MAX) > MAX_IMAGE_PIXELS {
        return Err("图片分辨率过大（上限约 6000 万像素）".into());
    }
    Ok(())
}

// Write the image as a real PNG, then update metadata. A failure to update
// metadata cannot leave a new unreferenced image behind.
fn persist_image(store: &DataStore, target_id: Option<String>, rgba: RgbaImage) -> Result<Note, String> {
    check_dimensions(rgba.width() as usize, rgba.height() as usize)?;
    let filename = format!("{}.png", uuid::Uuid::new_v4());
    let path = store.root.join("images").join(&filename);
    rgba.save_with_format(&path, ImageFormat::Png).map_err(|e| format!("保存图片失败: {e}"))?;
    let result = store.write(|notes| {
        let mut note = if let Some(ref id) = target_id {
            notes.iter().find(|n| n.id == *id && n.deleted_at.is_none()).cloned().ok_or("找不到当前便签")?
        } else { Note::new("image") };
        if note.kind == "todo" { return Err("不能用图片覆盖待办清单".into()); }
        note.kind = "image";
        note.image_name = Some(filename.clone());
        note.updated_at = now_ms();
        if let Some(existing) = notes.iter_mut().find(|n| n.id == note.id) {
            *existing = note.clone();
        } else { notes.push(note.clone()); }
        Ok(note)
    });
    if result.is_err() { let _ = fs::remove_file(&path); }
    result
}

fn decode_image_bytes(bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() > MAX_IMAGE_BYTES { return Err("图片文件超过 45MB 限制".into()); }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()
        .map_err(|e| format!("无法识别图片格式: {e}"))?;
    let (w, h) = reader.into_dimensions().map_err(|e| format!("图片头无效: {e}"))?;
    check_dimensions(w as usize, h as usize)?;
    image::load_from_memory(bytes).map_err(|e| format!("图片解码失败: {e}"))
        .map(|img| img.to_rgba8())
}

// Windows clipboard uses CF_DIB (8), CF_DIBV5 (17), CF_BITMAP (2),
// and copied Explorer image files use CF_HDROP (15). `arboard` cannot decode
// every legal DIB variant, especially PixPin's BI_BITFIELDS encoding.
// Only start the more expensive compatibility fallback when a media format
// is actually advertised; plain-text captures stay fast.
#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn IsClipboardFormatAvailable(format: u32) -> i32;
}

#[cfg(windows)]
fn has_windows_image_clipboard() -> bool {
    [2_u32, 8, 17, 15].iter().any(|format| unsafe { IsClipboardFormatAvailable(*format) != 0 })
}

#[cfg(windows)]
fn windows_image_compat(store: &DataStore) -> Result<RgbaImage, String> {
    // A fixed, non-user-provided PowerShell script uses Windows Forms' GDI+
    // decoder. This is a confirmed fallback for PixPin CF_DIB + BI_BITFIELDS.
    // Never run copied text as executable code or interpolate a clipboard path.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
for ($i = 0; $i -lt 4; $i++) {
  try {
    $img = [System.Windows.Forms.Clipboard]::GetImage()
    if ($null -eq $img -and [System.Windows.Forms.Clipboard]::ContainsFileDropList()) {
      $paths = [System.Windows.Forms.Clipboard]::GetFileDropList()
      if ($paths.Count -eq 1) {
        $p = [string]$paths[0]
        $ext = [System.IO.Path]::GetExtension($p).ToLowerInvariant()
        if ($ext -in @('.png','.jpg','.jpeg','.bmp','.gif','.tif','.tiff') -and [System.IO.File]::Exists($p)) {
          $img = [System.Drawing.Image]::FromFile($p)
        }
      }
    }
    if ($null -eq $img) { exit 4 }
    try { $img.Save($env:TIETIE_CAPTURE_PNG, [System.Drawing.Imaging.ImageFormat]::Png) }
    finally { $img.Dispose() }
    exit 0
  } catch {
    if ($i -eq 3) { [Console]::Error.WriteLine($_.Exception.Message); exit 5 }
    Start-Sleep -Milliseconds 70
  }
}
"#;
    let temp = store.root.join("images").join(format!(".clip-{}.png", uuid::Uuid::new_v4()));
    let result = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Sta", "-Command", SCRIPT])
        .env("TIETIE_CAPTURE_PNG", &temp)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW, avoids a console flash
        .output()
        .map_err(|e| format!("无法启动 Windows 图片兼容读取器: {e}"));
    let bytes = match result {
        Ok(output) if output.status.success() => fs::read(&temp)
            .map_err(|e| format!("读取临时图片失败: {e}")),
        Ok(output) => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(format!("Windows 兼容读取失败（代码 {:?}{}）", output.status.code(),
                if detail.is_empty() { "".to_string() } else { format!("，{detail}") }))
        },
        Err(e) => Err(e),
    };
    let _ = fs::remove_file(&temp);
    decode_image_bytes(&bytes?)
}

fn capture_clipboard_inner(store: &DataStore, target_id: Option<String>) -> Result<Note, String> {
    let mut clip = Clipboard::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
    // Prefer image over text when both flavors are present.
    let native_error = match clip.get_image() {
        Ok(img) => {
            check_dimensions(img.width, img.height)?;
            let rgba = RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
                .ok_or("剪贴板图片像素无效")?;
            return persist_image(store, target_id, rgba);
        },
        Err(err) => err.to_string(),
    };

    #[cfg(windows)]
    if has_windows_image_clipboard() {
        match windows_image_compat(store) {
            Ok(img) => return persist_image(store, target_id, img),
            Err(err) => return Err(format!("检测到图片，但读取失败。快速读取: {native_error}；兼容读取: {err}")),
        }
    }

    let text = clip.get_text().map_err(|_| {
        format!("剪贴板中没有可读取的文字或图片（图片读取: {native_error}）。请先复制，再试一次。")
    })?;
    if text.trim().is_empty() { return Err("剪贴板文字为空".into()); }
    if text.len() > 1_000_000 { return Err("复制的文字超过 1MB 限制".into()); }
    store.write(|notes| {
        if let Some(ref id) = target_id {
            let note = notes.iter_mut().find(|n| n.id == *id && n.deleted_at.is_none()).ok_or("找不到当前便签")?;
            if note.kind == "todo" { return Err("请在待办输入框中粘贴文字".into()); }
            if !note.text.is_empty() { note.text.push('\n'); }
            note.text.push_str(&text);
            note.updated_at = now_ms(); Ok(note.clone())
        } else {
            let mut note = Note::new("text"); note.text = text;
            notes.push(note.clone()); Ok(note)
        }
    })
}

#[tauri::command]
async fn capture_clipboard(app: AppHandle, target_id: Option<String>) -> Result<Note, String> {
    // Don't block the WebView's command dispatch while PowerShell is decoding.
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<DataStore>();
        let note = capture_clipboard_inner(&store, target_id)?;
        let _ = app.emit("notes-changed", ());
        Ok::<_, String>(note)
    }).await.map_err(|e| format!("读取剪贴板任务失败: {e}"))?
}

#[tauri::command]
async fn paste_image_data(app: AppHandle, target_id: Option<String>, image_data: String) -> Result<Note, String> {
    // Used only by Ctrl+V WebView ClipboardEvent. The MIME is checked here,
    // dimensions and image format are verified before writing a file.
    let data = image_data.strip_prefix("data:image/").ok_or("粘贴数据不是图片")?;
    let (_, b64) = data.split_once(";base64,").ok_or("图片数据不是 Base64 格式")?;
    if b64.len() > MAX_IMAGE_BYTES * 4 / 3 + 8 { return Err("图片超过 45MB 限制".into()); }
    let bytes = STANDARD.decode(b64).map_err(|e| format!("图片 Base64 解码失败: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let rgba = decode_image_bytes(&bytes)?;
        let store = app.state::<DataStore>();
        let note = persist_image(&store, target_id, rgba)?;
        let _ = app.emit("notes-changed", ());
        Ok::<_, String>(note)
    }).await.map_err(|e| format!("写入图片任务失败: {e}"))?
}

#[tauri::command]
fn get_image_data(store: State<'_, DataStore>, id: String) -> Result<String, String> {
    let note = store.read(|notes| notes.iter().find(|n| n.id == id && n.deleted_at.is_none()).cloned())?.ok_or("找不到图片")?;
    let file = store.image_file(&note)?;
    let data = fs::read(file).map_err(|e| format!("读取图片失败: {e}"))?;
    if data.len() > 80_000_000 { return Err("图片文件过大".into()); }
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(data)))
}

#[tauri::command]
fn delete_note(app: AppHandle, store: State<'_, DataStore>, id: String) -> Result<(), String> {
    store.write(|notes| {
        let note = notes.iter_mut().find(|n| n.id == id && n.deleted_at.is_none()).ok_or("找不到记录")?;
        note.deleted_at = Some(now_ms()); note.visible = false; note.pinned = false;
        Ok(())
    })?;
    if let Some(w) = app.get_webview_window(&format!("float_{id}")) { let _ = w.close(); }
    let _ = app.emit("notes-changed", ());
    Ok(())
}

#[tauri::command]
fn restore_note(app: AppHandle, store: State<'_, DataStore>, id: String) -> Result<(), String> {
    store.write(|notes| {
        let note = notes.iter_mut().find(|n| n.id == id && n.deleted_at.is_some()).ok_or("找不到已删除的记录")?;
        note.deleted_at = None; note.updated_at = now_ms(); Ok(())
    })?;
    let _ = app.emit("notes-changed", ());
    Ok(())
}

fn mark_visible(store: &DataStore, id: &str, visible: bool) -> Result<Note, String> {
    store.write(|notes| {
        let note = notes.iter_mut().find(|n| n.id == id && n.deleted_at.is_none()).ok_or("找不到记录")?;
        note.visible = visible; if visible { note.pinned = true; }
        Ok(note.clone())
    })
}

fn build_float(app: &AppHandle, note: &Note) -> Result<(), String> {
    let label = format!("float_{}", note.id);
    if let Some(w) = app.get_webview_window(&label) {
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("float.html".into()))
        .title("贴贴 · 桌面浮贴")
        .position(note.x as f64, note.y as f64)
        .inner_size(note.w as f64, note.h as f64)
        .min_inner_size(220.0, 160.0)
        .transparent(true).decorations(false).shadow(false)
        .always_on_top(note.always_on_top)
        .skip_taskbar(true).resizable(true)
        .build().map_err(|e| format!("浮贴窗口创建失败: {e}"))?;
    window.set_focus().ok();
    Ok(())
}

#[tauri::command]
async fn open_float(app: AppHandle, store: State<'_, DataStore>, id: String) -> Result<(), String> {
    let note = mark_visible(&store, &id, true)?;
    if let Err(e) = build_float(&app, &note) {
        let _ = mark_visible(&store, &id, false);
        return Err(e);
    }
    let _ = app.emit("notes-changed", ());
    Ok(())
}

#[tauri::command]
fn hide_float(app: AppHandle, store: State<'_, DataStore>, id: String) -> Result<(), String> {
    mark_visible(&store, &id, false)?;
    if let Some(w) = app.get_webview_window(&format!("float_{id}")) {
        w.hide().map_err(|e| e.to_string())?;
    }
    let _ = app.emit("notes-changed", ());
    Ok(())
}

#[tauri::command]
fn update_float(
    app: AppHandle, store: State<'_, DataStore>, id: String,
    x: Option<i32>, y: Option<i32>, w: Option<u32>, h: Option<u32>,
    opacity: Option<f64>, always_on_top: Option<bool>
) -> Result<Note, String> {
    let note = store.write(|notes| {
        let note = notes.iter_mut().find(|n| n.id == id && n.deleted_at.is_none()).ok_or("找不到记录")?;
        if let Some(x) = x { note.x = x; }
        if let Some(y) = y { note.y = y; }
        if let Some(w) = w { note.w = w.clamp(220, 2500); }
        if let Some(h) = h { note.h = h.clamp(160, 2000); }
        if let Some(o) = opacity { note.opacity = o.clamp(0.6, 1.0); }
        if let Some(t) = always_on_top { note.always_on_top = t; }
        Ok(note.clone())
    })?;
    if let Some(t) = always_on_top {
        if let Some(win) = app.get_webview_window(&format!("float_{id}")) { let _ = win.set_always_on_top(t); }
    }
    let _ = app.emit("notes-changed", ());
    Ok(note)
}

#[tauri::command]
fn get_hotkeys(status: State<'_, HotkeyStatus>) -> HotkeyStatus { status.inner().clone() }

#[tauri::command]
fn get_data_directory(store: State<'_, DataStore>) -> String { store.root.display().to_string() }

#[tauri::command]
fn show_note_in_panel(app: AppHandle, store: State<'_, DataStore>, id: String) -> Result<(), String> {
    let exists = store.read(|v| v.iter().any(|n| n.id == id && n.deleted_at.is_none()))?;
    if !exists { return Err("找不到这条记录".into()); }
    let main = app.get_webview_window("main").ok_or("主窗口不存在")?;
    main.show().map_err(|e| e.to_string())?;
    main.set_focus().map_err(|e| e.to_string())?;
    app.emit_to("main", "open-note", id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn hide_panel(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("main").ok_or("主窗口不存在")?.hide().map_err(|e| e.to_string())
}

fn toggle_panel(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
            let _ = app.emit_to("main", "panel-shown", ());
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed { return; }
            if shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::KeyN) {
                toggle_panel(app);
            } else if shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::KeyV) {
                let app = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let store = app.state::<DataStore>();
                    let result = capture_clipboard_inner(&store, None);
                    let msg = match result { Ok(_) => { let _ = app.emit("notes-changed", ()); "✓ 剪贴板已收藏".to_string() }, Err(e) => e };
                    let _ = app.emit_to("main", "capture-feedback", msg);
                });
            }
        }).build())
        .setup(|app| {
            let directory: PathBuf = app.path().app_data_dir()?;
            let store = DataStore::open(directory).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            // Restore only previously visible floating references.
            let restore = store.read(|notes| notes.iter().filter(|n| n.deleted_at.is_none() && n.pinned && n.visible).cloned().collect::<Vec<_>>())?;
            app.manage(store);

            let toggle = MenuItem::with_id(app, "toggle", "打开 / 隐藏贴贴", true, None::<&str>)?;
            let capture = MenuItem::with_id(app, "capture", "收藏剪贴板", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出贴贴", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &capture, &quit])?;
            let tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().ok_or("缺少托盘图标")?.clone())
                .tooltip("贴贴 · 随手存下，随时找回")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|handle, event| match event.id().0.as_str() {
                    "toggle" => toggle_panel(handle),
                    "capture" => {
                        let handle = handle.clone();
                        tauri::async_runtime::spawn_blocking(move || {
                            let res = capture_clipboard_inner(&handle.state::<DataStore>(), None);
                            if res.is_ok() { let _ = handle.emit("notes-changed", ()); }
                            let _ = handle.emit_to("main", "capture-feedback", res.map(|_| "✓ 剪贴板已收藏".to_owned()).unwrap_or_else(|e|e));
                        });
                    },
                    "quit" => handle.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                        toggle_panel(tray.app_handle());
                    }
                })
                .build(app)?;
            let _ = tray;

            let key_panel = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyN);
            let key_capture = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV);
            let p = app.global_shortcut().register(key_panel).is_ok();
            let c = app.global_shortcut().register(key_capture).is_ok();
            if !p { eprintln!("贴贴: Ctrl+Alt+N 已被其他程序占用，请通过托盘打开"); }
            if !c { eprintln!("贴贴: Ctrl+Alt+V 已被其他程序占用"); }
            app.manage(HotkeyStatus { panel: p, capture: c });
            for note in restore { if let Err(e) = build_float(&app.handle(), &note) { eprintln!("恢复浮贴失败: {e}"); } }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                if let Some(id) = window.label().strip_prefix("float_") {
                    let app = window.app_handle();
                    let _ = mark_visible(&app.state::<DataStore>(), id, false);
                    let _ = app.emit("notes-changed", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            list_notes, save_text, save_todo, capture_clipboard, paste_image_data, get_image_data,
            delete_note, restore_note, open_float, hide_float, update_float,
            get_hotkeys, get_data_directory, hide_panel, show_note_in_panel
        ])
        .run(tauri::generate_context!())
        .expect("贴贴启动失败");
}
