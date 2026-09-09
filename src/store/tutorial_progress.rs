//! Tutorial history is UI state, independent of layout restoration and user settings.
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS tutorial_progress (
    topic_id TEXT NOT NULL, revision INTEGER NOT NULL,
    completed INTEGER NOT NULL DEFAULT 0 CHECK(completed IN (0,1)),
    resume_step TEXT, row_version INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(topic_id, revision)
);";

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Record {
    pub completed: bool,
    pub resume_step: Option<String>,
    pub version: i64,
}

pub(crate) fn load(conn: &Connection, topic: &str, revision: u32) -> rusqlite::Result<Record> {
    conn.query_row("SELECT completed, resume_step, row_version FROM tutorial_progress WHERE topic_id=?1 AND revision=?2",
        params![topic,revision], |r| Ok(Record { completed:r.get(0)?,resume_step:r.get(1)?,version:r.get(2)? }))
        .optional().map(Option::unwrap_or_default)
}

/// Completion is monotonic; stale resume writes never overwrite another view.
pub(crate) fn save(
    conn: &mut Connection,
    topic: &str,
    revision: u32,
    step: &str,
    completed: bool,
    expected: i64,
) -> rusqlite::Result<(Record, bool)> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO tutorial_progress(topic_id, revision) VALUES(?1,?2)",
        params![topic, revision],
    )?;
    let updated = tx.execute("UPDATE tutorial_progress SET resume_step=?3, row_version=row_version+1 WHERE topic_id=?1 AND revision=?2 AND row_version=?4",
        params![topic,revision,step,expected])?;
    if completed {
        tx.execute(
            "UPDATE tutorial_progress SET completed=1 WHERE topic_id=?1 AND revision=?2",
            params![topic, revision],
        )?;
    }
    let result = load(&tx, topic, revision)?;
    tx.commit()?;
    Ok((result, updated == 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stale_resume_cannot_undo_completion_or_position() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch(SCHEMA).unwrap();
        let a = save(&mut db, "basics", 1, "pane", false, 0).unwrap().0;
        let done = save(&mut db, "basics", 1, "workspace", true, a.version)
            .unwrap()
            .0;
        let stale = save(&mut db, "basics", 1, "surface", false, a.version)
            .unwrap()
            .0;
        assert_eq!(stale, done);
        assert!(stale.completed);
        assert_eq!(load(&db, "basics", 2).unwrap(), Record::default());
    }
    #[test]
    fn additive_schema_preserves_existing_ui_state() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE recent_files(path TEXT); INSERT INTO recent_files VALUES('keep'); PRAGMA user_version=1;").unwrap();
        db.execute_batch(SCHEMA).unwrap();
        db.execute_batch(SCHEMA).unwrap();
        assert_eq!(
            db.query_row("SELECT path FROM recent_files", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
    }
}
