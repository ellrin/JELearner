//! JELearner core: everything that needs no web server.
//!
//! - [`workspace`] — the user's private workspace (their own private repo)
//! - [`db`] — SQLite (words, grammar, progress, mistakes, lessons, articles)
//! - [`import`] — CSV / weekly JSON import, progress records export
//! - [`robots`], [`crawler`], [`news`] — polite news crawling
//! - [`provider`] — language-model providers (Claude API, llama-server, mock)
//! - [`claude_code`] — Claude subscription engine through the official Claude Code CLI
//! - [`lesson`] — article -> structured lesson -> Markdown (templates in files)
//! - [`course`] — courses: outlines (topic, fixed…), lesson plans, unit plans
//! - [`google`] — Gmail / Calendar traits, REST and in-memory implementations
//! - [`report`] — weekly report, lesson confirmation, review scheduling
//! - [`level`] — fine-grained levels and difficulty scores
//! - [`catalog`] — local model catalog (for the Mac)
//! - [`rules`], [`points`], [`progress`] — points, levels, difficulty (stage A)
//! - [`topics`], [`conversation`] — topics and the typed AI conversation
//! - [`notebook`] — the review notebook: one page per lesson / conversation
//! - [`speech`] — speech engine trait (browser speech now; native / Whisper / Kokoro later)

pub mod catalog;
pub mod claude_code;
pub mod conversation;
pub mod course;
pub mod crawler;
pub mod db;
pub mod google;
pub mod import;
pub mod lang;
pub mod lesson;
pub mod level;
pub mod news;
pub mod notebook;
pub mod points;
pub mod progress;
pub mod provider;
pub mod report;
pub mod robots;
pub mod rules;
pub mod speech;
pub mod srs;
pub mod topics;
pub mod vcs;
pub mod workspace;

pub use lang::Lang;
