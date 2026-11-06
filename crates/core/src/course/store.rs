//! Courses as files in the user's workspace, two per course:
//!
//! - `profiles/<id>/recipes/<course-id>.toml` — the **outline** (TOML, like
//!   the recipes of docs/plan.md §13, which a course file extends with its
//!   lessons). Written once when the course is created; after that it is
//!   the user's file: the app never rewrites it, so hand edits and comments
//!   survive. Extending a topic course only appends new `[[lesson]]` tables
//!   at the end ([`append_lessons`]).
//! - `profiles/<id>/recipes/<course-id>.progress.toml` — the **progress**
//!   (course status, lesson status, lesson file, units passed), written by
//!   the app ([`save_progress`]).
//!
//! New files always get a free name. Older outline files that still carry
//! progress fields load fine: those values are used until a progress file
//! exists, which then takes precedence. The database can be rebuilt from
//! these files and the lesson files they point to.

use super::model::*;
use super::CourseError;
use crate::workspace::slug;
use crate::workspace::Workspace;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const EXT: &str = "toml";
/// Suffix of the progress file: `<course-id>.progress.toml`.
pub const PROGRESS_SUFFIX: &str = ".progress.toml";

/// `profiles/<id>/recipes/`
pub fn recipes_dir(ws: &Workspace) -> PathBuf {
    ws.profile_dir().join("recipes")
}

/// `dir/stem.ext`, or `dir/stem-2.ext`… if taken. Never overwrites.
pub fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut p = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    p
}

/// Course ids are file stems: letters (also CJK), digits, `-`, `_` (no
/// dots, so a progress file is never taken for a course).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 80 && !id.starts_with(['-', '.']) && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// The outline file of a course.
pub fn outline_path(ws: &Workspace, id: &str) -> Result<PathBuf, CourseError> {
    if !valid_id(id) {
        return Err(CourseError::NotFound(id.to_string()));
    }
    Ok(recipes_dir(ws).join(format!("{id}.{EXT}")))
}

/// The progress file of a course.
pub fn progress_path(ws: &Workspace, id: &str) -> Result<PathBuf, CourseError> {
    if !valid_id(id) {
        return Err(CourseError::NotFound(id.to_string()));
    }
    Ok(recipes_dir(ws).join(format!("{id}{PROGRESS_SUFFIX}")))
}

fn ser<T: Serialize>(value: &T, origin: &str) -> Result<String, CourseError> {
    toml::to_string_pretty(value).map_err(|e| CourseError::Format(origin.to_string(), e.to_string()))
}

const OUTLINE_HEADER: &str = "# JELearner course outline (docs/courses.md). This file is yours: edit titles,\n\
                              # words, grammar or units freely. The app keeps progress in\n\
                              # <this name>.progress.toml and never rewrites this file.\n\n";

pub fn parse(text: &str, origin: &str) -> Result<Course, CourseError> {
    toml::from_str(text).map_err(|e| CourseError::Format(origin.to_string(), e.to_string()))
}

fn write_new(path: &Path, text: &str) -> Result<(), CourseError> {
    // create_new: a file appearing between the check and the write is not overwritten.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    f.write_all(text.as_bytes())?;
    Ok(())
}

/// Save a new course under a free name: `<date>-<slug of name>.toml`
/// (`-2`, `-3`… when taken), plus its progress file. Sets and returns the
/// course id.
pub fn save_new(ws: &Workspace, course: &mut Course) -> Result<String, CourseError> {
    let dir = recipes_dir(ws);
    std::fs::create_dir_all(&dir)?;
    let stem = format!("{}-{}", course.created, slug(&course.name, 40));
    let base = if valid_id(&stem) { stem } else { format!("course-{}", course.created) };
    let mut id = base.clone();
    let mut n = 2;
    // Free for both files (a left-over progress file is not adopted).
    while dir.join(format!("{id}.{EXT}")).exists() || dir.join(format!("{id}{PROGRESS_SUFFIX}")).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    course.id = id.clone();
    let outline = course.outline_only();
    write_new(&outline_path(ws, &id)?, &format!("{OUTLINE_HEADER}{}", ser(&outline, &id)?))?;
    save_progress(ws, course)?;
    Ok(id)
}

