use crate::{DataPaths, Error, Result};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const SCHEMA: i64 = 2;
const MAX_TEXT_BYTES: usize = 128 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaptureInput {
    pub request_id: String,
    pub text: String,
    pub source_app: String,
    pub project: Option<String>,
    pub session_uri: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub id: String,
    pub request_id: String,
    pub text: String,
    pub source_app: String,
    pub project: Option<String>,
    pub session_uri: Option<String>,
    pub created_at: i64,
    pub ai_state: String,
}

#[derive(Serialize, Debug)]
pub struct SearchPage {
    pub items: Vec<Capture>,
    pub elapsed_ms: f64,
    pub strategy: &'static str,
}

#[derive(Serialize, Debug)]
pub struct Diagnostics {
    pub sqlite_version: String,
    pub fts5: bool,
    pub journal_mode: String,
    pub synchronous: i64,
    pub schema_version: i64,
    pub count: i64,
    pub database_path: String,
    pub mcp_enabled: bool,
}

#[derive(Clone, Debug)]
pub struct Store {
    pub paths: DataPaths,
}

fn connect(paths: &DataPaths, create: bool) -> Result<Connection> {
    let path = paths.database();
    if path.exists() && fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(Error::Invalid("数据库不能是符号链接"));
    }
    let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    if create {
        flags |= OpenFlags::SQLITE_OPEN_CREATE;
    }
    let db = Connection::open_with_flags(path, flags)?;
    db.busy_timeout(Duration::from_millis(750))?;
    db.pragma_update(None, "foreign_keys", true)?;
    db.pragma_update(None, "synchronous", "FULL")?;
    Ok(db)
}

pub(crate) fn read_capture(row: &rusqlite::Row<'_>) -> rusqlite::Result<Capture> {
    Ok(Capture {
        id: row.get(0)?,
        request_id: row.get(1)?,
        text: row.get(2)?,
        source_app: row.get(3)?,
        project: row.get(4)?,
        session_uri: row.get(5)?,
        created_at: row.get(6)?,
        ai_state: row.get(7)?,
    })
}
pub(crate) const COLUMNS: &str =
    "c.id,c.request_id,c.text,c.source_app,c.project,c.session_uri,c.created_at,c.ai_state";

impl Store {
    pub fn open(paths: DataPaths) -> Result<Self> {
        let mut db = connect(&paths, true)?;
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA {
            return Err(Error::NewerSchema);
        }
        let journal: String = db.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        if journal != "wal" {
            db.pragma_update(None, "journal_mode", "WAL")?;
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA {
            return Err(Error::NewerSchema);
        }
        if version == 0 {
            tx.execute_batch(include_str!("../../../migrations/001_phase1.sql"))?;
        }
        if version < 2 {
            tx.execute_batch(include_str!("../../../migrations/002_interaction.sql"))?;
        }
        tx.commit()?;
        fs::set_permissions(paths.database(), fs::Permissions::from_mode(0o600))?;
        Ok(Self { paths })
    }

    pub(crate) fn connection(&self) -> Result<Connection> {
        let db = connect(&self.paths, false)?;
        let v: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if v != SCHEMA {
            return Err(Error::NewerSchema);
        }
        Ok(db)
    }

    pub fn capture(&self, input: CaptureInput) -> Result<Capture> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let capture = Self::insert_capture(&tx, &input)?;
        tx.commit()?;
        Ok(capture)
    }

