use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf, sync::Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub theme: String,
    pub panel_shortcut: String,
    pub capture_shortcut: String,
    pub panel_width: u32,
    pub panel_height: u32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            panel_shortcut: "Ctrl+Alt+N".into(),
            capture_shortcut: "Ctrl+Alt+V".into(),
            panel_width: 520,
            panel_height: 670,
        }
    }
}

pub struct PreferencesState {
    pub path: PathBuf,
    pub current: Mutex<Preferences>,
}
impl PreferencesState {
    pub fn open(root: &std::path::Path) -> Result<Self, String> {
        fs::create_dir_all(root).map_err(|e| format!("无法建立设置目录: {e}"))?;
        let path = root.join("preferences.json");
        let mut prefs: Preferences = if path.exists() {
            serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| format!("设置文件无法解析（请备份 preferences.json 后重试）: {e}"))?
        } else { Preferences::default() };
        if !matches!(prefs.theme.as_str(), "light" | "dark") { prefs.theme = "light".into(); }
        prefs.panel_width = prefs.panel_width.clamp(420, 1600);
        prefs.panel_height = prefs.panel_height.clamp(455, 1200);
        Ok(Self { path, current: Mutex::new(prefs) })
    }
    pub fn get(&self) -> Result<Preferences, String> {
        self.current.lock().map(|v| v.clone()).map_err(|_| "设置锁异常".into())
    }
    pub fn save(&self, next: Preferences) -> Result<(), String> {
        // Preferences are separate from the user's notes and image records.
        let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        let temporary = self.path.with_extension("json.tmp");
        fs::write(&temporary, bytes).map_err(|e| format!("无法写入设置: {e}"))?;
        // Windows won't replace the target via rename if it already exists.
        if self.path.exists() {
            fs::remove_file(&self.path).map_err(|e| format!("无法替换旧设置: {e}"))?;
        }
        fs::rename(&temporary, &self.path).map_err(|e| format!("无法完成设置保存: {e}"))?;
        *self.current.lock().map_err(|_| "设置锁异常".to_string())? = next;
        Ok(())
    }
}