/// Write the progress file of a saved course (the outline is not touched).
pub fn save_progress(ws: &Workspace, course: &Course) -> Result<(), CourseError> {
    if !outline_path(ws, &course.id)?.exists() {
        return Err(CourseError::NotFound(course.id.clone()));
    }
    let header = "# JELearner course progress, written by the app. The outline is in the file\n\
                  # of the same name without `.progress`.\n\n";
    std::fs::write(progress_path(ws, &course.id)?, format!("{header}{}", ser(&course.progress(), &course.id)?))?;
    Ok(())
}

#[derive(Serialize)]
struct LessonsOnly {
    #[serde(rename = "lesson")]
    lessons: Vec<LessonPlan>,
}

/// Append lessons (numbers `from` and up, e.g. after
/// [`super::outline::extend_topic_course`]) to the outline file as new
/// `[[lesson]]` tables at its end, leaving everything already there as it
/// is; then save the progress.
pub fn append_lessons(ws: &Workspace, course: &Course, from: u32, date: &str) -> Result<usize, CourseError> {
    let path = outline_path(ws, &course.id)?;
    if !path.exists() {
        return Err(CourseError::NotFound(course.id.clone()));
    }
    let new: Vec<LessonPlan> = course.outline_only().lessons.into_iter().filter(|l| l.number >= from).collect();
    let n = new.len();
    if n > 0 {
        let body = ser(&LessonsOnly { lessons: new }, &course.id)?;
        let series = course.series.max(1);
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(&path)?;
        f.write_all(format!("\n# Series {series}, added {date}\n\n{body}").as_bytes())?;
    }
    save_progress(ws, course)?;
    Ok(n)
}

/// Read a course: its outline with its progress put over it.
pub fn load(ws: &Workspace, id: &str) -> Result<Course, CourseError> {
    let path = outline_path(ws, id)?;
    let text = std::fs::read_to_string(&path).map_err(|_| CourseError::NotFound(id.to_string()))?;
    let mut c = parse(&text, &ws.relative(&path))?;
    // The file name is the identity.
    c.id = id.to_string();
    for l in c.lessons.iter_mut() {
        l.series = l.series.max(1);
    }
    c.series = c.lessons.iter().map(|l| l.series).chain([c.series]).max().unwrap_or(1).max(1);
    let ppath = progress_path(ws, id)?;
    if let Ok(ptext) = std::fs::read_to_string(&ppath) {
        let p: Progress = toml::from_str(&ptext).map_err(|e| CourseError::Format(ws.relative(&ppath), e.to_string()))?;
        c.apply_progress(&p);
    }
    Ok(c)
}

/// One line of the course list.
#[derive(Debug, Clone, Serialize)]
pub struct CourseSummary {
    pub id: String,
    pub kind: CourseKind,
    pub name: String,
    pub lang: crate::lang::Lang,
    pub status: CourseStatus,
    pub created: String,
    pub lessons: usize,
    pub done: usize,
    pub next: Option<u32>,
}

