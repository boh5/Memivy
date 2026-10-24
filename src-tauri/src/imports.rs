//! Native file selection keeps paths in the host; MemoryStore owns all writes.
use crate::errors::HostError;
use crate::workspace::{HostResult, Workspace, blocking, require_main};
use memivy_core::memory::{
    IMPORT_MAX_FILE_BYTES as MAX_FILE_BYTES, IMPORT_MAX_FILES as MAX_FILES,
    IMPORT_MAX_TOTAL_BYTES as MAX_TOTAL_BYTES, ImportFile, ImportRequest, ImportResult,
};
use serde::Serialize;
use std::{collections::HashSet, fs, io::Read, path::PathBuf};
use tauri::Emitter;

#[derive(Clone, Serialize)]
pub(crate) struct FilePreview {
    id: String,
    file_name: String,
    bytes: usize,
    error: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct ImportPreview {
    id: String,
    files: Vec<FilePreview>,
}

pub(crate) struct PendingImport {
    id: String,
    files: Vec<(FilePreview, Option<ImportFile>)>,
    selection: Option<Vec<String>>,
    applying: bool,
    preparing: bool,
}

impl PendingImport {
    fn request(&mut self, file_ids: Vec<String>) -> HostResult<ImportRequest> {
        if self.applying || self.preparing {
            return Err(HostError::new("import_busy"));
        }
        let selected: HashSet<_> = file_ids.iter().collect();
        if selected.is_empty() || selected.len() != file_ids.len() {
            return Err(HostError::new("invalid"));
        }
        let mut ids = Vec::new();
        let mut files = Vec::new();
        for (preview, file) in &self.files {
            if selected.contains(&preview.id) {
                files.push(file.clone().ok_or_else(|| HostError::new("invalid"))?);
                ids.push(preview.id.clone());
            }
        }
        if ids.len() != selected.len() {
            return Err(HostError::new("invalid"));
        }
        if self
            .selection
            .as_ref()
            .is_some_and(|previous| *previous != ids)
        {
            return Err(HostError::new("request_conflict"));
        }
        self.selection = Some(ids);
        self.applying = true;
        Ok(ImportRequest {
            request_id: self.id.clone(),
            files,
        })
    }
}

fn reserve_import(pending: &mut Option<PendingImport>, id: &str) -> HostResult<()> {
    if pending
        .as_ref()
        .is_some_and(|value| value.applying || value.preparing)
    {
        return Err(HostError::new("import_busy"));
    }
    // A reloaded webview may lose its token. Replacing an idle preview is safe:
    // it has no writes, and completed requests are idempotent in MemoryStore.
    *pending = Some(PendingImport {
        id: id.to_owned(),
        files: Vec::new(),
        selection: None,
        applying: false,
        preparing: true,
    });
    Ok(())
}

fn read_files(paths: Vec<PathBuf>) -> HostResult<Vec<(FilePreview, Option<ImportFile>)>> {
    if paths.len() > MAX_FILES {
        return Err(HostError::new("import_file_limit"));
    }
    let mut total = 0;
    Ok(paths
        .into_iter()
        .map(|path| {
            let file_name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut preview = FilePreview {
                id: uuid::Uuid::new_v4().to_string(),
                file_name: file_name.clone(),
                bytes: 0,
                error: None,
            };
            let loaded = (|| {
                if !path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("txt")
                }) {
                    return Err("import_file_type");
                }
                if file_name.is_empty()
                    || file_name.len() > 255
                    || file_name.contains(['/', '\\'])
                    || file_name.chars().any(char::is_control)
                    || path
                        .file_stem()
                        .is_none_or(|stem| stem.to_string_lossy().trim().is_empty())
                {
                    return Err("import_file_name");
                }
                let metadata = fs::metadata(&path).map_err(|_| "import_file_read")?;
                if !metadata.is_file() {
                    return Err("import_file_read");
                }
                preview.bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                if preview.bytes > MAX_FILE_BYTES {
                    return Err("import_file_size");
                }
                let mut bytes = Vec::new();
                fs::File::open(&path)
                    .map_err(|_| "import_file_read")?
                    .take((MAX_FILE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|_| "import_file_read")?;
                preview.bytes = bytes.len();
                if bytes.len() > MAX_FILE_BYTES {
                    return Err("import_file_size");
                }
                if total + bytes.len() > MAX_TOTAL_BYTES {
                    return Err("import_total_size");
                }
                let text = String::from_utf8(bytes).map_err(|_| "import_file_encoding")?;
                if text
                    .chars()
                    .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\n' | '\r' | '\t')))
                {
                    return Err("import_file_binary");
                }
                if text.trim_start_matches('\u{feff}').trim().is_empty() {
                    return Err("import_file_empty");
                }
                total += text.len();
                Ok(ImportFile { file_name, text })
            })();
            match loaded {
                Ok(file) => (preview, Some(file)),
                Err(error) => {
                    preview.error = Some(error);
                    (preview, None)
                }
            }
        })
        .collect())
}

