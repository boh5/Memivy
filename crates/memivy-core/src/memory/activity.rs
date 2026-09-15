use super::*;
use rusqlite::params;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ActivityDay {
    pub date: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct ActivitySummary {
    pub memory_count: i64,
    pub days: Vec<ActivityDay>,
}

#[derive(Debug, Serialize)]
pub struct ActivityRecord {
    pub id: String,
    #[serde(flatten)]
    pub row: LibraryRow,
}

#[derive(Debug, Serialize)]
pub struct ActivityRecords {
    pub items: Vec<ActivityRecord>,
    pub next_offset: Option<usize>,
}

impl MemoryStore {
    /// Count durable original inputs, never AI versions or split memories.
    /// Trashed and purged sources leave this view; restoring them restores activity.
    pub fn activity_summary(&self) -> Result<ActivitySummary> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let memory_count = tx.query_row(
            "SELECT count(*) FROM memories WHERE state='active'",
            [],
            |row| row.get(0),
        )?;
        // SQLite uses the Mac's historical local timezone rules, including DST.
        // Availability is authoritative, so this scan can use capture metadata
        // without reading original text. Only sparse daily counts cross IPC.
        let days = tx
            .prepare(
                "SELECT date(c.created_at / 1000, 'unixepoch', 'localtime') AS day, count(*)
             FROM captures c JOIN capture_state s ON s.capture_id=c.id
             WHERE s.availability='active'
             GROUP BY day ORDER BY day",
            )?
            .query_map([], |row| {
                Ok(ActivityDay {
                    date: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(ActivitySummary { memory_count, days })
    }

    /// Half-open local-day boundaries are computed by the UI's calendar, not by
    /// adding 24 hours; daylight-saving days can be shorter or longer.
    pub fn activity_records(
        &self,
        since: i64,
        until: i64,
        offset: usize,
    ) -> Result<ActivityRecords> {
        if since >= until || until.saturating_sub(since) > 27 * 60 * 60 * 1000 {
            return Err(DataError::Invalid);
        }
        let offset = i64::try_from(offset).map_err(|_| DataError::Invalid)?;
        let db = self.connection()?;
        let mut items = db
            .prepare(
                "SELECT c.id, substr(c.text,1,160), c.created_at, c.source,
                    (SELECT m.id FROM version_captures vc
                     JOIN memory_versions v ON v.id=vc.version_id
                     JOIN memories m ON m.id=v.memory_id AND m.state='active'
                     WHERE vc.capture_id=c.id
                     ORDER BY (m.current_version_id=vc.version_id) DESC,m.created_at,m.id LIMIT 1) AS memory_id
             FROM captures c JOIN capture_state s ON s.capture_id=c.id
             WHERE c.created_at>=?1 AND c.created_at<?2
               AND s.availability='active' AND c.text IS NOT NULL
             ORDER BY c.created_at DESC,c.id DESC LIMIT 41 OFFSET ?3",
            )?
            .query_map(params![since, until, offset], |row| {
                let capture_id: String = row.get(0)?;
                let text: String = row.get(1)?;
                let source: String = row.get(3)?;
                let memory_id: Option<String> = row.get(4)?;
                Ok(ActivityRecord {
                    id: capture_id.clone(),
                    row: LibraryRow {
                        key: RecordKey {
                            kind: if memory_id.is_some() {
                                "memory"
                            } else {
                                "capture"
                            }
                            .into(),
                            id: memory_id.unwrap_or(capture_id),
                        },
                        title: text
                            .lines()
                            .find(|line| !line.trim().is_empty())
                            .unwrap_or("")
                            .chars()
                            .take(80)
                            .collect(),
                        snippet: text,
                        updated_at: row.get(2)?,
                        origin: serde_json::from_str(&source).ok(),
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_offset = (items.len() > 40).then_some(offset as usize + 40);
        items.truncate(40);
        Ok(ActivityRecords { items, next_offset })
    }
}