/// Courses in the user's recipes folder, newest first. Files that are not
/// courses (plain recipes without an outline) or do not parse are listed in
/// the second value with the reason.
pub fn list(ws: &Workspace) -> (Vec<CourseSummary>, Vec<String>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for path in crate::workspace::files_with_ext(&recipes_dir(ws), &[EXT]) {
        let Some(id) = path.file_stem().map(|s| s.to_string_lossy().to_string()) else { continue };
        if path.parent() != Some(recipes_dir(ws).as_path()) || !valid_id(&id) {
            continue;
        }
        match load(ws, &id) {
            Ok(c) if !c.lessons.is_empty() => out.push(CourseSummary {
                id: c.id.clone(),
                kind: c.kind,
                name: c.name.clone(),
                lang: c.lang,
                status: c.status,
                created: c.created.clone(),
                lessons: c.lessons.len(),
                done: c.done_count(),
                next: c.next_lesson().map(|l| l.number),
            }),
            Ok(_) => skipped.push(format!("{id}：沒有課程大綱")),
            Err(e) => skipped.push(e.to_string()),
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then(a.id.cmp(&b.id)));
    (out, skipped)
}

/// Save a generated lesson's Markdown as `lessons/<lang>/<date>-<course-id>-<NN>.md`
/// under a free name; returns the workspace-relative path.
pub fn save_lesson_file(ws: &Workspace, course: &Course, number: u32, markdown: &str, date: &str) -> Result<String, CourseError> {
    let dir = ws.lessons_dir(course.lang);
    std::fs::create_dir_all(&dir)?;
    let course_part = course.id.strip_prefix(&format!("{}-", course.created)).unwrap_or(&course.id);
    let path = free_path(&dir, &format!("{date}-{course_part}-{number:02}"), "md");
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    f.write_all(markdown.as_bytes())?;
    Ok(ws.relative(&path))
}

/// What a passed unit is worth, for the points records (docs/plan.md §8):
/// the stage A points code turns this into a `records/points/` line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnitResult {
    pub lang: crate::lang::Lang,
    pub unit: UnitKind,
    pub skill: Skill,
    /// 1 when passed, 0 otherwise (bonuses are added by the points code).
    pub points: f32,
    pub score: Option<f32>,
    /// The lesson file (or course id) for the `detail` column.
    pub detail: String,
    /// The lesson became done with this unit.
    pub lesson_done: bool,
    /// The course became finished with this unit.
    pub course_finished: bool,
}