async fn select_files(app: &tauri::AppHandle) -> HostResult<Option<Vec<PathBuf>>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
        use objc2_foundation::{NSArray, NSString};
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(false);
        panel.setCanChooseFiles(true);
        panel.setAllowsMultipleSelection(true);
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&[
            NSString::from_str("md"),
            NSString::from_str("txt"),
        ])));
        panel.setMessage(Some(&NSString::from_str(&crate::i18n::text(
            &handle,
            "importOpen",
        ))));
        let paths = if panel.runModal() == NSModalResponseOK {
            Some(
                panel
                    .URLs()
                    .iter()
                    .filter_map(|url| url.path())
                    .map(|path| PathBuf::from(path.to_string()))
                    .collect(),
            )
        } else {
            None
        };
        let _ = tx.send(paths);
    })
    .map_err(|_| HostError::new("file_dialog_failed"))?;
    rx.await.map_err(|_| HostError::new("file_dialog_failed"))
}

#[tauri::command]
pub(crate) async fn import_prepare(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
) -> HostResult<Option<ImportPreview>> {
    let _work = crate::updates::work()?;
    require_main(&window)?;
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut pending = state.import_request.lock().unwrap();
        reserve_import(&mut pending, &id)?;
    }
    let loaded = match select_files(&app).await {
        Ok(Some(paths)) => crate::updates::spawn_blocking(move || read_files(paths))
            .await
            .map_err(|_| HostError::new("operation_failed"))
            .and_then(|result| result)
            .map(Some),
        Ok(None) => Ok(None),
        Err(error) => Err(error),
    };
    let mut pending = state.import_request.lock().unwrap();
    match loaded {
        Ok(Some(files)) => {
            let preview = ImportPreview {
                id: id.clone(),
                files: files.iter().map(|(preview, _)| preview.clone()).collect(),
            };
            *pending = Some(PendingImport {
                id,
                files,
                selection: None,
                applying: false,
                preparing: false,
            });
            Ok(Some(preview))
        }
        Ok(None) => {
            *pending = None;
            Ok(None)
        }
        Err(error) => {
            *pending = None;
            Err(error)
        }
    }
}

#[tauri::command]
pub(crate) async fn import_apply(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
    file_ids: Vec<String>,
) -> HostResult<ImportResult> {
    let _work = crate::updates::work()?;
    require_main(&window)?;
    let request = state
        .import_request
        .lock()
        .unwrap()
        .as_mut()
        .filter(|pending| pending.id == id)
        .ok_or_else(|| HostError::new("unavailable"))?
        .request(file_ids)?;
    let store = state.store.clone();
    let result = blocking(move || store.import_files(&request)).await;
    if let Some(pending) = state
        .import_request
        .lock()
        .unwrap()
        .as_mut()
        .filter(|pending| pending.id == id)
    {
        pending.applying = false;
    }
    if result.is_ok() {
        let _ = app.emit("resources-changed", ());
    }
    result
}

