//! The private workspace: a folder (ideally a private Git repository such as
//! `JELearner-private`) shared by one or more users (docs/plan.md §2).
//!
//! ```text
//! workspace.toml          id, name, admins, members
//! sources.toml            news sources (shared)
//! topics.toml             general topics (shared)
//! prompts/                prompt overrides (shared)
//! profiles/<id>/
//!   profile.toml          name, colour, languages, levels, daily goal…
//!   interests.csv         interest keywords
//!   vocab/ grammar/ materials/ lessons/en|ja/ conversations/ records/
//! ```
//!
//! The first version kept vocab/, grammar/… at the top level for a single
//! user; [`Root::migrate_v1`] moves them into `profiles/<id>/` (with
//! `git mv` in a repository) without deleting anything.
//!
//! SQLite is not in the workspace: one database per workspace and user in
//! the OS config folder, rebuilt from these files.

use crate::lang::Lang;
use crate::level;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const META_FILE: &str = "workspace.toml";
pub const PROFILE_FILE: &str = "profile.toml";
/// Folder of the template copied into each new user's profile.
pub const TEMPLATE_PROFILE_DIR: &str = "profile";
/// Top-level folders of a first-version (single-user) workspace.
pub const V1_DIRS: &[&str] = &["vocab", "grammar", "materials", "lessons", "records", "avatar"];