    pub(crate) fn insert_capture(
        tx: &rusqlite::Transaction<'_>,
        input: &CaptureInput,
    ) -> Result<Capture> {
        if Uuid::parse_str(&input.request_id).is_err() {
            return Err(Error::Invalid("请求标识必须为 UUID"));
        }
        if input.text.trim().is_empty() || input.text.len() > MAX_TEXT_BYTES {
            return Err(Error::Invalid("请输入原话，最多 128 KB"));
        }
        if input.source_app.trim().is_empty() || input.source_app.len() > 200 {
            return Err(Error::Invalid("来源应用名称无效"));
        }
        if input.project.as_ref().is_some_and(|s| s.len() > 200)
            || input.session_uri.as_ref().is_some_and(|s| s.len() > 2048)
        {
            return Err(Error::Invalid("来源信息过长"));
        }
        let id = Uuid::new_v4().to_string();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Invalid("系统时间无效"))?
            .as_millis() as i64;
        tx.execute("INSERT INTO captures(id,request_id,text,source_app,project,session_uri,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(request_id) DO NOTHING", params![id,input.request_id,input.text,input.source_app,input.project,input.session_uri,now])?;
        let capture = tx.query_row(
            &format!("SELECT {COLUMNS} FROM captures c WHERE request_id=?1"),
            [&input.request_id],
            read_capture,
        )?;
        if capture.text != input.text
            || capture.source_app != input.source_app
            || capture.project != input.project
            || capture.session_uri != input.session_uri
        {
            return Err(Error::RequestConflict);
        }
        Ok(capture)
    }

    pub fn mcp_capture(&self, input: CaptureInput) -> Result<Capture> {
        if !self.paths.mcp_enabled() {
            return Err(Error::McpDisabled);
        }
        self.capture(input)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<SearchPage> {
        if query.len() > 512 {
            return Err(Error::Invalid("搜索词最多 512 字节"));
        }
        let terms: Vec<_> = query.split_whitespace().collect();
        if terms.len() > 16 {
            return Err(Error::Invalid("一次最多搜索 16 个关键词"));
        }
        let start = Instant::now();
        let db = self.connection()?;
        let mut predicates = vec![
            "NOT EXISTS (SELECT 1 FROM prototype_withdrawn w WHERE w.capture_id=c.id)".to_owned(),
        ];
        let mut values: Vec<String> = Vec::new();
        let mut long = Vec::new();
        for term in &terms {
            if term.chars().count() >= 3 {
                long.push(format!("\"{}\"", term.replace('"', "\"\"")));
            } else {
                // Literal substring semantics, including %, _ and quotes. Search all rows,
                // not just recent rows; the response limit does not bound scan work.
                predicates.push("(instr(lower(c.text),lower(?)) > 0 OR instr(lower(c.source_app),lower(?)) > 0)".to_owned());
                values.extend([term.to_string(), term.to_string()]);
            }
        }
        let has_long = !long.is_empty();
        let has_short = !values.is_empty();
        if has_long {
            predicates.push(
                "c.rowid IN (SELECT rowid FROM captures_fts WHERE captures_fts MATCH ?)".into(),
            );
            values.push(long.join(" AND "));
        }
        let filter = if predicates.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", predicates.join(" AND "))
        };
        let sql = format!(
            "SELECT {COLUMNS} FROM captures c{filter} ORDER BY c.created_at DESC,c.rowid DESC LIMIT {}",
            limit.clamp(1, 50)
        );
        let items = db
            .prepare(&sql)?
            .query_map(params_from_iter(values), read_capture)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SearchPage {
            items,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            strategy: match (has_long, has_short) {
                (true, true) => "trigram+literal",
                (true, false) => "trigram",
                (false, true) => "literal",
                _ => "recent",
            },
        })
    }

    pub fn diagnostics(&self) -> Result<Diagnostics> {
        let db = self.connection()?;
        Ok(Diagnostics {
            sqlite_version: db.query_row("SELECT sqlite_version()", [], |r| r.get(0))?,
            fts5: db.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |r| {
                r.get(0)
            })?,
            journal_mode: db.pragma_query_value(None, "journal_mode", |r| r.get(0))?,
            synchronous: db.pragma_query_value(None, "synchronous", |r| r.get(0))?,
            schema_version: db.pragma_query_value(None, "user_version", |r| r.get(0))?,
            count: db.query_row("SELECT count(*) FROM captures", [], |r| r.get(0))?,
            database_path: self.paths.database().display().to_string(),
            mcp_enabled: self.paths.mcp_enabled(),
        })
    }
}
