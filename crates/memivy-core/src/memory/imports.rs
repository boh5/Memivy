//! Atomic, user-requested file import. Originals are saved before any AI work.
use super::{db::*, records::*, *};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const IMPORT_MAX_FILES: usize = 32;
pub const IMPORT_MAX_FILE_BYTES: usize = 128 * 1024;
pub const IMPORT_MAX_TOTAL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportFile {
    pub file_name: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportRequest {
    pub request_id: String,
    pub files: Vec<ImportFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportStatus {
    Imported,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportItem {
    pub file_name: String,
    pub status: ImportStatus,
    pub memory_id: String,
    pub version_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportResult {
    pub request_id: String,
    pub items: Vec<ImportItem>,
    pub receipt: Option<Receipt>,
}

// Keep idempotency outcomes free of original filenames or text. Empty Trash
// must not leave another recoverable copy of erased source metadata here.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredImportItem {
    status: ImportStatus,
    memory_id: String,
    version_id: String,
}

pub(super) fn validate_file_name(file_name: &str) -> Result<()> {
    valid_text(file_name, 255)?;
    if file_name.contains(['/', '\\']) || file_name.chars().any(char::is_control) {
        return Err(DataError::Invalid);
    }
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return Err(DataError::Invalid);
    };
    if stem.trim().is_empty()
        || !(extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt"))
    {
        return Err(DataError::Invalid);
    }
    Ok(())
}

fn validate_request(request: &ImportRequest) -> Result<()> {
    valid_id(&request.request_id)?;
    if request.files.is_empty() || request.files.len() > IMPORT_MAX_FILES {
        return Err(DataError::Invalid);
    }
    let mut bytes = 0usize;
    for file in &request.files {
        validate_file_name(&file.file_name)?;
        if file.text.len() > IMPORT_MAX_FILE_BYTES
            || file.text.trim_start_matches('\u{feff}').trim().is_empty()
            || file
                .text
                .chars()
                .any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
        {
            return Err(DataError::Invalid);
        }
        bytes += file.text.len();
    }
    if bytes > IMPORT_MAX_TOTAL_BYTES {
        return Err(DataError::Invalid);
    }
    Ok(())
}

fn title(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem)
        .trim();
    let mut title = String::new();
    for character in stem.chars() {
        if title.len() + character.len_utf8() > 200 {
            break;
        }
        title.push(character);
    }
    title
}

fn duplicate(db: &Connection, text: &str, digest: &[u8]) -> Result<Option<(String, String)>> {
    // Any active owning memory preserves the imported original as evidence,
    // including after ordinary edits or an Agent merge. Hidden owners do not.
    Ok(db.query_row(
        "SELECT m.id,m.current_version_id FROM import_sources i JOIN captures c ON c.id=i.capture_id JOIN capture_state s ON s.capture_id=c.id JOIN version_captures vc ON vc.capture_id=c.id JOIN memory_versions v ON v.id=vc.version_id JOIN memories m ON m.id=v.memory_id WHERE i.content_hash=?1 AND c.text=?2 AND json_extract(c.source,'$.kind')='imported' AND s.availability='active' AND m.state='active' ORDER BY m.created_at,m.id LIMIT 1",
        params![digest,text], |row| Ok((row.get(0)?,row.get(1)?)),
    ).optional()?)
}

impl MemoryStore {
    /// One transaction saves originals, initial memories, deduplication records
    /// and a grouped undo receipt. No model or organization work is scheduled.
    pub fn import_files(&self, request: &ImportRequest) -> Result<ImportResult> {
        validate_request(request)?;
        let hash = fingerprint(&("import", &request.files))?;
        let mut db = self.connection()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(receipt) = replay(&tx, &request.request_id, &hash)? {
            let encoded: String = tx.query_row(
                "SELECT result FROM import_batches WHERE request_id=?",
                [&request.request_id],
                |row| row.get(0),
            )?;
            let stored: Vec<StoredImportItem> =
                serde_json::from_str(&encoded).map_err(|_| DataError::Integrity)?;
            if stored.len() != request.files.len() {
                return Err(DataError::Integrity);
            }
            let has_receipt = stored
                .iter()
                .any(|item| item.status == ImportStatus::Imported);
            let items = stored
                .into_iter()
                .zip(&request.files)
                .map(|(item, file)| ImportItem {
                    file_name: file.file_name.clone(),
                    status: item.status,
                    memory_id: item.memory_id,
                    version_id: item.version_id,
                })
                .collect();
            // The original outcome remains historical; replay never restores
            // imported memories that were later edited, undone, trashed or purged.
            return Ok(ImportResult {
                request_id: request.request_id.clone(),
                items,
                receipt: has_receipt.then_some(receipt),
            });
        }
        let mut items = Vec::with_capacity(request.files.len());
        let mut created = vec![];
        for file in &request.files {
            let digest = Sha256::digest(file.text.as_bytes());
            if let Some((memory_id, version_id)) = duplicate(&tx, &file.text, &digest)? {
                items.push(ImportItem {
                    file_name: file.file_name.clone(),
                    status: ImportStatus::Duplicate,
                    memory_id,
                    version_id,
                });
                continue;
            }
            let raw = insert_capture(
                &tx,
                &id(),
                &file.text,
                &Origin::Imported {
                    file_name: file.file_name.clone(),
                },
            )?;
            let saved = create_captured_memory_with_title(&tx, &raw, &title(&file.file_name))?;
            tx.execute(
                "INSERT INTO import_sources(capture_id,content_hash) VALUES(?1,?2)",
                params![saved.capture_id, digest.as_slice()],
            )?;
            items.push(ImportItem {
                file_name: file.file_name.clone(),
                status: ImportStatus::Imported,
                memory_id: saved.memory_id.clone(),
                version_id: saved.version_id.clone(),
            });
            created.push(saved);
        }
        let main = created.first();
        let receipt = Receipt {
            request_id: request.request_id.clone(),
            action: "import".into(),
            capture_id: main.map(|memory| memory.capture_id.clone()),
            memory_id: main.map(|memory| memory.memory_id.clone()),
            before_version: None,
            after_version: main.map(|memory| memory.version_id.clone()),
            status: "applied".into(),
            reason: None,
            collection_changes: vec![],
        };
        // A duplicate-only batch still reserves its request ID against reuse by
        // another operation, but offers no empty undo action to the caller.
        save_receipt(&tx, &receipt, &hash)?;
        let changes: Vec<_> = created
            .iter()
            .skip(1)
            .map(|memory| ReceiptChange {
                memory_id: memory.memory_id.clone(),
                before_version: None,
                after_version: memory.version_id.clone(),
                before_state: "undone".into(),
                after_state: "active".into(),
                navigation_before: None,
                navigation_after: None,
            })
            .collect();
        save_changes(&tx, &request.request_id, &changes)?;
        let result = ImportResult {
            request_id: request.request_id.clone(),
            items,
            receipt: main.map(|_| receipt),
        };
        let stored: Vec<_> = result
            .items
            .iter()
            .map(|item| StoredImportItem {
                status: item.status,
                memory_id: item.memory_id.clone(),
                version_id: item.version_id.clone(),
            })
            .collect();
        tx.execute(
            "INSERT INTO import_batches(request_id,result) VALUES(?1,?2)",
            params![request.request_id, encode(&stored)?],
        )?;
        tx.commit()?;
        Ok(result)
    }
}
