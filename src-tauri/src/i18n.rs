//! Presentation preferences belong to the library, but never to its content backup.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};
use tauri::{Emitter, Manager};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Preference {
    System,
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub preference: Preference,
    pub language: String,
    pub revision: u64,
    pub error: Option<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct LanguageError {
    code: &'static str,
}
struct Inner {
    snapshot: Snapshot,
    unreadable: bool,
}
pub struct Languages {
    path: PathBuf,
    inner: Mutex<Inner>,
}
#[derive(Serialize, Deserialize)]
struct Saved {
    preference: Preference,
}

pub fn resolve(preferred: &[String]) -> &'static str {
    for value in preferred {
        let lower = value.replace('_', "-").to_ascii_lowercase();
        let parts: Vec<_> = lower.split('-').collect();
        if parts[0] == "en" {
            return "en";
        }
        if parts[0] != "zh" || parts.contains(&"hant") {
            continue;
        }
        if parts.contains(&"hans") {
            return "zh-CN";
        }
        if parts.contains(&"tw") || parts.contains(&"hk") || parts.contains(&"mo") {
            continue;
        }
        if parts.len() == 1 || parts.contains(&"cn") || parts.contains(&"sg") {
            return "zh-CN";
        }
    }
    "en"
}
fn system_languages() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        objc2_foundation::NSLocale::preferredLanguages()
            .iter()
            .map(|s| s.to_string())
            .collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}
