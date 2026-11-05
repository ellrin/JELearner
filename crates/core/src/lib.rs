//! JELearner core: everything that needs no web server.
//!
//! - [`workspace`] — the user's private workspace (their own private repo)
//! - [`db`] — SQLite (words, grammar, progress, mistakes, lessons, articles)
//! - [`import`] — CSV / weekly JSON import, progress records export
//! - [`robots`], [`crawler`], [`news`] — polite news crawling
//! - [`provider`] — language-model providers (Claude API, llama-server, mock)
//! - [`claude_code`] — Claude subscription engine through the official Claude Code CLI
//! - [`lesson`] — article -> structured lesson -> Markdown (templates in files)
//! - [`google`] — Gmail / Calendar traits, REST and in-memory implementations
//! - [`report`] — weekly report, lesson confirmation, review scheduling
//! - [`level`] — fine-grained levels and difficulty scores
//! - [`catalog`] — local model catalog (for the Mac)

pub mod catalog;
pub mod claude_code;
pub mod crawler;
pub mod db;
pub mod google;
pub mod import;
pub mod lang;
pub mod lesson;
pub mod level;
pub mod news;
pub mod provider;
pub mod report;
pub mod robots;
pub mod srs;
pub mod vcs;
pub mod workspace;

pub use lang::Lang;
