//! Version-derived index lifecycle. The app owns one writer; all readers share it.
use super::{db::*, *};
use crate::embedding::{self as emb, Preferences, cache::HfModelCache, chunk, client};
use crate::models::{Registry, Source};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::time::Duration;
#[derive(Serialize)]
pub struct EmbeddingStatus {
    pub enabled: bool,
    pub preparing: bool,
    pub paused: bool,
    pub state: String,
    pub downloaded: u64,
    pub bytes: u64,
    pub processed: i64,
    pub total: i64,
    pub failed: i64,
    pub error: Option<String>,
}
#[derive(Clone)]
pub(super) struct IndexMeta {
    pub fingerprint: String,
    pub revision: String,
    pub state: String,
    pub upper: i64,
    pub cursor: i64,
}
pub(super) fn meta(db: &rusqlite::Connection) -> Result<Option<IndexMeta>> {
    Ok(db.query_row("SELECT fingerprint,revision,state,upper_rowid,cursor FROM embedding_index_meta WHERE id=1",[],|r|Ok(IndexMeta {fingerprint:r.get(0)?,revision:r.get(1)?,state:r.get(2)?,upper:r.get(3)?,cursor:r.get(4)?})).optional()?)
}
fn model_error(_: String) -> DataError {
    DataError::Io
}
impl MemoryStore {
    pub fn embedding_status(&self) -> Result<EmbeddingStatus> {
        let prefs = Preferences::read(&self.root).map_err(model_error)?;
        let cache = HfModelCache::for_user().map_err(model_error)?;
        let registry = Registry::read(&self.root).map_err(model_error)?;
        let remote = registry.embedding.source == Source::Service;
        let fingerprint = registry.fingerprint().map_err(model_error)?;
        let db = self.connection()?;
        let index = meta(&db)?;
        let total: i64 = db.query_row(
            "SELECT count(*) FROM memories WHERE state='active'",
            [],
            |r| r.get(0),
        )?;
        let (processed,failed):(i64,i64)=db.query_row("SELECT count(CASE WHEN e.error IS NULL THEN 1 END),count(CASE WHEN e.error IS NOT NULL THEN 1 END) FROM embedding_records e JOIN memories m ON m.id=e.memory_id AND m.current_version_id=e.version_id WHERE m.state='active'",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        let state = if prefs.clear_requested {
            "clearing"
        } else if prefs.paused {
            "paused"
        } else if prefs.error.is_some() {
            "failed"
        } else if !prefs.wanted() {
            if remote || cache.published() {
                "disabled"
            } else {
                "not_downloaded"
            }
        } else if !remote && !cache.published() {
            "downloading"
        } else if prefs.enabled
            && !prefs.preparing
            && index
                .as_ref()
                .is_some_and(|i| i.state == "ready" && i.fingerprint == fingerprint)
        {
            "ready"
        } else if !remote && !client::ready(&self.root) {
            "warming"
        } else {
            "indexing"
        };
        Ok(EmbeddingStatus {
            enabled: prefs.enabled,
            preparing: prefs.preparing,
            paused: prefs.paused,
            state: state.into(),
            downloaded: cache.downloaded(),
            bytes: emb::MODEL.bytes,
            processed,
            total,
            failed,
            error: prefs.error,
        })
    }
    /// Publish a tested binding together with its activation state. Readers never
    /// observe the candidate while activation can still fail.
    pub fn apply_embedding_model(
        &self,
        registry: &mut Registry,
        revision: &str,
    ) -> std::result::Result<(), String> {
        let _control = emb::lock(&self.root, "embedding-control.lock")?;
        let mut prefs = Preferences::read(&self.root)?;
        if prefs.clear_requested {
            return Err("正在清理本地模型，请稍后重试".into());
        }
        let mut previous = Registry::read(&self.root)?;
        registry.save(&self.root, revision)?;
        prefs.preparing = true;
        prefs.paused = false;
        prefs.error = None;
        if let Err(error) = prefs.save(&self.root) {
            return Err(if previous.save(&self.root, &registry.revision).is_ok() {
                error
            } else {
                format!("{error}；配置恢复未完成，请重新读取设置后检查")
            });
        }
        Ok(())
    }
    pub fn embedding_control(&self, action: &str) -> Result<()> {
        let _lock = emb::lock(&self.root, "embedding-control.lock").map_err(model_error)?;
        let mut prefs = Preferences::read(&self.root).map_err(model_error)?;
        if prefs.clear_requested && action != "clear" {
            return Err(DataError::Busy);
        }
        match action {
            "enable" | "resume" => {
                prefs.preparing = true;
                prefs.paused = false;
                prefs.error = None;
            }
            "pause" => {
                prefs.paused = true;
            }
            "disable" | "cancel" => {
                prefs.enabled = false;
                prefs.preparing = false;
                prefs.paused = false;
                prefs.error = None;
            }
            "retry" => {
                let cache = HfModelCache::for_user().map_err(model_error)?;
                if Registry::read(&self.root)
                    .map_err(model_error)?
                    .embedding
                    .source
                    == Source::Local
                    && cache.published()
                    && cache.verify().is_err()
                {
                    cache.clear().map_err(model_error)?;
                }
                prefs.preparing = true;
                prefs.paused = false;
                prefs.error = None;
                let mut db = self.connection()?;
                let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
                tx.execute("DELETE FROM embedding_records WHERE error IS NOT NULL", [])?;
                tx.execute(
                    "UPDATE embedding_index_meta SET revision=?1,cursor=0,state='building',upper_rowid=(SELECT COALESCE(max(rowid),0) FROM memories)",
                    [id()],
                )?;
                tx.commit()?;
            }
            "rebuild" => {
                prefs.enabled = false;
                prefs.preparing = true;
                prefs.paused = false;
                prefs.error = None;
                self.reset_embedding_index()?;
            }
            "clear" => {
                prefs.enabled = false;
                prefs.preparing = false;
                prefs.paused = false;
                prefs.error = None;
                prefs.clear_requested = true;
            }
            _ => return Err(DataError::Invalid),
        }
        prefs.save(&self.root).map_err(model_error)
    }
    pub(super) fn reset_embedding_index(&self) -> Result<()> {
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        reset_with(
            &tx,
            &Registry::read(&self.root)
                .map_err(model_error)?
                .fingerprint()
                .map_err(model_error)?,
        )?;
        tx.commit()?;
        Ok(())
    }
    /// One finite unit per host tick; closing Settings has no effect on the task.
    pub async fn embedding_tick(&self) {
        let Ok(_writer) = emb::lock(&self.root, "embedding-writer.lock") else {
            return;
        };
        let Ok(control) = emb::lock(&self.root, "embedding-control.lock") else {
            return;
        };
        let Ok(prefs) = Preferences::read(&self.root) else {
            return;
        };
        let Ok(registry) = Registry::read(&self.root) else {
            return;
        };
        drop(control);
        if prefs.clear_requested {
            match HfModelCache::for_user()
                .and_then(|cache| cache.clear())
                .map_err(model_error)
                .and_then(|_| self.reset_embedding_index())
            {
                Ok(()) => {
                    if let Ok(_control) = emb::lock(&self.root, "embedding-control.lock") {
                        let mut p = prefs;
                        p.clear_requested = false;
                        let _ = p.save(&self.root);
                    }
                }
                Err(e) => {
                    if let Ok(_control) = emb::lock(&self.root, "embedding-control.lock") {
                        let mut p = prefs;
                        p.clear_requested = false;
                        p.error = Some(e.to_string());
                        let _ = p.save(&self.root);
                    }
                }
            }
            return;
        }
        if !prefs.wanted() || prefs.error.is_some() {
            return;
        }
        let Ok(fingerprint) = registry.fingerprint() else {
            return;
        };
        // A ready, unchanged index must not wake an idle model every heartbeat.
        if prefs.enabled && !prefs.preparing {
            let unchanged = (|| -> Result<bool> {
                let db = self.connection()?;
                if meta(&db)?.is_none_or(|m| m.state != "ready" || m.fingerprint != fingerprint) {
                    return Ok(false);
                }
                Ok(!db.query_row("SELECT EXISTS(SELECT 1 FROM memories m LEFT JOIN embedding_records e ON e.memory_id=m.id WHERE m.state='active' AND (e.version_id IS NULL OR e.version_id!=m.current_version_id))",[],|r|r.get::<_,bool>(0))?)
            })();
            if unchanged == Ok(true) {
                return;
            }
        }
        if registry.embedding.source == Source::Local {
            let cache = match HfModelCache::for_user() {
                Ok(cache) => cache,
                Err(error) => {
                    self.embedding_failure_for(&fingerprint, error);
                    return;
                }
            };
            if !cache.published()
                && let Err(e) = cache
                    .download(|| {
                        Preferences::read(&self.root).is_ok_and(|p| p.wanted())
                            && self.embedding_matches(&fingerprint)
                    })
                    .await
            {
                self.embedding_failure_for(&fingerprint, e);
                return;
            }
            if !client::ready(&self.root) {
                if let Err(e) = client::warmup(&self.root) {
                    self.embedding_failure_for(&fingerprint, e);
                    return;
                }
                let began = std::time::Instant::now();
                while !client::ready(&self.root) {
                    if let Some(error) = client::startup_error(&self.root) {
                        self.embedding_failure_for(&fingerprint, error);
                        return;
                    }
                    if !Preferences::read(&self.root).is_ok_and(|p| p.wanted())
                        || !self.embedding_matches(&fingerprint)
                    {
                        return;
                    }
                    if began.elapsed() > Duration::from_secs(60) {
                        self.embedding_failure_for(
                            &fingerprint,
                            "模型加载失败或超过 60 秒，请重试".into(),
                        );
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
        let store = self.clone();
        match tokio::task::spawn_blocking(move || store.embedding_step()).await {
            Ok(Ok(())) => {}
            Ok(Err(DataError::Busy)) => {}
            Ok(Err(e)) => self.embedding_failure_for(&fingerprint, e.to_string()),
            Err(_) => self.embedding_failure_for(&fingerprint, "索引任务未完成，可重试".into()),
        }
    }
    fn embedding_matches(&self, fingerprint: &str) -> bool {
        Registry::read(&self.root)
            .and_then(|r| r.fingerprint())
            .is_ok_and(|f| f == fingerprint)
    }
    fn embedding_failure_for(&self, fingerprint: &str, error: String) {
        if self.embedding_matches(fingerprint) {
            self.embedding_failure(error);
        }
    }
    fn embedding_failure(&self, error: String) {
        let Ok(_guard) = emb::lock(&self.root, "embedding-control.lock") else {
            return;
        };
        if let Ok(mut prefs) = Preferences::read(&self.root)
            && prefs.wanted()
        {
            prefs.error = Some(error);
            let _ = prefs.save(&self.root);
        }
    }
    fn embedding_step(&self) -> Result<()> {
        let control =
            emb::lock(&self.root, "embedding-control.lock").map_err(|_| DataError::Busy)?;
        let r = Registry::read(&self.root).map_err(model_error)?;
        drop(control);
        let fp = r.fingerprint().map_err(model_error)?;
        let remote = if r.embedding.source == Source::Service {
            Some(r.resolve(&r.embedding).map_err(model_error)?)
        } else {
            None
        };
        self.embedding_step_for(&fp, |text| {
            if let Some(m) = &remote {
                crate::models::embed(m, text, r.embedding.dimensions, Duration::from_secs(30))
                    .map(|v| crate::models::bytes(&v))
            } else {
                client::encode(&self.root, "document", text, Duration::from_secs(30))
                    .and_then(|v| emb::vector_bytes(&v))
            }
        })
    }
    #[cfg(test)]
    fn embedding_step_with(&self, encode: impl FnMut(&str) -> emb::Result<Vec<u8>>) -> Result<()> {
        let fp = Registry::read(&self.root)
            .map_err(model_error)?
            .fingerprint()
            .map_err(model_error)?;
        self.embedding_step_for(&fp, encode)
    }
    fn embedding_step_for(
        &self,
        fingerprint: &str,
        mut encode: impl FnMut(&str) -> emb::Result<Vec<u8>>,
    ) -> Result<()> {
        // Restore cannot replace the database beneath this unit. No SQLite
        // transaction remains open during model encoding.
        let _restore = super::access::root_lock(&self.root, false)?;
        super::access::available(&self.root)?;
        if !Preferences::read(&self.root).map_err(model_error)?.wanted() {
            return Ok(());
        }
        let mut db = self.connection()?;
        if Registry::read(&self.root)
            .map_err(model_error)?
            .fingerprint()
            .map_err(model_error)?
            != fingerprint
        {
            return Ok(());
        }
        if meta(&db)?.is_none_or(|i| i.fingerprint != fingerprint) {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            reset_with(&tx, fingerprint)?;
            tx.commit()?;
        }
        let index = meta(&db)?.ok_or(DataError::Integrity)?;
        let next: Option<(i64, String)> = if index.state == "building" {
            db.query_row("SELECT rowid,id FROM memories WHERE state='active' AND rowid>?1 AND rowid<=?2 ORDER BY rowid LIMIT 1",params![index.cursor,index.upper],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
        } else {
            db.query_row("SELECT m.rowid,m.id FROM memories m LEFT JOIN embedding_records e ON e.memory_id=m.id WHERE m.state='active' AND (e.version_id IS NULL OR e.version_id!=m.current_version_id) ORDER BY m.rowid LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
        };
        let Some((rowid, memory)) = next else {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE embedding_index_meta SET cursor=upper_rowid WHERE id=1 AND revision=?",
                [&index.revision],
            )?;
            let failed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM embedding_records e JOIN memories m ON m.id=e.memory_id AND m.current_version_id=e.version_id WHERE m.state='active' AND e.error IS NOT NULL)",[],|r|r.get(0))?;
            if !failed {
                tx.execute(
                    "UPDATE embedding_index_meta SET state='ready' WHERE id=1 AND revision=?",
                    [&index.revision],
                )?;
            }
            tx.commit()?;
            if failed && index.state == "building" {
                self.embedding_failure_for(
                    fingerprint,
                    "部分记忆编码失败，请重试；字面检索仍然可用".into(),
                );
            }
            if !failed {
                let _control =
                    emb::lock(&self.root, "embedding-control.lock").map_err(model_error)?;
                let mut p = Preferences::read(&self.root).map_err(model_error)?;
                if p.wanted() && self.embedding_matches(fingerprint) {
                    p.enabled = true;
                    p.preparing = false;
                    p.save(&self.root).map_err(model_error)?;
                }
            }
            return Ok(());
        };
        let version = match self.memory(&memory) {
            Ok(m) => m.current,
            Err(DataError::Unavailable) => {
                if index.state == "building" {
                    db.execute(
                        "UPDATE embedding_index_meta SET cursor=?1 WHERE revision=?2",
                        params![rowid, index.revision],
                    )?;
                }
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        let chunks = chunk::split(&version.title, &version.body);
        let mut vectors = Vec::with_capacity(chunks.len());
        let mut error = None;
        for c in &chunks {
            if !Preferences::read(&self.root).map_err(model_error)?.wanted()
                || Registry::read(&self.root)
                    .map_err(model_error)?
                    .fingerprint()
                    .map_err(model_error)?
                    != fingerprint
            {
                return Ok(());
            }
            let reused:Option<Vec<u8>>=db.query_row("SELECT vector FROM embedding_chunks WHERE memory_id=?1 AND input_hash=?2 LIMIT 1",params![memory,c.hash],|r|r.get(0)).optional()?;
            if let Some(v) = reused {
                vectors.push(v);
                continue;
            }
            match encode(&c.text) {
                Ok(v) => vectors.push(v),
                Err(e) => {
                    error = Some(e);
                    break;
                }
            }
        }
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if meta(&tx)?.is_none_or(|m| m.revision != index.revision)
            || Registry::read(&self.root)
                .map_err(model_error)?
                .fingerprint()
                .map_err(model_error)?
                != fingerprint
        {
            return Ok(());
        }
        if !Preferences::read(&self.root).map_err(model_error)?.wanted() {
            return Ok(());
        }
        let current:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM memories WHERE id=?1 AND current_version_id=?2 AND state='active')",params![memory,version.id],|r|r.get(0))?;
        if current {
            tx.execute("DELETE FROM embedding_chunks WHERE memory_id=?", [&memory])?;
            if error.is_none() {
                for (n, (c, v)) in chunks.iter().zip(vectors).enumerate() {
                    tx.execute(
                        "INSERT INTO embedding_chunks VALUES(?1,?2,?3,?4,?5,?6,?7)",
                        params![
                            memory,
                            version.id,
                            n as i64,
                            c.start as i64,
                            c.end as i64,
                            c.hash,
                            v
                        ],
                    )?;
                }
            }
            let input = emb::hash(
                chunks
                    .iter()
                    .flat_map(|c| c.hash.bytes())
                    .collect::<Vec<_>>()
                    .as_slice(),
            );
            tx.execute("INSERT INTO embedding_records VALUES(?1,?2,?3,?4) ON CONFLICT(memory_id) DO UPDATE SET version_id=excluded.version_id,input_hash=excluded.input_hash,error=excluded.error",params![memory,version.id,input,error])?;
        }
        if index.cursor < index.upper {
            tx.execute(
                "UPDATE embedding_index_meta SET cursor=?1 WHERE revision=?2",
                params![rowid, index.revision],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(super) fn query_vector(
        &self,
        request: &SearchRequest,
    ) -> std::result::Result<Option<(String, Vec<u8>)>, String> {
        let control = emb::lock(&self.root, "embedding-control.lock")?;
        let prefs = Preferences::read(&self.root)?;
        if !prefs.enabled || prefs.paused {
            return Ok(None);
        }
        if let Some(error) = prefs.error {
            return Err(error);
        }
        let registry = Registry::read(&self.root)?;
        drop(control);
        let remote = registry.embedding.source == Source::Service;
        let fingerprint = registry.fingerprint()?;
        if !remote && !HfModelCache::for_user()?.published() {
            return Err("本地模型文件缺失，请在设置中重试".into());
        }

        let db = self.connection().map_err(|e| e.to_string())?;
        let Some(index) = meta(&db).map_err(|e| e.to_string())? else {
            return Err("向量索引尚未建立".into());
        };
        if index.state != "ready" || index.fingerprint != fingerprint {
            return Err("向量索引正在重建".into());
        }
        let text = if let Some(m) = &request.reference_memory_id {
            let v = self.memory(m).map_err(|e| e.to_string())?.current;
            format!(
                "{}\n{}",
                v.title.chars().take(150).collect::<String>(),
                v.body.chars().take(650).collect::<String>()
            )
        } else {
            request.query.clone()
        };
        let vector = if remote {
            let input = format!("{}{}", registry.embedding.query_prefix, text);
            crate::models::bytes(&crate::models::embed(
                &registry.resolve(&registry.embedding)?,
                &input,
                registry.embedding.dimensions,
                Duration::from_secs(3),
            )?)
        } else {
            emb::vector_bytes(&client::encode(
                &self.root,
                "query",
                &chunk::query(&text)?,
                Duration::from_secs(2),
            )?)?
        };
        if !Preferences::read(&self.root)?.enabled
            || Registry::read(&self.root)?.fingerprint()? != fingerprint
        {
            return Err("语义检索配置已变化，本次使用基础搜索".into());
        }
        Ok(Some((index.revision, vector)))
    }
}
pub(super) fn reset(db: &rusqlite::Connection) -> Result<()> {
    reset_with(db, &emb::fingerprint())
}
fn reset_with(db: &rusqlite::Connection, fingerprint: &str) -> Result<()> {
    db.execute_batch("DELETE FROM embedding_chunks; DELETE FROM embedding_records;")?;
    db.execute("INSERT INTO embedding_index_meta VALUES(1,?1,?2,'building',(SELECT COALESCE(max(rowid),0) FROM memories),0) ON CONFLICT(id) DO UPDATE SET fingerprint=excluded.fingerprint,revision=excluded.revision,state='building',upper_rowid=excluded.upper_rowid,cursor=0",params![fingerprint,id()])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn setup() -> (tempfile::TempDir, MemoryStore) {
        let d = tempfile::tempdir().unwrap();
        let s = MemoryStore::open(d.path()).unwrap();
        Preferences {
            preparing: true,
            ..Default::default()
        }
        .save(d.path())
        .unwrap();
        (d, s)
    }
    fn capture(s: &MemoryStore, text: &str) -> CaptureResult {
        s.capture(&CaptureRequest {
            request_id: id(),
            text: text.into(),
            origin: Origin::User {
                app: "QA".into(),
                project: None,
                uri: None,
            },
        })
        .unwrap()
    }
    fn vector() -> Vec<u8> {
        let mut v = vec![0.0; emb::DIMENSIONS];
        v[0] = 1.0;
        emb::vector_bytes(&v).unwrap()
    }
    #[test]
    fn model_switch_rejects_inflight_vectors_and_supports_new_dimensions() {
        use crate::models::{Binding, Connection, Registry, Source};
        let (d, s) = setup();
        capture(&s, "保留原文，切换模型测试");
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        let mut r = Registry::default();
        r.connections.push(Connection {
            id: "api".into(),
            name: "Test".into(),
            base_url: "http://localhost:9/v1".into(),
            api_key: None,
        });
        r.embedding = Binding {
            source: Source::Service,
            connection: "api".into(),
            model: "three".into(),
            dimensions: Some(3),
            ..Binding::default()
        };
        r.save(d.path(), "initial").unwrap();
        s.embedding_step_with(|_| Ok(crate::models::bytes(&[1., 0., 0.])))
            .unwrap();
        let db = s.connection().unwrap();
        assert_eq!(
            db.query_row("SELECT length(vector) FROM embedding_chunks", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            12
        );
        let rev = r.revision.clone();
        r.embedding.model = "four".into();
        r.embedding.dimensions = Some(4);
        r.save(d.path(), &rev).unwrap();
        s.embedding_step_with(|_| {
            let rev = r.revision.clone();
            r.embedding.model = "five".into();
            r.embedding.dimensions = Some(5);
            r.save(d.path(), &rev).unwrap();
            Ok(crate::models::bytes(&[1., 0., 0., 0.]))
        })
        .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM embedding_chunks", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        s.embedding_step_with(|_| Ok(crate::models::bytes(&[1., 0., 0., 0., 0.])))
            .unwrap();
        assert_eq!(
            db.query_row("SELECT length(vector) FROM embedding_chunks", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            20
        );
    }
    #[tokio::test]
    async fn ready_index_stays_idle_without_a_worker_or_download() {
        let (_d, s) = setup();
        s.embedding_step_with(|_| unreachable!()).unwrap();
        s.embedding_tick().await;
        let prefs = Preferences::read(&s.root).unwrap();
        assert!(prefs.enabled && !prefs.preparing && prefs.error.is_none());
        assert!(!s.root.join("models").exists());
        assert!(!client::ready(&s.root));
    }
    #[test]
    fn retry_invalidates_old_cursor_and_incremental_failures_stay_local() {
        let (_d, s) = setup();
        capture(&s, "first failed row");
        capture(&s, "second row");
        s.embedding_step_with(|_| Err("synthetic".into())).unwrap();
        s.embedding_step_with(|_| {
            s.embedding_control("retry").unwrap();
            Ok(vector())
        })
        .unwrap();
        assert_eq!(meta(&s.connection().unwrap()).unwrap().unwrap().cursor, 0);
        assert_eq!(s.embedding_status().unwrap().processed, 0);
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        s.embedding_step_with(|_| unreachable!()).unwrap();
        let failed = capture(&s, "new failing row");
        s.embedding_step_with(|_| Err("synthetic".into())).unwrap();
        s.embedding_step_with(|_| unreachable!()).unwrap();
        let prefs = Preferences::read(&s.root).unwrap();
        assert!(prefs.enabled && prefs.error.is_none());
        assert_eq!(s.embedding_status().unwrap().failed, 1);
        s.edit_memory(&EditRequest {
            request_id: id(),
            memory_id: failed.memory_id,
            expected_version: failed.version_id,
            title: "fixed".into(),
            body: "valid edited content".into(),
        })
        .unwrap();
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        assert_eq!(s.embedding_status().unwrap().failed, 0);
        assert_eq!(s.embedding_status().unwrap().processed, 3);
    }
    #[test]
    fn edits_discard_inflight_vectors_and_changes_are_incremental() {
        let (_d, s) = setup();
        let m = capture(&s, "旧内容");
        s.embedding_step_with(|_| {
            s.edit_memory(&EditRequest {
                request_id: id(),
                memory_id: m.memory_id.clone(),
                expected_version: m.version_id.clone(),
                title: "新内容".into(),
                body: "changed current body".into(),
            })
            .unwrap();
            Ok(vector())
        })
        .unwrap();
        assert_eq!(
            s.connection()
                .unwrap()
                .query_row("SELECT count(*) FROM embedding_chunks", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        s.embedding_step_with(|_| panic!("finite first scan already visited this row"))
            .unwrap();
        assert_eq!(
            meta(&s.connection().unwrap()).unwrap().unwrap().state,
            "ready"
        );
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        assert_eq!(s.embedding_status().unwrap().processed, 1);
        let v = s.memory(&m.memory_id).unwrap().current;
        s.edit_memory(&EditRequest {
            request_id: id(),
            memory_id: m.memory_id.clone(),
            expected_version: v.id,
            title: "新内容".into(),
            body: "newer current body".into(),
        })
        .unwrap();
        let db = s.connection().unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM embedding_chunks c JOIN memories m ON m.id=c.memory_id AND m.current_version_id=c.version_id WHERE m.state='active'",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    }
    #[test]
    fn new_captures_do_not_extend_initial_scan_and_failed_units_do_not_claim_readiness() {
        let (_d, s) = setup();
        capture(&s, "first");
        s.embedding_step_with(|_| {
            capture(&s, "new while encoding");
            Ok(vector())
        })
        .unwrap();
        s.embedding_step_with(|_| panic!("new rows must not extend checkpoint"))
            .unwrap();
        assert_eq!(
            meta(&s.connection().unwrap()).unwrap().unwrap().state,
            "ready"
        );
        s.embedding_step_with(|_| Err("synthetic failure".into()))
            .unwrap();
        assert_eq!(s.embedding_status().unwrap().failed, 1);
        s.embedding_control("retry").unwrap();
        s.embedding_step_with(|_| panic!("same input reuses vector"))
            .unwrap();
        s.embedding_step_with(|_| Err("synthetic failure".into()))
            .unwrap();
        s.embedding_step_with(|_| unreachable!()).unwrap();
        assert_eq!(
            meta(&s.connection().unwrap()).unwrap().unwrap().state,
            "building"
        );
        assert!(Preferences::read(&s.root).unwrap().error.is_some());
    }
    #[test]
    fn cancellation_and_rebuild_invalidate_inflight_units() {
        let (_d, s) = setup();
        capture(&s, "first");
        s.embedding_step_with(|_| {
            s.embedding_control("disable").unwrap();
            Ok(vector())
        })
        .unwrap();
        assert_eq!(s.embedding_status().unwrap().processed, 0);
        s.embedding_control("enable").unwrap();
        s.embedding_step_with(|_| {
            s.embedding_control("rebuild").unwrap();
            Ok(vector())
        })
        .unwrap();
        assert_eq!(s.embedding_status().unwrap().processed, 0);
        s.embedding_step_with(|_| Ok(vector())).unwrap();
        let before = meta(&s.connection().unwrap()).unwrap().unwrap().revision;
        let out = tempfile::tempdir().unwrap();
        let backup = out.path().join("backup.db");
        s.backup(&backup).unwrap();
        let restored = MemoryStore::restore_backup(&backup, out.path().join("restored")).unwrap();
        assert_ne!(
            meta(&restored.connection().unwrap())
                .unwrap()
                .unwrap()
                .revision,
            before
        );
        assert_eq!(restored.embedding_status().unwrap().processed, 0);
    }
}
