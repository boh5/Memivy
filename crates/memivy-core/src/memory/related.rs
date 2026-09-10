//! On-demand local related memories; no model calls or persistent recommendation state.
use super::{db::valid_id, records, retrieval, *};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct RelatedMemory {
    pub memory_id: String,
    pub version_id: String,
    pub title: String,
    pub snippet: String,
    pub source: SourceRef,
}

impl MemoryStore {
    pub fn related_memories(
        &self,
        memory_id: &str,
        expected_version: &str,
    ) -> Result<Vec<RelatedMemory>> {
        self.related_memories_in_collection(memory_id, expected_version, None)
    }
    pub fn related_memories_in_collection(
        &self,
        memory_id: &str,
        expected_version: &str,
        collection: Option<&str>,
    ) -> Result<Vec<RelatedMemory>> {
        valid_id(memory_id)?;
        valid_id(expected_version)?;
        let mut db = self.connection()?;
        retrieval::budget(&db)?;
        let tx = db.transaction()?;
        records::head(&tx, memory_id, expected_version)?;
        if let Some(collection) = collection
            && !super::navigation::source_in_collection(
                &tx,
                collection,
                &SourceRef::Version(expected_version.into()),
            )?
        {
            return Err(DataError::Unavailable);
        }
        tx.commit()?;
        let result = self.search(&SearchRequest {
            reference_memory_id: Some(memory_id.into()),
            scope: SearchScope {
                collection_id: collection.map(str::to_owned),
                ..Default::default()
            },
            limit: 3,
            excerpt_chars: 160,
            ..Default::default()
        })?;
        records::head(&self.connection()?, memory_id, expected_version)?;
        Ok(result
            .items
            .into_iter()
            .map(|r| RelatedMemory {
                memory_id: r.memory_id,
                version_id: r.version_id,
                title: r.title,
                snippet: r.evidence.text,
                source: r.evidence.source,
            })
            .collect())
    }
}
