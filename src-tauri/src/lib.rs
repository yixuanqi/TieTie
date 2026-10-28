mod storage;
mod preferences;

use arboard::Clipboard;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{ImageFormat, RgbaImage, ImageReader};
use storage::{now_ms, DataStore, Note, Task};
use preferences::{Preferences, PreferencesState};
use std::{str::FromStr, sync::Mutex};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent, LogicalSize,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use std::{fs, io::Cursor, path::PathBuf};
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

// v0.4.5: 改用透明窗口 + CSS 圆角（WebView2 按像素混合，四角平滑抗锯齿）。
// 早期的 SetWindowRgn 原生裁剪是 1 位掩码，Win10 下四角有锯齿，已移除。
// 注意：主面板/设置窗口需 transparent:true（tauri.conf.json 与 show_settings）。

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
        note.kind = "image".to_string();
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

// Win32 clipboard may contain CF_BITMAP / DIB / DIBV5, CF_HDROP (a
// copied file), or registered PNG/JFIF/WebP flavors. PixPin can switch
// between image data and a file-drop depending on its settings.
#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn OpenClipboard(hwnd: *mut std::ffi::c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn EnumClipboardFormats(format: u32) -> u32;
    fn GetClipboardFormatNameW(format: u32, buffer: *mut u16, len: i32) -> i32;
}

#[cfg(windows)]
fn has_windows_image_clipboard() -> bool {
    // CF_BITMAP 2, CF_DIB 8, CF_HDROP 15, CF_DIBV5 17
    if [2_u32, 8, 17, 15].iter().any(|f| unsafe { IsClipboardFormatAvailable(*f) != 0 }) {
        return true;
    }
    // Clipboard producers sometimes expose only a registered "PNG" or
    // "image/webp" format, not one of the standard CF_* formats.
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 { return false; }
        let mut format = 0_u32;
        let mut found = false;
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 { break; }
            let mut name = [0_u16; 128];
            let len = GetClipboardFormatNameW(format, name.as_mut_ptr(), name.len() as i32);
            if len > 0 {
                let label = String::from_utf16_lossy(&name[..len as usize]).to_ascii_lowercase();
                if ["png", "jfif", "webp", "gif", "image/png", "image/jpeg", "image/webp", "image/bmp", "image/avif"].contains(&label.as_str()) {
                    found = true; break;
                }
            }
        }
        CloseClipboard();
        found
    }
}

