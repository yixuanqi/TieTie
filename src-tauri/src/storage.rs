use serde::{Deserialize, Serialize};
use std::{fs, io::{self, Write}, path::PathBuf, sync::Mutex, time::{SystemTime, UNIX_EPOCH}};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub done_at: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub kind: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub tasks: Vec<Task>,
    #[serde(default)]
    pub image_name: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub pinned: bool,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub opacity: f64,
    pub always_on_top: bool,
}

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64
}

impl Note {
    pub fn new(kind: &str) -> Self {
        let now = now_ms();
        Self {
            id: Uuid::new_v4().to_string(), kind: kind.into(), text: String::new(),
            tasks: Vec::new(), image_name: None, created_at: now, updated_at: now,
            deleted_at: None, visible: false, pinned: false,
            x: 260, y: 170, w: 350, h: 290, opacity: 1.0, always_on_top: true,
        }
    }
}

pub struct DataStore {
    pub root: PathBuf,
    pub notes: Mutex<Vec<Note>>,
}

impl DataStore {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(root.join("images")).map_err(|e| e.to_string())?;
        let path = root.join("notes.json");
        let notes = if path.is_file() {
            let buf = fs::read(&path).map_err(|e| e.to_string())?;
            // Never silently replace a broken file with an empty collection.
            serde_json::from_slice::<Vec<Note>>(&buf).map_err(|e| format!("数据文件解析失败，请先备份 {}: {e}", path.display()))?
        } else { Vec::new() };
        Ok(Self { root, notes: Mutex::new(notes) })
    }

    pub fn read<R>(&self, f: impl FnOnce(&Vec<Note>) -> R) -> Result<R, String> {
        let notes = self.notes.lock().map_err(|e| e.to_string())?;
        Ok(f(&notes))
    }

    pub fn write<R>(&self, f: impl FnOnce(&mut Vec<Note>) -> Result<R, String>) -> Result<R, String> {
        let mut notes = self.notes.lock().map_err(|e| e.to_string())?;
        let snapshot = notes.clone();
        let result = f(&mut notes)?;
        if let Err(error) = self.flush(&notes) {
            *notes = snapshot;
            return Err(error);
        }
        Ok(result)
    }

    fn flush(&self, notes: &Vec<Note>) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(notes).map_err(|e| e.to_string())?;
        let path = self.root.join("notes.json");
        let temp = self.root.join("notes.json.tmp");
        let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(&data).and_then(|_| file.sync_all()).map_err(|e| e.to_string())?;
        if path.exists() { let _ = fs::copy(&path, self.root.join("notes.json.bak")); }
        // Windows std::fs::rename replaces an existing file on supported filesystems.
        fs::rename(&temp, &path).map_err(|e| format!("保存记录失败: {e}"))?;
        Ok(())
    }

    pub fn image_file(&self, note: &Note) -> Result<PathBuf, String> {
        let name = note.image_name.as_ref().ok_or("这条记录没有图片")?;
        // Only UUID-generated filenames may reach the filesystem.
        if !name.ends_with(".png") || name.contains('/') || name.contains('\\') || name.contains("..") {
            return Err("图片路径无效".into());
        }
        Ok(self.root.join("images").join(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn storage_round_trip_and_backup() {
        let dir = std::env::temp_dir().join(format!("tietie-test-{}", Uuid::new_v4()));
        let store = DataStore::open(dir.clone()).unwrap();
        let n = Note::new("text");
        store.write(|v| { v.push(n.clone()); Ok(()) }).unwrap();
        store.write(|v| { v[0].text = "可搜索的 FFT 文字".into(); Ok(()) }).unwrap();
        let copy = DataStore::open(dir.clone()).unwrap();
        assert_eq!(copy.read(|v| v[0].text.clone()).unwrap(), "可搜索的 FFT 文字");
        assert!(dir.join("notes.json.bak").is_file());
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn rejects_relative_image_paths() {
        let store = DataStore::open(std::env::temp_dir().join(format!("tietie-test-{}", Uuid::new_v4()))).unwrap();
        let mut n = Note::new("image");
        n.image_name = Some("../private.png".into());
        assert!(store.image_file(&n).is_err());
        let _ = fs::remove_dir_all(store.root);
    }
}
