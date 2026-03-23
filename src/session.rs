use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::schema::SchemaOverview;
use crate::types::TemporalOverride;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumConversion {
    pub field_name: String,
    pub enum_name: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub created_at: String,
    pub modified_at: String,
    pub files: Vec<SessionFile>,
    #[serde(default = "default_jaccard")]
    pub jaccard_threshold: f32,
    #[serde(default)]
    pub enum_conversions: Vec<EnumConversion>,
    #[serde(default)]
    pub hidden_fields: Vec<String>, // "StructName.field_name" format
}

fn default_jaccard() -> f32 {
    0.8
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FileSource {
    Json,
    Har,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFile {
    pub id: String,
    pub original_path: String,
    pub filename: String,
    pub imported_at: String,
    pub content: String,
    #[serde(default = "default_source")]
    pub source: FileSource,
}

fn default_source() -> FileSource {
    FileSource::Json
}

pub struct LoadedSession {
    pub session: Session,
    pub parsed_files: Vec<(String, serde_json::Value)>,
    pub temporal_overrides: BTreeMap<String, TemporalOverride>,
    pub schema: Option<SchemaOverview>,
}

impl LoadedSession {
    pub fn new(session: Session) -> Self {
        let parsed_files: Vec<(String, serde_json::Value)> = session
            .files
            .iter()
            .filter_map(|f| {
                serde_json::from_str(&f.content)
                    .ok()
                    .map(|v| (f.filename.clone(), v))
            })
            .collect();

        let mut loaded = Self {
            schema: None,
            session,
            parsed_files,
            temporal_overrides: BTreeMap::new(),
        };
        loaded.rebuild_schema();
        loaded
    }

    pub fn rebuild_schema(&mut self) {
        if !self.parsed_files.is_empty() {
            let threshold = self.session.jaccard_threshold;
            self.schema = Some(SchemaOverview::infer(&self.parsed_files, threshold));
        } else {
            self.schema = None;
        }
    }

    pub fn add_file(&mut self, path: &str, content: String, source: FileSource) -> Result<(), String> {
        let value: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("Invalid JSON: {}", e))?;

        let filename = PathBuf::from(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed.json".to_string());

        let now = chrono::Utc::now().to_rfc3339();
        let file = SessionFile {
            id: uuid::Uuid::new_v4().to_string(),
            original_path: path.to_string(),
            filename: filename.clone(),
            imported_at: now.clone(),
            content,
            source,
        };

        self.session.files.push(file);
        self.session.modified_at = now;
        self.parsed_files.push((filename, value));

        Ok(())
    }

    pub fn remove_file(&mut self, index: usize) {
        if index < self.session.files.len() {
            self.session.files.remove(index);
            self.parsed_files.remove(index);
            self.session.modified_at = chrono::Utc::now().to_rfc3339();
        }
    }
}

impl Session {
    pub fn new(name: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: String::new(),
            tags: Vec::new(),
            created_at: now.clone(),
            modified_at: now,
            files: Vec::new(),
            jaccard_threshold: default_jaccard(),
            enum_conversions: Vec::new(),
            hidden_fields: Vec::new(),
        }
    }
}

pub struct SessionManager {
    pub sessions: Vec<Session>,
    data_dir: PathBuf,
    pub last_session_id: Option<String>,
}

impl SessionManager {
    pub fn new() -> Self {
        let data_dir = directories::ProjectDirs::from("com", "jv", "jv")
            .map(|dirs| dirs.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".jv_data"));

        let mut manager = Self {
            sessions: Vec::new(),
            data_dir,
            last_session_id: None,
        };
        manager.load_sessions();
        manager.load_last_session_id();
        manager
    }

    fn sessions_dir(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }

    fn load_sessions(&mut self) {
        let dir = self.sessions_dir();
        if !dir.exists() {
            return;
        }

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(session) = serde_json::from_str::<Session>(&content) {
                            self.sessions.push(session);
                        }
                    }
                }
            }
        }

        self.sessions
            .sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    }

    pub fn save_session(&self, session: &Session) {
        let dir = self.sessions_dir();
        std::fs::create_dir_all(&dir).ok();

        let path = dir.join(format!("{}.json", session.id));
        if let Ok(content) = serde_json::to_string_pretty(session) {
            std::fs::write(path, content).ok();
        }
    }

    pub fn create_session(&mut self, name: &str) -> Session {
        let session = Session::new(name);
        self.save_session(&session);
        self.sessions.insert(0, session.clone());
        session
    }

    pub fn delete_session(&mut self, id: &str) {
        self.sessions.retain(|s| s.id != id);
        let path = self.sessions_dir().join(format!("{}.json", id));
        std::fs::remove_file(path).ok();
    }

    pub fn update_session(&mut self, session: &Session) {
        self.save_session(session);
        if let Some(existing) = self.sessions.iter_mut().find(|s| s.id == session.id) {
            *existing = session.clone();
        }
    }

    fn state_path(&self) -> PathBuf {
        self.data_dir.join("state.json")
    }

    fn load_last_session_id(&mut self) {
        if let Ok(content) = std::fs::read_to_string(self.state_path()) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                self.last_session_id = val["last_session_id"].as_str().map(|s| s.to_string());
            }
        }
    }

    pub fn save_last_session_id(&self, id: &str) {
        let val = serde_json::json!({ "last_session_id": id });
        std::fs::create_dir_all(&self.data_dir).ok();
        if let Ok(content) = serde_json::to_string(&val) {
            std::fs::write(self.state_path(), content).ok();
        }
    }
}