fn effective(preference: &Preference, system: &[String]) -> String {
    match preference {
        Preference::En => "en",
        Preference::ZhCn => "zh-CN",
        Preference::System => resolve(system),
    }
    .into()
}
impl Languages {
    fn new(path: PathBuf, system: &[String]) -> Self {
        let read = Self::read(&path);
        let unreadable = read.is_err();
        let preference = read.unwrap_or(Preference::System);
        let language = if unreadable {
            "en".into()
        } else {
            effective(&preference, system)
        };
        Self {
            path,
            inner: Mutex::new(Inner {
                snapshot: Snapshot {
                    preference,
                    language,
                    revision: 0,
                    error: unreadable.then_some("preferences_unavailable"),
                },
                unreadable,
            }),
        }
    }
    fn read(path: &std::path::Path) -> Result<Preference, ()> {
        match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<Saved>(&bytes)
                .map(|s| s.preference)
                .map_err(|_| ()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Preference::System),
            Err(_) => Err(()),
        }
    }
    fn set(&self, preference: Preference, system: &[String]) -> Result<(), LanguageError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.unreadable {
            return Err(LanguageError {
                code: "language_preferences_unavailable",
            });
        }
        let temp = self
            .path
            .with_file_name(format!(".ui-preferences-{}.tmp", uuid::Uuid::new_v4()));
        let save = || -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&serde_json::to_vec(&Saved {
                preference: preference.clone(),
            })?)?;
            file.sync_all()?;
            fs::rename(&temp, &self.path)
        };
        if save().is_err() {
            let _ = fs::remove_file(temp);
            return Err(LanguageError {
                code: "language_save_failed",
            });
        }
        inner.snapshot.language = effective(&preference, system);
        inner.snapshot.preference = preference;
        inner.snapshot.error = None;
        inner.snapshot.revision += 1;
        Ok(())
    }
    fn refresh(&self, system: &[String]) {
        let mut inner = self.inner.lock().unwrap();
        if inner.unreadable {
            let Ok(preference) = Self::read(&self.path) else {
                return;
            };
            inner.snapshot.preference = preference;
            inner.unreadable = false;
            inner.snapshot.error = None;
            inner.snapshot.revision += 1;
        }
        let language = effective(&inner.snapshot.preference, system);
        if inner.snapshot.language != language {
            inner.snapshot.language = language;
            inner.snapshot.revision += 1;
        }
    }
}
pub fn language(app: &tauri::AppHandle) -> String {
    app.state::<Languages>()
        .inner
        .lock()
        .unwrap()
        .snapshot
        .language
        .clone()
}
pub fn text(app: &tauri::AppHandle, key: &str) -> String {
    static EN: OnceLock<serde_json::Value> = OnceLock::new();
    static ZH: OnceLock<serde_json::Value> = OnceLock::new();
    let en = EN.get_or_init(|| {
        serde_json::from_str(include_str!("../../locales/en/native.json"))
            .expect("native English resources")
    });
    let zh = ZH.get_or_init(|| {
        serde_json::from_str(include_str!("../../locales/zh-CN/native.json"))
            .expect("native Chinese resources")
    });
    let selected = if language(app) == "zh-CN" { zh } else { en };
    selected
        .get(key)
        .or_else(|| en.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or(key)
        .into()
}
pub fn setup(app: &tauri::AppHandle) {
    let path = app
        .state::<crate::workspace::Workspace>()
        .store
        .database_path()
        .with_file_name("ui-preferences.json");
    app.manage(Languages::new(path, &system_languages()));
}
/// Call on the UI thread. Retrying a menu never aborts application startup.
pub fn refresh(app: &tauri::AppHandle) {
    let state = app.state::<Languages>();
    let before = state.inner.lock().unwrap().snapshot.clone();
    state.refresh(&system_languages());
    let retry = before.error == Some("native_menu_unavailable");
    let changed = before.revision != state.inner.lock().unwrap().snapshot.revision;
    if changed || retry {
        update_native(app);
    }
}
pub fn update_native(app: &tauri::AppHandle) {
    let failed = crate::desktop::update_menu(app).is_err();
    if let Some(window) = app.get_webview_window("capture") {
        let _ = window.set_title(&text(app, "captureWindow"));
    }
    let state = app.state::<Languages>();
    let mut inner = state.inner.lock().unwrap();
    let error = if inner.unreadable {
        Some("preferences_unavailable")
    } else if failed {
        Some("native_menu_unavailable")
    } else {
        None
    };
    if inner.snapshot.error != error {
        inner.snapshot.error = error;
        inner.snapshot.revision += 1;
    }
    let snapshot = inner.snapshot.clone();
    drop(inner);
    let _ = app.emit("ui-language-changed", snapshot);
}
#[tauri::command]
pub async fn ui_language_snapshot(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<Snapshot, LanguageError> {
    if !matches!(window.label(), "main" | "capture") {
        return Err(LanguageError { code: "forbidden" });
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        refresh(&handle);
        let value = handle
            .state::<Languages>()
            .inner
            .lock()
            .unwrap()
            .snapshot
            .clone();
        let _ = tx.send(value);
    })
    .map_err(|_| LanguageError {
        code: "native_menu_unavailable",
    })?;
    rx.await.map_err(|_| LanguageError {
        code: "native_menu_unavailable",
    })
}
#[tauri::command]
pub async fn ui_language_set(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    preference: String,
) -> Result<Snapshot, LanguageError> {
    if window.label() != "main" {
        return Err(LanguageError { code: "forbidden" });
    }
    let _update_work =
        crate::updates::work().map_err(|error| LanguageError { code: error.code })?;
    let preference = serde_json::from_value::<Preference>(serde_json::Value::String(preference))
        .map_err(|_| LanguageError {
            code: "invalid_language",
        })?;
    let handle = app.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        if let Err(error) = handle
            .state::<Languages>()
            .set(preference, &system_languages())
        {
            let _ = tx.send(Err(error));
            return;
        }
        update_native(&handle);
        let snapshot = handle
            .state::<Languages>()
            .inner
            .lock()
            .unwrap()
            .snapshot
            .clone();
        let _ = tx.send(Ok(snapshot));
    })
    .map_err(|_| LanguageError {
        code: "native_menu_unavailable",
    })?;
    rx.await.map_err(|_| LanguageError {
        code: "native_menu_unavailable",
    })?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matching() {
        #[derive(Deserialize)]
        struct Case {
            preferences: Vec<String>,
            language: String,
        }
        let cases: Vec<Case> =
            serde_json::from_str(include_str!("../../tests/fixtures/languages.json")).unwrap();
        for case in cases {
            assert_eq!(
                resolve(&case.preferences),
                case.language,
                "{:?}",
                case.preferences
            );
        }
    }
    #[test]
    fn system_refresh_changes_revision_only_when_effective_language_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-preferences.json");
        let state = Languages::new(path.clone(), &["en-US".into()]);
        state.refresh(&["en-GB".into()]);
        assert_eq!(state.inner.lock().unwrap().snapshot.revision, 0);
        state.refresh(&["zh-Hans".into()]);
        assert_eq!(state.inner.lock().unwrap().snapshot.revision, 1);
        assert!(
            !path.exists(),
            "system discovery must not write preferences"
        );
        state.set(Preference::En, &["zh".into()]).unwrap();
        state.refresh(&["zh".into()]);
        let snapshot = state.inner.lock().unwrap().snapshot.clone();
        assert_eq!(snapshot.language, "en");
        assert_eq!(snapshot.revision, 2);
    }
    #[test]
    fn native_resources_cover_the_same_nonempty_keys() {
        let en: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/en/native.json")).unwrap();
        let zh: serde_json::Value =
            serde_json::from_str(include_str!("../../locales/zh-CN/native.json")).unwrap();
        assert_eq!(
            en.as_object().unwrap().keys().collect::<Vec<_>>(),
            zh.as_object().unwrap().keys().collect::<Vec<_>>()
        );
        for object in [en, zh] {
            for value in object.as_object().unwrap().values() {
                assert!(!value.as_str().unwrap().trim().is_empty());
            }
        }
    }
    #[test]
    fn unreadable_is_not_overwritten_and_can_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-preferences.json");
        fs::write(&path, "broken").unwrap();
        let state = Languages::new(path.clone(), &["zh".into()]);
        assert_eq!(state.inner.lock().unwrap().snapshot.language, "en");
        assert!(state.set(Preference::En, &[]).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
        fs::write(path, r#"{"preference":"zh-CN"}"#).unwrap();
        state.refresh(&[]);
        assert_eq!(state.inner.lock().unwrap().snapshot.language, "zh-CN");
    }
    #[test]
    fn saves_atomically_and_failed_save_keeps_choice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-preferences.json");
        let state = Languages::new(path.clone(), &[]);
        state.set(Preference::ZhCn, &[]).unwrap();
        assert_eq!(Languages::read(&path).unwrap(), Preference::ZhCn);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(state.set(Preference::En, &[]).is_err());
        assert_eq!(
            state.inner.lock().unwrap().snapshot.preference,
            Preference::ZhCn
        );
    }
}