#[tauri::command]
pub(crate) fn import_discard(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, Workspace>,
    id: String,
) -> HostResult<()> {
    let _work = crate::updates::work()?;
    require_main(&window)?;
    let mut pending = state.import_request.lock().unwrap();
    if pending.as_ref().is_some_and(|pending| pending.id == id) {
        if pending.as_ref().is_some_and(|pending| pending.applying) {
            return Err(HostError::new("import_busy"));
        }
        *pending = None;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_preserves_original_bytes_and_reports_bad_files_without_paths() {
        let dir = tempfile::tempdir().unwrap();
        let original = "\u{feff}# 中文资料\r\nOriginal words — preserved.\r\n";
        let inputs: &[(&str, &[u8])] = &[
            ("reference.MD", original.as_bytes()),
            ("empty.txt", b" \n\t"),
            ("encoding.txt", &[0xff, 0xfe]),
            ("binary.txt", b"prefix\0suffix"),
            ("image.png", b"image"),
            ("large.txt", &vec![b'a'; MAX_FILE_BYTES + 1]),
        ];
        let paths = inputs
            .iter()
            .map(|(name, contents)| {
                let path = dir.path().join(name);
                fs::write(&path, contents).unwrap();
                path
            })
            .collect();
        let files = read_files(paths).unwrap();
        assert_eq!(files[0].1.as_ref().unwrap().text, original);
        let errors: Vec<_> = files.iter().map(|(preview, _)| preview.error).collect();
        assert_eq!(
            errors,
            vec![
                None,
                Some("import_file_empty"),
                Some("import_file_encoding"),
                Some("import_file_binary"),
                Some("import_file_type"),
                Some("import_file_size")
            ]
        );
        let preview = ImportPreview {
            id: uuid::Uuid::new_v4().to_string(),
            files: files.iter().map(|(preview, _)| preview.clone()).collect(),
        };
        let json = serde_json::to_string(&preview).unwrap();
        assert!(!json.contains(dir.path().to_str().unwrap()));
        assert!(!json.contains("Original words"));
    }

    #[test]
    fn preview_limits_count_total_bytes_and_non_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_files(vec![dir.path().join("missing.txt"); MAX_FILES + 1])
                .err()
                .unwrap()
                .code,
            "import_file_limit"
        );
        let path = dir.path().join("full.txt");
        fs::write(&path, vec![b'a'; MAX_FILE_BYTES]).unwrap();
        let files = read_files(vec![path; 17]).unwrap();
        assert!(files[..16].iter().all(|(_, file)| file.is_some()));
        assert_eq!(files[16].0.error, Some("import_total_size"));
        let directory = dir.path().join("directory.txt");
        fs::create_dir(&directory).unwrap();
        assert_eq!(
            read_files(vec![directory]).unwrap()[0].0.error,
            Some("import_file_read")
        );
    }

    #[test]
    fn apply_uses_selected_snapshot_and_freezes_retry_identity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "Selected original").unwrap();
        let files = read_files(vec![path.clone(), path.clone()]).unwrap();
        let first = files[0].0.id.clone();
        let second = files[1].0.id.clone();
        let id = uuid::Uuid::new_v4().to_string();
        let mut pending = PendingImport {
            id: id.clone(),
            files,
            selection: None,
            applying: false,
            preparing: false,
        };
        fs::write(&path, "Later file edit").unwrap();
        assert_eq!(
            pending.request(vec!["unknown".into()]).err().unwrap().code,
            "invalid"
        );
        assert_eq!(
            pending
                .request(vec![first.clone(), first.clone()])
                .err()
                .unwrap()
                .code,
            "invalid"
        );
        let request = pending.request(vec![first.clone()]).unwrap();
        assert_eq!(request.request_id, id);
        assert_eq!(request.files[0].text, "Selected original");
        assert_eq!(
            pending.request(vec![first.clone()]).err().unwrap().code,
            "import_busy"
        );
        pending.applying = false;
        assert_eq!(
            pending.request(vec![second]).err().unwrap().code,
            "request_conflict"
        );
        assert_eq!(
            pending.request(vec![first]).unwrap().files[0].text,
            "Selected original"
        );
    }

    #[test]
    fn reloaded_view_can_replace_idle_preview_but_never_interrupts_work() {
        let mut pending = None;
        reserve_import(&mut pending, "first").unwrap();
        assert_eq!(
            reserve_import(&mut pending, "second").unwrap_err().code,
            "import_busy"
        );
        let first = pending.as_mut().unwrap();
        first.preparing = false;
        first.applying = true;
        assert_eq!(
            reserve_import(&mut pending, "second").unwrap_err().code,
            "import_busy"
        );
        pending.as_mut().unwrap().applying = false;
        reserve_import(&mut pending, "second").unwrap();
        assert_eq!(pending.unwrap().id, "second");
    }
}
