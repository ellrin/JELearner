//! The private workspace: a folder (ideally the user's own private Git
//! repository, e.g. `JELearner-private`) holding everything personal that is
//! worth keeping and syncing as plain files:
//!
//! ```text
//! workspace.toml        id, owner, levels, report e-mail
//! sources.toml          news sources (RSS / Atom)
//! vocab/*.csv           words I want to learn
//! grammar/*.csv         grammar points I want to learn
//! materials/<week>/*.json  weekly study material I collect
//! lessons/<lang>/*.md   generated lessons
//! records/*.csv         exported progress and mistakes (portable backup)
//! prompts/              optional overrides of the bundled prompt templates
//! avatar/               avatar settings (later stage)
//! ```
//!
//! The SQLite database is *not* in the workspace: it lives in the OS config
//! folder, keyed by the workspace id, and is rebuilt from these files.

use crate::lang::Lang;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const META_FILE: &str = "workspace.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceMeta {
    /// Stable id; names this workspace's database. Generated on first use.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub owner: Option<String>,
    /// Languages this user studies.
    #[serde(default = "default_langs")]
    pub languages: Vec<Lang>,
    /// Language explanations are written in.
    #[serde(default = "default_native")]
    pub native_language: String,
    /// Target level per language, e.g. `en = "B2"`, `ja = "N3"`.
    #[serde(default)]
    pub levels: BTreeMap<Lang, String>,
    /// Where weekly reports and reminders go (never committed by the app
    /// unless the user writes it here themselves).
    #[serde(default)]
    pub report_email: Option<String>,
    /// IANA time zone for calendar events, e.g. "Asia/Taipei".
    #[serde(default = "default_tz")]
    pub timezone: String,
}

fn default_langs() -> Vec<Lang> {
    vec![Lang::En, Lang::Ja]
}
fn default_native() -> String {
    "zh-TW".into()
}
fn default_tz() -> String {
    "Asia/Taipei".into()
}

impl Default for WorkspaceMeta {
    fn default() -> Self {
        WorkspaceMeta {
            id: String::new(),
            owner: None,
            languages: default_langs(),
            native_language: default_native(),
            levels: BTreeMap::new(),
            report_email: None,
            timezone: default_tz(),
        }
    }
}

impl WorkspaceMeta {
    pub fn level(&self, lang: Lang) -> String {
        self.levels.get(&lang).cloned().unwrap_or_else(|| lang.default_level().to_string())
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub meta: WorkspaceMeta,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("找不到工作區：{0}")]
    Missing(String),
    #[error("workspace.toml 格式錯誤：{0}")]
    Meta(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// A short random-looking id without pulling in a UUID crate.
pub fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    format!("ws-{:016x}", h.finish())
}

impl Workspace {
    /// Open an existing workspace. A folder without `workspace.toml` is not a
    /// workspace; use [`Workspace::init`] to create or complete one.
    pub fn open(root: &Path) -> Result<Workspace, WorkspaceError> {
        let file = root.join(META_FILE);
        if !file.exists() {
            return Err(WorkspaceError::Missing(root.display().to_string()));
        }
        let raw = std::fs::read_to_string(&file)?;
        let mut meta: WorkspaceMeta = toml::from_str(&raw).map_err(|e| WorkspaceError::Meta(e.to_string()))?;
        if meta.id.trim().is_empty() {
            // A template copy or a hand-written file: give it an id once.
            meta.id = new_id();
            write_meta(root, &meta)?;
        }
        Ok(Workspace { root: root.to_path_buf(), meta })
    }

    /// Create the folder layout from the template, copying only files that do
    /// not exist yet (never overwrites the user's files), then open it.
    pub fn init(root: &Path, template: &Path) -> Result<Workspace, WorkspaceError> {
        std::fs::create_dir_all(root)?;
        copy_missing(template, root)?;
        for dir in ["vocab", "grammar", "materials", "lessons/en", "lessons/ja", "records", "prompts"] {
            std::fs::create_dir_all(root.join(dir))?;
        }
        if !root.join(META_FILE).exists() {
            write_meta(root, &WorkspaceMeta::default())?;
        }
        Workspace::open(root)
    }

    pub fn save_meta(&self) -> Result<(), WorkspaceError> {
        write_meta(&self.root, &self.meta)
    }

    pub fn vocab_dir(&self) -> PathBuf {
        self.root.join("vocab")
    }
    pub fn grammar_dir(&self) -> PathBuf {
        self.root.join("grammar")
    }
    pub fn materials_dir(&self) -> PathBuf {
        self.root.join("materials")
    }
    pub fn lessons_dir(&self, lang: Lang) -> PathBuf {
        self.root.join("lessons").join(lang.code())
    }
    pub fn records_dir(&self) -> PathBuf {
        self.root.join("records")
    }
    pub fn prompts_dir(&self) -> PathBuf {
        self.root.join("prompts")
    }
    pub fn sources_file(&self) -> PathBuf {
        self.root.join("sources.toml")
    }

    /// Workspace-relative path for display and for the database.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }
}

fn write_meta(root: &Path, meta: &WorkspaceMeta) -> Result<(), WorkspaceError> {
    let body = toml::to_string_pretty(meta).map_err(|e| WorkspaceError::Meta(e.to_string()))?;
    let header = "# JELearner private workspace. This file is yours; edit freely.\n\
                  # `id` names this workspace's local database; keep it stable.\n\n";
    std::fs::write(root.join(META_FILE), format!("{header}{body}"))?;
    Ok(())
}

/// Copy files that do not exist yet; never overwrite.
pub fn copy_missing(from: &Path, to: &Path) -> std::io::Result<usize> {
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            n += copy_missing(&entry.path(), &target)?;
        } else if !target.exists() {
            std::fs::copy(entry.path(), target)?;
            n += 1;
        }
    }
    Ok(n)
}

/// Files with one of the given extensions under `dir`, recursively, sorted.
pub fn files_with_ext(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            out.extend(files_with_ext(&path, exts));
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
        {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// A safe file stem from user text: letters, digits, CJK, `-`, `_`.
pub fn slug(text: &str, max: usize) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            for l in c.to_lowercase() {
                out.push(l);
            }
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.chars().count() >= max {
            break;
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() { "untitled".into() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_never_overwrites_and_assigns_id() {
        let tpl = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tpl.path().join("vocab")).unwrap();
        std::fs::write(tpl.path().join("vocab/words.csv"), "template").unwrap();
        std::fs::write(tpl.path().join(META_FILE), "id = \"\"\n").unwrap();

        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("vocab")).unwrap();
        std::fs::write(ws.path().join("vocab/words.csv"), "mine").unwrap();

        let w = Workspace::init(ws.path(), tpl.path()).unwrap();
        assert!(w.meta.id.starts_with("ws-"));
        assert_eq!(std::fs::read_to_string(ws.path().join("vocab/words.csv")).unwrap(), "mine");
        // The id is persisted, so reopening gives the same database.
        assert_eq!(Workspace::open(ws.path()).unwrap().meta.id, w.meta.id);
        assert!(ws.path().join("lessons/ja").is_dir());
    }

    #[test]
    fn slug_keeps_cjk() {
        assert_eq!(slug("AI chips: What's next?", 40), "ai-chips-what-s-next");
        assert_eq!(slug("生成AIの規制", 40), "生成aiの規制");
        assert_eq!(slug("!!!", 40), "untitled");
    }
}
