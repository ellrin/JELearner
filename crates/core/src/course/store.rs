//! Course outlines as files in the user's workspace:
//! `profiles/<id>/recipes/<course-id>.toml` (TOML, like the recipes of
//! docs/plan.md §13, which a course file extends with its outline).
//!
//! New files always get a free name; an existing file is only rewritten by
//! [`save`] for the course it holds (status updates). The database can be
//! rebuilt from these files and the lesson files they point to.

use super::model::*;
use super::CourseError;
use crate::workspace::Workspace;
use crate::workspace::slug;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const EXT: &str = "toml";

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

/// Course ids are file stems: letters (also CJK), digits, `-`, `_`.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 80 && !id.starts_with(['-', '.']) && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

fn path_of(ws: &Workspace, id: &str) -> Result<PathBuf, CourseError> {
    if !valid_id(id) {
        return Err(CourseError::NotFound(id.to_string()));
    }
    Ok(recipes_dir(ws).join(format!("{id}.{EXT}")))
}

fn to_toml(course: &Course) -> Result<String, CourseError> {
    let body = toml::to_string_pretty(course).map_err(|e| CourseError::Format(course.id.clone(), e.to_string()))?;
    Ok(format!(
        "# JELearner course outline (docs/courses.md). The app updates `status`, `file`,\n\
         # `generated`, `done` and `units_done`; titles, words and grammar may be edited by hand.\n\n{body}"
    ))
}

pub fn parse(text: &str, origin: &str) -> Result<Course, CourseError> {
    toml::from_str(text).map_err(|e| CourseError::Format(origin.to_string(), e.to_string()))
}

/// Save a new course under a free name: `<date>-<slug of name>.toml`
/// (`-2`, `-3`… when taken). Sets and returns the course id.
pub fn save_new(ws: &Workspace, course: &mut Course) -> Result<String, CourseError> {
    let dir = recipes_dir(ws);
    std::fs::create_dir_all(&dir)?;
    let stem = format!("{}-{}", course.created, slug(&course.name, 40));
    let stem = if valid_id(&stem) { stem } else { format!("course-{}", course.created) };
    let path = free_path(&dir, &stem, EXT);
    course.id = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or(stem);
    // create_new: a file appearing between the check and the write is not overwritten.
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&path)?;
    f.write_all(to_toml(course)?.as_bytes())?;
    Ok(course.id.clone())
}

/// Rewrite the file of a saved course (after a status change).
pub fn save(ws: &Workspace, course: &Course) -> Result<(), CourseError> {
    let path = path_of(ws, &course.id)?;
    if !path.exists() {
        return Err(CourseError::NotFound(course.id.clone()));
    }
    std::fs::write(&path, to_toml(course)?)?;
    Ok(())
}

pub fn load(ws: &Workspace, id: &str) -> Result<Course, CourseError> {
    let path = path_of(ws, id)?;
    let text = std::fs::read_to_string(&path).map_err(|_| CourseError::NotFound(id.to_string()))?;
    let mut c = parse(&text, &ws.relative(&path))?;
    // The file name is the identity.
    c.id = id.to_string();
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
/// nothing and is worth 0. The caller saves the course.
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
        let text = std::fs::read_to_string(recipes_dir(&ws).join(format!("{id}.toml"))).unwrap();
        assert!(text.contains("kind = \"fixed\"") && text.contains("[[lesson]]") && text.contains("[[lesson.unit]]"), "{text}");
        assert_eq!(load(&ws, &id).unwrap(), c);
        // Same name again: a free name, the first file untouched.
        let mut c2 = course();
        assert_eq!(save_new(&ws, &mut c2).unwrap(), format!("{id}-2"));
        assert_eq!(std::fs::read_to_string(recipes_dir(&ws).join(format!("{id}.toml"))).unwrap(), text);
        // A plain recipe and a broken file are skipped, not fatal.
        std::fs::write(recipes_dir(&ws).join("interest.toml"), "kind = \"topic\"\nname = \"AI\"\nlang = \"en\"\nid = \"x\"\n").unwrap();
        std::fs::write(recipes_dir(&ws).join("broken.toml"), "kind = [").unwrap();
        let (list, skipped) = list(&ws);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].lessons, 3);
        assert_eq!(list[0].next, Some(1));
        assert_eq!(skipped.len(), 2);
        assert!(load(&ws, "../profile").is_err());
        // Hand-edited files with only some fields still load.
        std::fs::write(recipes_dir(&ws).join("mini.toml"), "kind = \"topic\"\nname = \"Mini\"\nlang = \"ja\"\nid = \"\"\n[[lesson]]\nnumber = 1\n").unwrap();
        let m = load(&ws, "mini").unwrap();
        assert_eq!((m.id.as_str(), m.daily_minutes, m.lessons[0].status), ("mini", 30, LessonStatus::Planned));
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
        save(&ws, &c).unwrap();
        assert_eq!(load(&ws, &c.id).unwrap().status, CourseStatus::Finished);
    }
}