/// Record a unit attempt. A passed unit is marked done (once; doing it again
/// is worth no new point) and the lesson is done when all its units are; the
/// course is finished when every lesson is done. A failed attempt changes
/// nothing and is worth 0. The caller saves with [`save_progress`].
pub fn mark_unit(course: &mut Course, number: u32, unit: u32, passed: bool, score: Option<f32>, date: &str) -> Result<UnitResult, CourseError> {
    let lang = course.lang;
    let id = course.id.clone();
    let lp = course.lesson_mut(number).ok_or_else(|| CourseError::NotFound(format!("{id} 第 {number} 課")))?;
    let u = lp.units.get(unit as usize).cloned().ok_or_else(|| CourseError::NotFound(format!("{id} 第 {number} 課的第 {} 個單元", unit + 1)))?;
    let new = passed && !lp.units_done.contains(&unit);
    if new {
        lp.units_done.push(unit);
        lp.units_done.sort_unstable();
    }
    let was_done = lp.status == LessonStatus::Done;
    if lp.units_done.len() >= lp.units.len() && !was_done {
        lp.status = LessonStatus::Done;
        lp.done = Some(date.to_string());
    }
    let lesson_done = !was_done && lp.status == LessonStatus::Done;
    let detail = lp.file.clone().unwrap_or_else(|| id.clone());
    let all_done = course.lessons.iter().all(|l| l.status == LessonStatus::Done);
    let course_finished = all_done && course.status == CourseStatus::Active;
    if course_finished {
        course.status = CourseStatus::Finished;
    }
    Ok(UnitResult { lang, unit: u.kind, skill: u.skill, points: if new { 1.0 } else { 0.0 }, score, detail, lesson_done, course_finished })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::course::outline::{create_fixed_course, FixedRequest};
    use crate::lang::Lang;
    use crate::workspace::{Profile, Root};

    pub fn workspace() -> (tempfile::TempDir, Workspace) {
        let tpl = tempfile::tempdir().unwrap();
        std::fs::write(tpl.path().join("workspace.toml"), "id = \"\"\n").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut root = Root::init(dir.path(), tpl.path()).unwrap();
        let ws = root.create_profile(tpl.path(), Profile::new("", "ellrin")).unwrap();
        (dir, ws)
    }

    fn course() -> Course {
        let l = crate::course::outline::tests::learner(Lang::En);
        let words = (0..20).map(|i| TargetWord { term: format!("w{i}"), reading: "/w/".into(), meaning: "字".into() }).collect();
        let grammar = vec![TargetGrammar { pattern: "Not only ... but also ...".into(), meaning: "不僅…而且…".into() }];
        create_fixed_course(&l, &FixedRequest { name: "TOEIC 單字".into(), words, grammar, vocab_files: vec!["vocab/toeic-en.csv".into()], ..Default::default() }, "2026-11-05").unwrap()
    }

    #[test]
    fn round_trip_and_free_names() {
        let (_d, ws) = workspace();
        let mut c = course();
        let id = save_new(&ws, &mut c).unwrap();
        assert_eq!(id, "2026-11-05-toeic-單字");
        let outline = recipes_dir(&ws).join(format!("{id}.toml"));
        let text = std::fs::read_to_string(&outline).unwrap();
        assert!(text.contains("kind = \"fixed\"") && text.contains("[[lesson]]") && text.contains("[[lesson.unit]]"), "{text}");
        assert!(!text.contains("status") && !text.contains("units_done"), "no progress in the outline:\n{text}");
        assert!(recipes_dir(&ws).join(format!("{id}.progress.toml")).exists());
        assert_eq!(load(&ws, &id).unwrap(), c);
        // Same name again: a free name, the first file untouched.
        let mut c2 = course();
        assert_eq!(save_new(&ws, &mut c2).unwrap(), format!("{id}-2"));
        assert_eq!(std::fs::read_to_string(&outline).unwrap(), text);
        // A left-over progress file is not adopted by a new course.
        std::fs::write(recipes_dir(&ws).join("2026-11-05-toeic-單字-3.progress.toml"), "").unwrap();
        assert_eq!(save_new(&ws, &mut course()).unwrap(), format!("{id}-4"));
        // A plain recipe and a broken file are skipped, not fatal; progress files are not courses.
        std::fs::write(recipes_dir(&ws).join("interest.toml"), "kind = \"topic\"\nname = \"AI\"\nlang = \"en\"\nid = \"x\"\n").unwrap();
        std::fs::write(recipes_dir(&ws).join("broken.toml"), "kind = [").unwrap();
        let (list, skipped) = list(&ws);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].lessons, 3);
        assert_eq!(list[0].next, Some(1));
        assert_eq!(skipped.len(), 2, "{skipped:?}");
        assert!(load(&ws, "../profile").is_err());
        // Hand-written files with only some fields still load.
        std::fs::write(recipes_dir(&ws).join("mini.toml"), "kind = \"topic\"\nname = \"Mini\"\nlang = \"ja\"\nid = \"\"\n[[lesson]]\nnumber = 1\n").unwrap();
        let m = load(&ws, "mini").unwrap();
        assert_eq!((m.id.as_str(), m.daily_minutes, m.lessons[0].status, m.series), ("mini", 30, LessonStatus::Planned, 1));
    }

    #[test]
    fn hand_edits_survive_progress_updates() {
        let (_d, ws) = workspace();
        let mut c = course();
        let id = save_new(&ws, &mut c).unwrap();
        let outline = outline_path(&ws, &id).unwrap();
        // The learner edits the outline: a comment and a new title.
        let edited = std::fs::read_to_string(&outline)
            .unwrap()
            .replacen("title = \"第 1 課：w0、w1、w2\"", "# my own note\ntitle = \"Office words\"", 1);
        assert!(edited.contains("Office words"));
        std::fs::write(&outline, &edited).unwrap();

        let mut c = load(&ws, &id).unwrap();
        assert_eq!(c.lessons[0].title, "Office words");
        c.lesson_mut(1).unwrap().file = Some("profiles/ellrin/lessons/en/x.md".into());
        c.lesson_mut(1).unwrap().status = LessonStatus::Generated;
        mark_unit(&mut c, 1, 0, true, None, "2026-11-05").unwrap();
        save_progress(&ws, &c).unwrap();
        assert_eq!(std::fs::read_to_string(&outline).unwrap(), edited, "the outline is never rewritten");

        let back = load(&ws, &id).unwrap();
        assert_eq!(back.lessons[0].title, "Office words");
        assert_eq!(back.lessons[0].status, LessonStatus::Generated);
        assert_eq!(back.lessons[0].units_done, vec![0]);
        assert_eq!(back.lessons[0].file.as_deref(), Some("profiles/ellrin/lessons/en/x.md"));
        assert_eq!(back.lessons[1].status, LessonStatus::Planned);
        let p = std::fs::read_to_string(progress_path(&ws, &id).unwrap()).unwrap();
        assert!(p.contains("units_done = [0]") && !p.contains("w0"), "{p}");
    }

    #[test]
    fn older_outline_with_progress_fields_still_loads() {
        let (_d, ws) = workspace();
        std::fs::create_dir_all(recipes_dir(&ws)).unwrap();
        let old = "id = \"old\"\nkind = \"fixed\"\nname = \"Old\"\nlang = \"en\"\nstatus = \"active\"\n\n[[lesson]]\nnumber = 1\nstatus = \"generated\"\nfile = \"a.md\"\nunits_done = [0]\n\n[[lesson]]\nnumber = 2\n";
        std::fs::write(recipes_dir(&ws).join("old.toml"), old).unwrap();
        let mut c = load(&ws, "old").unwrap();
        assert_eq!((c.lessons[0].status, c.lessons[0].file.as_deref()), (LessonStatus::Generated, Some("a.md")));
        // From the first update on, progress lives in its own file; the old file stays as it is.
        c.lesson_mut(2).unwrap().status = LessonStatus::Generated;
        save_progress(&ws, &c).unwrap();
        assert_eq!(std::fs::read_to_string(recipes_dir(&ws).join("old.toml")).unwrap(), old);
        let back = load(&ws, "old").unwrap();
        assert_eq!(back.lessons[1].status, LessonStatus::Generated);
        assert_eq!(back.lessons[0].units_done, vec![0]);
    }

    #[test]
    fn appended_lessons_keep_the_rest() {
        let (_d, ws) = workspace();
        let mut c = course();
        let id = save_new(&ws, &mut c).unwrap();
        let outline = outline_path(&ws, &id).unwrap();
        let edited = format!("{}\n# a note at the end\n", std::fs::read_to_string(&outline).unwrap());
        std::fs::write(&outline, &edited).unwrap();
        let mut c = load(&ws, &id).unwrap();
        let mut extra = c.lessons[0].clone();
        extra.number = 4;
        extra.series = 2;
        extra.title = "Series two".into();
        c.lessons.push(extra);
        c.series = 2;
        assert_eq!(append_lessons(&ws, &c, 4, "2026-11-12").unwrap(), 1);
        let text = std::fs::read_to_string(&outline).unwrap();
        assert!(text.starts_with(&edited), "earlier text untouched");
        assert!(text.contains("# Series 2, added 2026-11-12"));
        let back = load(&ws, &id).unwrap();
        assert_eq!(back.lessons.len(), 4);
        assert_eq!((back.lessons[3].title.as_str(), back.lessons[3].series, back.series), ("Series two", 2, 2));
    }

    #[test]
    fn units_lessons_and_course_finish() {
        let (_d, ws) = workspace();
        let mut c = course();
        save_new(&ws, &mut c).unwrap();
        let r = mark_unit(&mut c, 1, 1, false, Some(0.4), "2026-11-05").unwrap();
        assert_eq!(r.points, 0.0);
        let r = mark_unit(&mut c, 1, 1, true, Some(0.8), "2026-11-05").unwrap();
        assert_eq!((r.points, r.unit, r.skill), (1.0, UnitKind::Reading, Skill::Reading));
        assert_eq!(mark_unit(&mut c, 1, 1, true, None, "2026-11-05").unwrap().points, 0.0, "no second point");
        assert!(mark_unit(&mut c, 1, 9, true, None, "2026-11-05").is_err());
        for u in [0, 2] {
            assert!(!mark_unit(&mut c, 1, u, true, None, "2026-11-05").unwrap().lesson_done);
        }
        let r = mark_unit(&mut c, 1, 3, true, None, "2026-11-05").unwrap();
        assert!(r.lesson_done && !r.course_finished);
        assert_eq!(c.lessons[0].status, LessonStatus::Done);
        let mut last = None;
        for n in 2..=c.lessons.len() as u32 {
            for u in 0..c.lesson(n).unwrap().units.len() as u32 {
                last = Some(mark_unit(&mut c, n, u, true, None, "2026-11-06").unwrap());
            }
        }
        assert!(last.unwrap().course_finished);
        assert_eq!(c.status, CourseStatus::Finished);
        save_progress(&ws, &c).unwrap();
        assert_eq!(load(&ws, &c.id).unwrap().status, CourseStatus::Finished);
    }
}