/// UI colour themes (web/themes.css): ten dark then ten light; in each, five
/// single-hue themes then five combinations. A profile may only store one of
/// these ids.
pub const THEMES: &[&str] = &[
    "hacker", "amber", "ice", "ultraviolet", "redalert", "neon", "shibuya", "hongkong", "synthwave", "aurora",
    "lavender", "lemon", "mint", "blueberry", "strawberry", "giftbox", "peachlavender", "sakuramatcha", "blueberrylemon",
    "caramelpistachio",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MemberKind {
    Owner,
    Household,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    pub kind: MemberKind,
    /// Approved deploy keys (stage 0-2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkspaceMeta {
    /// Stable id; names this workspace's databases. Generated on first use.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub admins: Vec<String>,
    #[serde(default, rename = "member")]
    pub members: Vec<Member>,

    // First-version fields, read only to migrate them into a profile.
    #[serde(default, skip_serializing)]
    pub owner: Option<String>,
    #[serde(default, skip_serializing)]
    pub languages: Option<Vec<Lang>>,
    #[serde(default, skip_serializing)]
    pub native_language: Option<String>,
    #[serde(default, skip_serializing)]
    pub levels: Option<BTreeMap<Lang, String>>,
    #[serde(default, skip_serializing)]
    pub report_email: Option<String>,
    #[serde(default, skip_serializing)]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LangProfile {
    /// Level reached (only goes up), e.g. `B1-3`.
    pub level: String,
    /// Content difficulty score (adjusts both ways), see [`level`].
    pub difficulty: u32,
    /// How many more lessons ask "too easy / right / too hard".
    #[serde(default = "five")]
    pub early_feedback_left: u32,
    /// When the current level was reached (RFC 3339); points since then
    /// count towards the next step. `None` = since the beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level_since: Option<String>,
}

fn five() -> u32 {
    5
}

impl LangProfile {
    /// Start from a self-assessed major level (`B1`, `N5`).
    pub fn starting_at(lang: Lang, major: &str) -> LangProfile {
        let start = level::major_start(lang, major).unwrap_or(0);
        LangProfile { level: level::from_score(lang, start).label, difficulty: start, early_feedback_left: 5, level_since: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConversationSettings {
    #[serde(default = "secs")]
    pub seconds_per_turn: u32,
    #[serde(default = "turns")]
    pub turns: u32,
    /// Share of interest topics; the rest are general topics.
    #[serde(default = "ratio")]
    pub interest_ratio: f32,
}

fn secs() -> u32 {
    15
}
fn turns() -> u32 {
    20
}
fn ratio() -> f32 {
    0.6
}

impl Default for ConversationSettings {
    fn default() -> Self {
        ConversationSettings { seconds_per_turn: secs(), turns: turns(), interest_ratio: ratio() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_langs")]
    pub languages: Vec<Lang>,
    #[serde(default = "default_native")]
    pub native_language: String,
    #[serde(default = "default_minutes")]
    pub daily_minutes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_email: Option<String>,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(default)]
    pub lang: BTreeMap<Lang, LangProfile>,
    #[serde(default)]
    pub conversation: ConversationSettings,
    /// UI colour theme (one of `THEMES`); follows the user across browsers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

fn default_color() -> String {
    "#ff6b35".into()
}
fn default_langs() -> Vec<Lang> {
    vec![Lang::En]
}
fn default_native() -> String {
    "zh-TW".into()
}
fn default_minutes() -> u32 {
    30
}
fn default_tz() -> String {
    "Asia/Taipei".into()
}

impl Profile {
    pub fn new(id: &str, name: &str) -> Profile {
        Profile {
            id: id.into(),
            name: name.into(),
            color: default_color(),
            languages: default_langs(),
            native_language: default_native(),
            daily_minutes: default_minutes(),
            report_email: None,
            timezone: default_tz(),
            lang: BTreeMap::new(),
            conversation: ConversationSettings::default(),
            theme: None,
        }
    }

    /// This language's settings; a learner who has not set it starts at the
    /// language's default major level.
    pub fn lang(&self, lang: Lang) -> LangProfile {
        self.lang.get(&lang).cloned().unwrap_or_else(|| LangProfile::starting_at(lang, lang.default_major()))
    }

    /// Current content difficulty as a level.
    pub fn difficulty(&self, lang: Lang) -> level::Level {
        level::from_score(lang, self.lang(lang).difficulty)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("找不到工作區：{0}")]
    Missing(String),
    #[error("{0} 格式錯誤：{1}")]
    Format(String, String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// What kind of folder a workspace root is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    /// Multi-user (`profiles/`).
    V2,
    /// First version: single user, data at the top level.
    V1,
}

/// A short random-looking id without pulling in a UUID crate.
pub fn new_id() -> String {
    format!("ws-{:016x}", random64())
}

fn random64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(nanos);
    h.write_u32(std::process::id());
    h.finish()
}

/// A profile id from a display name: ASCII letters, digits and `-`; other
/// names get a short generated id.
pub fn profile_id(name: &str, taken: &[String]) -> String {
    let base: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() { format!("user-{:04x}", random64() & 0xffff) } else { base.chars().take(24).collect() };
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn valid_profile_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !id.starts_with('-')
}

/// The workspace as a whole (no user chosen yet).
#[derive(Debug, Clone)]
pub struct Root {
    pub root: PathBuf,
    pub meta: WorkspaceMeta,
}

impl Root {
    /// Open an existing workspace (a folder with `workspace.toml`).
    pub fn open(root: &Path) -> Result<Root, WorkspaceError> {
        let file = root.join(META_FILE);
        if !file.exists() {
            return Err(WorkspaceError::Missing(root.display().to_string()));
        }
        let raw = std::fs::read_to_string(&file)?;
        let mut meta: WorkspaceMeta = toml::from_str(&raw).map_err(|e| WorkspaceError::Format(META_FILE.into(), e.to_string()))?;
        let mut r = Root { root: root.to_path_buf(), meta: meta.clone() };
        if meta.id.trim().is_empty() {
            meta.id = new_id();
            r.meta.id = meta.id.clone();
            // Keep first-version fields in the file until migration reads them.
            if r.layout() == Layout::V2 {
                r.save()?;
            } else {
                set_toml_id(&file, &meta.id)?;
            }
        }
        Ok(r)
    }

    /// Create (or complete) a workspace from the template: copies files that
    /// do not exist yet, never overwrites, leaves out the per-user template.
    pub fn init(root: &Path, template: &Path) -> Result<Root, WorkspaceError> {
        std::fs::create_dir_all(root)?;
        copy_missing_except(template, root, &[TEMPLATE_PROFILE_DIR])?;
        std::fs::create_dir_all(root.join("prompts"))?;
        std::fs::create_dir_all(root.join("profiles"))?;
        if !root.join(META_FILE).exists() {
            Root { root: root.to_path_buf(), meta: WorkspaceMeta::default() }.save()?;
        }
        Root::open(root)
    }

    pub fn save(&self) -> Result<(), WorkspaceError> {
        let body = toml::to_string_pretty(&self.meta).map_err(|e| WorkspaceError::Format(META_FILE.into(), e.to_string()))?;
        let header = "# JELearner workspace. `id` names the local databases; keep it stable.\n\
                      # Members and admins are managed in the app (設定 → 使用者).\n\n";
        std::fs::write(self.root.join(META_FILE), format!("{header}{body}"))?;
        Ok(())
    }

    pub fn layout(&self) -> Layout {
        let has_v1 = V1_DIRS.iter().filter(|d| **d != "avatar").any(|d| self.root.join(d).is_dir());
        if has_v1 && self.list_profiles().is_empty() { Layout::V1 } else { Layout::V2 }
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// Users: members listed in workspace.toml that have a profile folder,
    /// plus profile folders not (yet) listed — another device may have
    /// added one.
    pub fn list_profiles(&self) -> Vec<Profile> {
        let mut out: Vec<Profile> = Vec::new();
        let Ok(rd) = std::fs::read_dir(self.profiles_dir()) else { return out };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if !e.path().is_dir() || !valid_profile_id(&id) {
                continue;
            }
            if let Ok(p) = read_profile(&e.path(), &id) {
                out.push(p);
            }
        }
        // Members order first, then the rest by name.
        let order = |p: &Profile| self.meta.members.iter().position(|m| m.id == p.id).unwrap_or(usize::MAX);
        out.sort_by(|a, b| order(a).cmp(&order(b)).then(a.name.cmp(&b.name)));
        out
    }

    pub fn is_admin(&self, id: &str) -> bool {
        self.meta.admins.iter().any(|a| a == id)
    }

    /// Add a user: profile folder from the template, profile.toml, member
    /// entry. The first user becomes owner and admin.
    pub fn create_profile(&mut self, template: &Path, mut profile: Profile) -> Result<Workspace, WorkspaceError> {
        let taken: Vec<String> = self.list_profiles().into_iter().map(|p| p.id).collect();
        if profile.id.is_empty() {
            profile.id = profile_id(&profile.name, &taken);
        }
        if !valid_profile_id(&profile.id) {
            return Err(WorkspaceError::Invalid(format!("使用者代號只能用英數字與 -：{}", profile.id)));
        }
        if taken.contains(&profile.id) {
            return Err(WorkspaceError::Invalid(format!("已經有使用者 {}", profile.id)));
        }
        if profile.name.trim().is_empty() {
            return Err(WorkspaceError::Invalid("請輸入名字".into()));
        }
        let dir = self.profiles_dir().join(&profile.id);
        let tpl = template.join(TEMPLATE_PROFILE_DIR);
        if tpl.is_dir() {
            copy_missing(&tpl, &dir)?;
        }
        ensure_profile_dirs(&dir)?;
        write_profile(&dir, &profile)?;
        let first = self.meta.members.is_empty();
        if !self.meta.members.iter().any(|m| m.id == profile.id) {
            self.meta.members.push(Member {
                id: profile.id.clone(),
                name: profile.name.clone(),
                kind: if first { MemberKind::Owner } else { MemberKind::Household },
                devices: vec![],
            });
        }
        if self.meta.admins.is_empty() {
            self.meta.admins.push(profile.id.clone());
        }
        if self.meta.name.trim().is_empty() {
            self.meta.name = format!("{} 的工作區", profile.name);
        }
        self.save()?;
        Ok(Workspace { root: self.root.clone(), meta: self.meta.clone(), profile })
    }

    /// Enter as an existing user.
    pub fn user(&self, id: &str) -> Result<Workspace, WorkspaceError> {
        if !valid_profile_id(id) {
            return Err(WorkspaceError::Invalid(format!("沒有這位使用者：{id}")));
        }
        let profile = read_profile(&self.profiles_dir().join(id), id)?;
        Ok(Workspace { root: self.root.clone(), meta: self.meta.clone(), profile })
    }

    /// Move a first-version workspace into `profiles/<id>/` for its user.
    /// Uses `git mv` when the workspace is a Git repository. Never deletes or
    /// overwrites: an item whose target exists is left where it is and
    /// reported. Safe to run again.
    pub fn migrate_v1(&mut self, template: &Path, name: &str, id: Option<&str>) -> Result<Migration, WorkspaceError> {
        if self.layout() != Layout::V1 {
            return Err(WorkspaceError::Invalid("這個工作區已經是多使用者結構".into()));
        }
        let taken: Vec<String> = self.list_profiles().into_iter().map(|p| p.id).collect();
        let id = id.map(str::to_string).unwrap_or_else(|| profile_id(name, &taken));
        let mut profile = Profile::new(&id, name);
        // Carry over first-version settings.
        if let Some(langs) = self.meta.languages.clone() {
            profile.languages = langs;
        }
        if let Some(n) = self.meta.native_language.clone() {
            profile.native_language = n;
        }
        profile.report_email = self.meta.report_email.clone().filter(|e| !e.is_empty());
        if let Some(tz) = self.meta.timezone.clone() {
            profile.timezone = tz;
        }
        for lang in profile.languages.clone() {
            let major = self.meta.levels.as_ref().and_then(|l| l.get(&lang)).cloned().unwrap_or_else(|| lang.default_major().into());
            let major = level::parse(lang, &major).map(|l| l.major.to_string()).unwrap_or_else(|| lang.default_major().into());
            profile.lang.insert(lang, LangProfile::starting_at(lang, &major));
        }
        let dir = self.profiles_dir().join(&id);
        std::fs::create_dir_all(&dir)?;
        let git = self.root.join(".git").exists();
        let mut moved = vec![];
        let mut skipped = vec![];
        for d in V1_DIRS {
            let from = self.root.join(d);
            if !from.exists() {
                continue;
            }
            let to = dir.join(d);
            if to.exists() {
                skipped.push(d.to_string());
                continue;
            }
            let rel_to = format!("profiles/{id}/{d}");
            let ok = git
                && std::process::Command::new("git")
                    .args(["mv", "-k", d, &rel_to])
                    .current_dir(&self.root)
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
                && to.exists();
            if !ok && from.exists() {
                std::fs::rename(&from, &to)?;
            }
            moved.push(d.to_string());
        }
        let tpl = template.join(TEMPLATE_PROFILE_DIR);
        if tpl.is_dir() {
            // Only fills folders that do not exist; existing data is untouched.
            for sub in ["conversations", "photos", "recipes"] {
                let t = tpl.join(sub);
                if t.is_dir() && !dir.join(sub).exists() {
                    copy_missing(&t, &dir.join(sub))?;
                }
            }
        }
        ensure_profile_dirs(&dir)?;
        if !dir.join(PROFILE_FILE).exists() {
            write_profile(&dir, &profile)?;
        }
        if self.meta.name.is_empty() {
            self.meta.name = format!("{name} 的工作區");
        }
        if !self.meta.members.iter().any(|m| m.id == id) {
            self.meta.members.push(Member { id: id.clone(), name: name.into(), kind: MemberKind::Owner, devices: vec![] });
        }
        if self.meta.admins.is_empty() {
            self.meta.admins.push(id.clone());
        }
        // Rewrites workspace.toml without the first-version fields (now in
        // profile.toml).
        self.save()?;
        Ok(Migration { profile: id, moved, skipped })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Migration {
    pub profile: String,
    pub moved: Vec<String>,
    /// Folders left in place because the target already existed.
    pub skipped: Vec<String>,
}

fn set_toml_id(file: &Path, id: &str) -> Result<(), WorkspaceError> {
    let raw = std::fs::read_to_string(file)?;
    let mut doc: toml::Table = raw.parse().map_err(|e: toml::de::Error| WorkspaceError::Format(META_FILE.into(), e.to_string()))?;
    doc.insert("id".into(), toml::Value::String(id.into()));
    std::fs::write(file, toml::to_string(&doc).map_err(|e| WorkspaceError::Format(META_FILE.into(), e.to_string()))?)?;
    Ok(())
}

fn ensure_profile_dirs(dir: &Path) -> Result<(), WorkspaceError> {
    for d in ["vocab", "grammar", "materials", "lessons/en", "lessons/ja", "conversations/en", "conversations/ja", "records/points"] {
        std::fs::create_dir_all(dir.join(d))?;
    }
    Ok(())
}

pub fn read_profile(dir: &Path, id: &str) -> Result<Profile, WorkspaceError> {
    let file = dir.join(PROFILE_FILE);
    let raw = std::fs::read_to_string(&file).map_err(|_| WorkspaceError::Missing(file.display().to_string()))?;
    let mut p: Profile = toml::from_str(&raw).map_err(|e| WorkspaceError::Format(format!("profiles/{id}/{PROFILE_FILE}"), e.to_string()))?;
    // The folder name is the identity.
    p.id = id.to_string();
    Ok(p)
}

pub fn write_profile(dir: &Path, p: &Profile) -> Result<(), WorkspaceError> {
    let body = toml::to_string_pretty(p).map_err(|e| WorkspaceError::Format(PROFILE_FILE.into(), e.to_string()))?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(PROFILE_FILE), format!("# JELearner user settings. Edit in the app or by hand.\n\n{body}"))?;
    Ok(())
}

/// One user inside a workspace: where their files are.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub root: PathBuf,
    pub meta: WorkspaceMeta,
    pub profile: Profile,
}

impl Workspace {
    pub fn profile_dir(&self) -> PathBuf {
        self.root.join("profiles").join(&self.profile.id)
    }
    pub fn save_profile(&self) -> Result<(), WorkspaceError> {
        write_profile(&self.profile_dir(), &self.profile)
    }
    pub fn vocab_dir(&self) -> PathBuf {
        self.profile_dir().join("vocab")
    }
    pub fn grammar_dir(&self) -> PathBuf {
        self.profile_dir().join("grammar")
    }
    pub fn materials_dir(&self) -> PathBuf {
        self.profile_dir().join("materials")
    }
    pub fn lessons_dir(&self, lang: Lang) -> PathBuf {
        self.profile_dir().join("lessons").join(lang.code())
    }
    pub fn conversations_dir(&self, lang: Lang) -> PathBuf {
        self.profile_dir().join("conversations").join(lang.code())
    }
    pub fn records_dir(&self) -> PathBuf {
        self.profile_dir().join("records")
    }
    pub fn interests_file(&self) -> PathBuf {
        self.profile_dir().join("interests.csv")
    }
    /// Prompt overrides are shared by the workspace.
    pub fn prompts_dir(&self) -> PathBuf {
        self.root.join("prompts")
    }
    pub fn sources_file(&self) -> PathBuf {
        self.root.join("sources.toml")
    }
    pub fn topics_file(&self) -> PathBuf {
        self.root.join("topics.toml")
    }
    pub fn points_dir(&self) -> PathBuf {
        self.records_dir().join("points")
    }
    /// A workspace path relative to this user's profile folder, as written
    /// in records/points (`lessons/en/…`, `conversations/ja/…`).
    pub fn profile_relative(&self, path: &Path) -> String {
        let full = if path.is_absolute() { path.to_path_buf() } else { self.root.join(path) };
        full.strip_prefix(self.profile_dir()).unwrap_or(&full).to_string_lossy().replace('\\', "/")
    }

    /// Workspace-relative path for display and for the database.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root).unwrap_or(path).to_string_lossy().replace('\\', "/")
    }
}

/// Copy files that do not exist yet; never overwrite.
pub fn copy_missing(from: &Path, to: &Path) -> std::io::Result<usize> {
    copy_missing_except(from, to, &[])
}

/// [`copy_missing`], leaving out top-level entries named in `skip`.
pub fn copy_missing_except(from: &Path, to: &Path, skip: &[&str]) -> std::io::Result<usize> {
    std::fs::create_dir_all(to)?;
    let mut n = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let target = to.join(&name);
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
        } else if path.extension().and_then(|e| e.to_str()).is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e))) {
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

    fn template() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join(META_FILE), "id = \"\"\n").unwrap();
        std::fs::write(t.path().join("sources.toml"), "").unwrap();
        std::fs::create_dir_all(t.path().join("profile/vocab")).unwrap();
        std::fs::write(t.path().join("profile/vocab/my-words-en.csv"), "term\nsample\n").unwrap();
        t
    }

    #[test]
    fn init_create_users_and_isolate_folders() {
        let tpl = template();
        let ws = tempfile::tempdir().unwrap();
        let mut root = Root::init(ws.path(), tpl.path()).unwrap();
        assert!(root.meta.id.starts_with("ws-"));
        assert!(!ws.path().join("profile").exists(), "per-user template is not copied to the top");
        assert_eq!(root.layout(), Layout::V2);

        let a = root.create_profile(tpl.path(), Profile::new("", "Ellrin")).unwrap();
        let b = root.create_profile(tpl.path(), Profile::new("", "daidai")).unwrap();
        assert_eq!(a.profile.id, "ellrin");
        assert_eq!(b.profile.id, "daidai");
        assert!(a.vocab_dir().join("my-words-en.csv").exists());
        assert_ne!(a.vocab_dir(), b.vocab_dir());
        assert_eq!(root.meta.admins, vec!["ellrin".to_string()]);
        assert_eq!(root.meta.members[1].kind, MemberKind::Household);

        // Reopen: members and profiles survive; same id.
        let again = Root::open(ws.path()).unwrap();
        assert_eq!(again.meta.id, root.meta.id);
        assert_eq!(again.list_profiles().len(), 2);
        assert_eq!(again.user("ellrin").unwrap().profile.name, "Ellrin");
        assert!(again.user("../etc").is_err());
        assert!(root.create_profile(tpl.path(), Profile::new("ellrin", "Again")).is_err());
    }

    #[test]
    fn migrate_first_version_without_losing_files() {
        let tpl = template();
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(
            ws.path().join(META_FILE),
            "id = \"ws-old\"\nowner = \"Ellrin\"\nlanguages = [\"en\", \"ja\"]\nreport_email = \"me@example.com\"\n[levels]\nen = \"B2\"\nja = \"N5\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(ws.path().join("vocab")).unwrap();
        std::fs::write(ws.path().join("vocab/mine.csv"), "term\nmy word\n").unwrap();
        std::fs::create_dir_all(ws.path().join("lessons/en")).unwrap();
        std::fs::write(ws.path().join("lessons/en/a.md"), "# A").unwrap();

        let mut root = Root::open(ws.path()).unwrap();
        assert_eq!(root.layout(), Layout::V1);
        let m = root.migrate_v1(tpl.path(), "Ellrin", None).unwrap();
        assert_eq!(m.profile, "ellrin");
        assert!(m.moved.contains(&"vocab".to_string()));
        assert_eq!(std::fs::read_to_string(ws.path().join("profiles/ellrin/vocab/mine.csv")).unwrap(), "term\nmy word\n");
        assert!(ws.path().join("profiles/ellrin/lessons/en/a.md").exists());
        assert!(!ws.path().join("vocab").exists());

        let root = Root::open(ws.path()).unwrap();
        assert_eq!(root.layout(), Layout::V2);
        assert_eq!(root.meta.id, "ws-old", "same id, same database name");
        let u = root.user("ellrin").unwrap();
        assert_eq!(u.profile.languages, vec![Lang::En, Lang::Ja]);
        assert_eq!(u.profile.lang(Lang::En).level, "B2-1");
        assert_eq!(u.profile.report_email.as_deref(), Some("me@example.com"));
        let meta = std::fs::read_to_string(ws.path().join(META_FILE)).unwrap();
        assert!(!meta.contains("report_email"), "first-version fields moved into profile.toml");

        // Running again is refused, nothing changes.
        let mut root = Root::open(ws.path()).unwrap();
        assert!(root.migrate_v1(tpl.path(), "Ellrin", None).is_err());
    }

    #[test]
    fn profile_ids() {
        assert_eq!(profile_id("Ellrin Chung", &[]), "ellrin-chung");
        assert_eq!(profile_id("ellrin", &["ellrin".into()]), "ellrin-2");
        assert!(profile_id("測試", &[]).starts_with("user-"));
    }

    #[test]
    fn slug_keeps_cjk() {
        assert_eq!(slug("AI chips: What's next?", 40), "ai-chips-what-s-next");
        assert_eq!(slug("生成AIの規制", 40), "生成aiの規制");
        assert_eq!(slug("!!!", 40), "untitled");
    }

    /// WCAG contrast ratio of two `#rrggbb` colours.
    fn contrast(a: &str, b: &str) -> f64 {
        let lum = |h: &str| {
            let c: Vec<f64> = (0..3)
                .map(|i| {
                    let v = u8::from_str_radix(&h[1 + 2 * i..3 + 2 * i], 16).unwrap() as f64 / 255.0;
                    if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
                })
                .collect();
            0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// docs/design.md「主題」: text ≥ 7:1, muted ≥ 4.5:1 on panel, every
    /// accent's -text ≥ 4.5:1 on bg and panel, every -ink ≥ 4.5:1 on its fill.
    #[test]
    fn themes_meet_the_contrast_rules() {
        let css = include_str!("../../../web/themes.css");
        for id in THEMES {
            let start = css.find(&format!(":root[data-theme=\"{id}\"] {{")).unwrap();
            let block = &css[start..start + css[start..].find("\n}").unwrap()];
            let tok = |name: &str| {
                let key = format!("--{name}: #");
                let i = block.find(&key).unwrap_or_else(|| panic!("{id}: no {name}")) + key.len() - 1;
                &block[i..i + 7]
            };
            let (bg, panel) = (tok("bg"), tok("panel"));
            assert!(contrast(tok("text"), bg) >= 7.0 && contrast(tok("text"), panel) >= 7.0, "{id}: text");
            assert!(contrast(tok("muted"), panel) >= 4.5, "{id}: muted on panel");
            for a in ["accent", "accent-2", "accent-3"] {
                let t = tok(&format!("{a}-text"));
                assert!(contrast(t, bg) >= 4.5 && contrast(t, panel) >= 4.5, "{id}: {a}-text {t}");
                let (fill, ink) = (tok(a), tok(&format!("{a}-ink")));
                assert!(contrast(ink, fill) >= 4.5, "{id}: {a}-ink {ink} on {fill}");
            }
        }
    }

    #[test]
    fn theme_ids_match_the_stylesheet() {
        let css = include_str!("../../../web/themes.css");
        let app = include_str!("../../../web/app.js");
        assert_eq!(THEMES.len(), 20);
        for id in THEMES {
            assert!(css.contains(&format!(":root[data-theme=\"{id}\"]")), "{id} missing in themes.css");
            assert!(app.contains(&format!("id: \"{id}\"")), "{id} missing in app.js");
        }
        assert_eq!(css.matches(":root[data-theme=").count(), THEMES.len(), "themes.css has a theme not in THEMES");
        for token in [
            "--accent:", "--accent-2:", "--accent-3:", "--accent-2-ink:", "--accent-3-ink:",
            "--accent-text:", "--accent-2-text:", "--accent-3-text:",
        ] {
            assert_eq!(css.matches(token).count(), THEMES.len(), "every theme defines {token}");
        }
        // Old profiles without a theme still load, and none is written back.
        let p: Profile = toml::from_str("id = \"ellrin\"\nname = \"Ellrin\"\n").unwrap();
        assert_eq!(p.theme, None);
        assert!(!toml::to_string(&p).unwrap().contains("theme"));
    }
}