#[cfg(windows)]
fn windows_image_compat(store: &DataStore) -> Result<RgbaImage, String> {
    // The script reads clipboard *formats only*, never executes clipboard text.
    // It first imports a copied image file (PixPin auto/PNG/WebP modes), then
    // registered byte-stream image formats, then GDI+ bitmap representations.
    // PowerShell runs in a Windows STA thread because Clipboard is COM-backed.
    const SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$target = $env:TIETIE_CAPTURE_IMAGE
$extensions = @('.png','.jpg','.jpeg','.bmp','.gif','.webp','.tif','.tiff','.avif')
for ($attempt = 0; $attempt -lt 4; $attempt++) {
  try {
    $data = [System.Windows.Forms.Clipboard]::GetDataObject()
    if ($null -eq $data) { throw '剪贴板暂时不可用' }
    # PixPin's "复制图像为文件" (Auto, PNG, WebP etc.) arrives as CF_HDROP.
    if ([System.Windows.Forms.Clipboard]::ContainsFileDropList()) {
      $paths = [System.Windows.Forms.Clipboard]::GetFileDropList()
      if ($paths.Count -eq 1) {
        $source = [string]$paths[0]
        $extension = [System.IO.Path]::GetExtension($source).ToLowerInvariant()
        if ($extension -in $extensions -and [System.IO.File]::Exists($source)) {
          if (([System.IO.FileInfo]::new($source)).Length -gt 45000000) { throw '图片文件超过 45MB 限制' }
          if ($extension -eq '.avif') {
            try {
              $avif = [System.Drawing.Image]::FromFile($source)
              try { $avif.Save($target, [System.Drawing.Imaging.ImageFormat]::Png) }
              finally { $avif.Dispose() }
              exit 0
            } catch { throw '当前图片为 AVIF，请将 PixPin 的「复制图像为文件」改成 PNG 或 WebP' }
          }
          [System.IO.File]::Copy($source, $target, $true)
          exit 0
        }
      }
    }
    # Some apps publish raw PNG / JFIF streams without CF_DIB.
    foreach ($format in @('PNG','image/png','JFIF','image/jpeg','WebP','WEBP','image/webp','GIF','image/gif')) {
      try {
        if (-not $data.GetDataPresent($format, $false)) { continue }
        $value = $data.GetData($format, $false)
        if ($value -is [byte[]]) {
          [System.IO.File]::WriteAllBytes($target, $value); exit 0
        }
        if ($value -is [System.IO.Stream]) {
          $out = [System.IO.File]::Create($target)
          try { if ($value.CanSeek) { $value.Position = 0 }; $value.CopyTo($out) }
          finally { $out.Dispose() }
          exit 0
        }
        if ($value -is [System.Drawing.Image]) {
          try { $value.Save($target,[System.Drawing.Imaging.ImageFormat]::Png) }
          finally { $value.Dispose() }
          exit 0
        }
      } catch { # Try the next advertised format or a bitmap decoder.
      }
    }
    # Standard bitmap, DIB and DIBV5 through Windows Forms / GDI+.
    $img = [System.Windows.Forms.Clipboard]::GetImage()
    if ($null -ne $img) {
      try { $img.Save($target, [System.Drawing.Imaging.ImageFormat]::Png) }
      finally { $img.Dispose() }
      exit 0
    }
    $formats = ($data.GetFormats($false) | Sort-Object -Unique) -join ', '
    throw ('无法解码这些剪贴板格式：' + $formats)
  } catch {
    if ($attempt -eq 3) { [Console]::Error.WriteLine($_.Exception.Message); exit 5 }
    Start-Sleep -Milliseconds 90
  }
}
"#;
    let temp = store.root.join("images").join(format!(".clip-{}.bin", uuid::Uuid::new_v4()));
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Sta", "-Command", SCRIPT])
        .env("TIETIE_CAPTURE_IMAGE", &temp)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .map_err(|e| format!("无法启动 Windows 图片读取器: {e}"));
    let bytes = match output {
        Ok(out) if out.status.success() => fs::read(&temp)
            .map_err(|e| format!("读取临时图片失败: {e}")),
        Ok(out) => {
            let detail = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if detail.is_empty() {
                format!("Windows 图片兼容读取失败（代码 {:?}）", out.status.code())
            } else {
                format!("Windows 图片兼容读取失败：{detail}")
            })
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

// Thumbnails are created once and cached in the user's app data directory.
// This avoids passing megabytes of full-resolution PNG bytes to WebView2 for
// every small search result. The original PNG remains untouched.
#[tauri::command]
async fn get_thumbnail_data(app: AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let store = app.state::<DataStore>();
        let note = store.read(|notes| notes.iter().find(|n| n.id == id && n.deleted_at.is_none()).cloned())?
            .ok_or("找不到图片")?;
        let original = store.image_file(&note)?; // validates generated image filenames
        let cache_dir = store.root.join("thumbnails");
        fs::create_dir_all(&cache_dir).map_err(|e| format!("缩略图目录创建失败: {e}"))?;
        let filename = note.image_name.as_ref().ok_or("没有原始图片")?;
        let cached = cache_dir.join(filename);
        let bytes = if cached.is_file() {
            fs::read(&cached).map_err(|e| format!("读取缩略图失败: {e}"))?
        } else {
            let source = fs::read(&original).map_err(|e| format!("读取原始图片失败: {e}"))?;
            if source.len() > 80_000_000 { return Err("图片文件过大".into()); }
            let decoder = ImageReader::new(Cursor::new(&source)).with_guessed_format()
                .map_err(|e| format!("识别图片格式失败: {e}"))?;
            let (w, h) = decoder.into_dimensions().map_err(|e| format!("图片头无效: {e}"))?;
            check_dimensions(w as usize, h as usize)?;
            let photo = image::load_from_memory(&source).map_err(|e| format!("生成缩略图失败: {e}"))?;
            let thumb = photo.thumbnail(192, 144);
            let mut output = Cursor::new(Vec::new());
            thumb.write_to(&mut output, ImageFormat::Png).map_err(|e| format!("缩略图编码失败: {e}"))?;
            let data = output.into_inner();
            let _ = fs::write(&cached, &data); // cache failures must not lose original image
            data
        };
        Ok::<_, String>(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
    }).await.map_err(|e| format!("缩略图任务失败: {e}"))?
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
fn get_hotkeys(status: State<'_, Mutex<HotkeyStatus>>) -> Result<HotkeyStatus, String> {
    status.lock().map(|s| s.clone()).map_err(|_| "快捷键状态锁异常".into())
}

#[tauri::command]
fn get_preferences(state: State<'_, PreferencesState>) -> Result<Preferences, String> { state.get() }

fn parse_hotkey(input: &str) -> Result<Shortcut, String> {
    // Only Ctrl/Alt/Shift + one letter/digit/F-key; reject disruptive bare keys.
    let segments: Vec<&str> = input.split('+').collect();
    if segments.len() < 2 || segments.len() > 4 { return Err("请使用 Ctrl/Alt/Shift + 字母、数字或 F1-F12".into()); }
    let key = segments.last().unwrap().to_ascii_uppercase();
    let valid_key = (key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric()))
        || (key.starts_with('F') && key[1..].parse::<u8>().map(|n| (1..=12).contains(&n)).unwrap_or(false));
    if !valid_key { return Err("快捷键只支持字母、数字或 F1-F12".into()); }
    let mods = &segments[..segments.len()-1];
    if !mods.iter().any(|m| m.eq_ignore_ascii_case("Ctrl") || m.eq_ignore_ascii_case("Alt")) {
        return Err("至少需要 Ctrl 或 Alt 修饰键".into());
    }
    for (index, part) in mods.iter().enumerate() {
        if !["Ctrl", "Alt", "Shift"].iter().any(|v| part.eq_ignore_ascii_case(v))
            || mods[..index].iter().any(|prev| prev.eq_ignore_ascii_case(part)) {
            return Err("修饰键无效或重复".into());
        }
    }
    Shortcut::from_str(input).map_err(|e| format!("快捷键无效: {e}"))
}

#[tauri::command]
fn save_preferences(
    app: AppHandle, state: State<'_, PreferencesState>,
    theme: String, panel_shortcut: String, capture_shortcut: String,
) -> Result<Preferences, String> {
    if theme != "light" && theme != "dark" { return Err("仅支持亮色或暗色主题".into()); }
    let panel_new = parse_hotkey(&panel_shortcut)?;
    let capture_new = parse_hotkey(&capture_shortcut)?;
    if panel_new == capture_new { return Err("两个操作不能使用相同的快捷键".into()); }
    let previous = state.get()?;
    let panel_old = parse_hotkey(&previous.panel_shortcut)?;
    let capture_old = parse_hotkey(&previous.capture_shortcut)?;
    let change_panel = panel_new != panel_old;
    let change_capture = capture_new != capture_old;
    // Register before unregistering the old keys. A failure (key owned by
    // another app) keeps the original working shortcuts and settings intact.
    let manager = app.global_shortcut();
    if change_panel { manager.register(panel_new).map_err(|e| format!("唤出快捷键被占用: {e}"))?; }
    if change_capture {
        if let Err(e) = manager.register(capture_new) {
            if change_panel { let _ = manager.unregister(panel_new); }
            return Err(format!("收藏快捷键被占用: {e}"));
        }
    }
    let mut next = previous.clone();
    next.theme = theme;
    next.panel_shortcut = panel_shortcut;
    next.capture_shortcut = capture_shortcut;
    if let Err(e) = state.save(next.clone()) {
        if change_panel { let _ = manager.unregister(panel_new); }
        if change_capture { let _ = manager.unregister(capture_new); }
        return Err(e);
    }
    if change_panel { let _ = manager.unregister(panel_old); }
    if change_capture { let _ = manager.unregister(capture_old); }
    if let Ok(mut status) = app.state::<Mutex<HotkeyStatus>>().lock() {
        if change_panel { status.panel = true; }
        if change_capture { status.capture = true; }
    }
    let _ = app.emit("preferences-changed", next.clone());
    Ok(next)
}

#[tauri::command]
fn save_panel_size(state: State<'_, PreferencesState>, width: u32, height: u32) -> Result<(), String> {
    let mut prefs = state.get()?;
    let width = width.clamp(420, 1600);
    let height = height.clamp(455, 1200);
    if prefs.panel_width.abs_diff(width) < 2 && prefs.panel_height.abs_diff(height) < 2 { return Ok(()); }
    prefs.panel_width = width;
    prefs.panel_height = height;
    state.save(prefs)
}

fn show_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        w.show().map_err(|e| e.to_string())?;
        w.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("贴贴 · 设置")
        .inner_size(450.0, 530.0)
        .min_inner_size(410.0, 460.0)
        .center().transparent(true).decorations(false).shadow(false)
        .always_on_top(true).resizable(false).skip_taskbar(true)
        .build().map_err(|e| format!("打开设置失败: {e}"))?;
    Ok(())
}

#[tauri::command]
async fn open_settings(app: AppHandle) -> Result<(), String> { show_settings(&app) }

#[tauri::command]
fn hide_settings(app: AppHandle) -> Result<(), String> {
    app.get_webview_window("settings").ok_or("设置窗口不存在")?.hide().map_err(|e| e.to_string())
}

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

// v0.4.6: 彩虹屁词库。默认词库随二进制内置，首次调用时种子化到数据目录的
// praises.json，用户可直接编辑该文件增删词条；读取失败时回退内置默认。
const DEFAULT_PRAISES: &str = include_str!("../praises_default.json");

#[tauri::command]
fn load_praises(store: State<'_, DataStore>) -> Vec<String> {
    let path = store.root.join("praises.json");
    if !path.exists() {
        let _ = std::fs::write(&path, DEFAULT_PRAISES);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| serde_json::from_str(DEFAULT_PRAISES).unwrap()),
        Err(_) => serde_json::from_str(DEFAULT_PRAISES).unwrap(),
    }
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
        // v0.4.5: 单实例保护。重复启动时不再创建第二个托盘图标，
        // 而是唤醒已运行实例并显示主面板。必须第一个注册。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
                let _ = app.emit_to("main", "panel-shown", ());
            }
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed { return; }
            let prefs = match app.state::<PreferencesState>().get() { Ok(p) => p, Err(_) => return };
            if parse_hotkey(&prefs.panel_shortcut).map(|key| key == *shortcut).unwrap_or(false) {
                toggle_panel(app);
            } else if parse_hotkey(&prefs.capture_shortcut).map(|key| key == *shortcut).unwrap_or(false) {
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
            let preferences = PreferencesState::open(&app.path().app_data_dir()?)
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            let saved = preferences.get().map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(preferences);
            if let Some(main) = app.get_webview_window("main") {
                let _ = main.set_size(LogicalSize::new(saved.panel_width as f64, saved.panel_height as f64));
            }

            let toggle = MenuItem::with_id(app, "toggle", "打开 / 隐藏贴贴", true, None::<&str>)?;
            let capture = MenuItem::with_id(app, "capture", "收藏剪贴板", true, None::<&str>)?;
            let settings = MenuItem::with_id(app, "settings", "设置 · 快捷键与主题", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出贴贴", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &capture, &settings, &quit])?;
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
                    "settings" => {
                        // 不能在托盘菜单事件（主线程事件循环内）同步创建窗口，
                        // 否则 build() 阻塞等待事件循环处理 → 死锁。放到后台线程。
                        let handle = handle.clone();
                        std::thread::spawn(move || {
                            if let Err(e) = show_settings(&handle) { eprintln!("设置打开失败: {e}"); }
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

            let p = parse_hotkey(&saved.panel_shortcut).ok()
                .map(|key| app.global_shortcut().register(key).is_ok()).unwrap_or(false);
            let c = parse_hotkey(&saved.capture_shortcut).ok()
                .map(|key| app.global_shortcut().register(key).is_ok()).unwrap_or(false);
            if !p { eprintln!("贴贴: 唤出快捷键不可用，请通过托盘打开设置"); }
            if !c { eprintln!("贴贴: 收藏快捷键不可用，请通过托盘打开设置"); }
            app.manage(Mutex::new(HotkeyStatus { panel: p, capture: c }));
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
            list_notes, save_text, save_todo, capture_clipboard, paste_image_data, get_image_data, get_thumbnail_data,
            delete_note, restore_note, open_float, hide_float, update_float,
            get_hotkeys, get_data_directory, hide_panel, show_note_in_panel, load_praises,
            get_preferences, save_preferences, save_panel_size, open_settings, hide_settings
        ])
        .run(tauri::generate_context!())
        .expect("贴贴启动失败");
}
