//! Cross-process, transactionally committed UI invalidation metadata.
use super::*;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeCursor {
    pub epoch: String,
    pub sequence: i64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ResourceChange {
    pub domain: String,
    pub entity: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct LibraryChanges {
    pub cursor: ChangeCursor,
    pub reset: bool,
    pub changes: Vec<ResourceChange>,
}
impl MemoryStore {
    pub fn library_changes(&self, cursor: Option<&ChangeCursor>) -> Result<LibraryChanges> {
        let mut db = self.connection()?;
        let tx = db.transaction()?;
        let epoch: String =
            tx.query_row("SELECT epoch FROM ui_change_epoch WHERE id=1", [], |r| {
                r.get(0)
            })?;
        let (first, last): (i64, i64) = tx.query_row(
            "SELECT COALESCE(MIN(seq),0),COALESCE(MAX(seq),0) FROM ui_changes",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let mut reset =
            cursor.is_none_or(|c| c.epoch != epoch || c.sequence < first - 1 || c.sequence > last);
        let changes = if reset {
            vec![]
        } else {
            tx.prepare("SELECT DISTINCT domain,entity FROM ui_changes WHERE seq>? ORDER BY domain,entity LIMIT 513")?
                .query_map([cursor.map_or(0, |c| c.sequence)], |r| Ok(ResourceChange { domain:r.get(0)?,entity:r.get(1)? }))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        reset |= changes.len() > 512;
        Ok(LibraryChanges {
            cursor: ChangeCursor {
                epoch,
                sequence: last,
            },
            reset,
            changes: if reset { vec![] } else { changes },
        })
    }
}

impl MemoryStore {
    /// Observe committed UI-visible changes across app and MCP connections.
    /// Drafts and derived indexes deliberately do not advance this revision.
    pub fn change_watcher(&self) -> Result<MemoryChangeWatcher> {
        let db = self.connection()?;
        let last = db.query_row(
            "SELECT revision FROM library_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        Ok(MemoryChangeWatcher { db, last })
    }
}
pub struct MemoryChangeWatcher {
    db: Connection,
    last: i64,
}
impl MemoryChangeWatcher {
    pub fn changed(&mut self) -> Result<bool> {
        let next = self.db.query_row(
            "SELECT revision FROM library_revision WHERE id=1",
            [],
            |r| r.get(0),
        )?;
        let changed = next != self.last;
        self.last = next;
        Ok(changed)
    }
}
