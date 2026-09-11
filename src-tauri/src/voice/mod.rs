mod audio;
mod engine;
use crate::workspace::HostResult;
use memivy_core::{
    embedding::{private_dir, write_json},
    speech::SpeechCache,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    enabled: bool,
    preload: bool,
    shortcut: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Part {
    file: String,
    text: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    key: String,
    prefix: String,
    suffix: String,
    base: String,
    parts: Vec<Part>,
    #[serde(default)]
    applied: bool,
    recording: bool,
    starting: bool,
    processing: bool,
    error: Option<String>,
    seconds: f32,
    level: f32,
}
impl Session {
    fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| p.text.as_deref())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn complete(&self) -> bool {
        !self.recording
            && !self.starting
            && !self.processing
            && self.error.is_none()
            && self.parts.iter().all(|p| p.text.is_some())
    }
}
#[derive(Serialize)]
pub struct SessionView {
    id: String,
    key: String,
    base: String,
    body: String,
    text: String,
    applied: bool,
    recording: bool,
    starting: bool,
    processing: bool,
    complete: bool,
    error: Option<String>,
    seconds: f32,
    level: f32,
}
impl From<&Session> for SessionView {
    fn from(s: &Session) -> Self {
        Self {
            id: s.id.clone(),
            key: s.key.clone(),
            base: s.base.clone(),
            body: if s.text().is_empty() {
                s.base.clone()
            } else {
                format!("{}{}{}", s.prefix, s.text(), s.suffix)
            },
            text: s.text(),
            applied: s.applied,
            recording: s.recording,
            starting: s.starting,
            processing: s.processing
                || (s.error.is_none() && s.parts.iter().any(|p| p.text.is_none())),
            complete: s.complete(),
            error: s.error.clone(),
            seconds: s.seconds,
            level: s.level,
        }
    }
}
struct State {
    prefs: Preferences,
    session: Option<Session>,
    model: String,
    backend: Option<String>,
    error: Option<String>,
    last_used: Instant,
    load_requested: bool,
    shortcut_pending: bool,
    shortcut_down: bool,
}
pub struct Service {
    root: PathBuf,
    state: Mutex<State>,
    engine: Mutex<Option<engine::Engine>>,
    stop: Mutex<Option<Arc<AtomicBool>>>,
    download: AtomicBool,
    download_cancel: AtomicBool,
}
pub struct Voice(pub Arc<Service>);
#[derive(Serialize)]
pub struct Status {
    enabled: bool,
    preload: bool,
    shortcut: String,
    state: String,
    backend: Option<String>,
    error: Option<String>,
    downloaded: u64,
    bytes: u64,
    cache: String,
    available: bool,
    session: Option<SessionView>,
}
impl Service {
    fn persist(&self, s: &State) -> HostResult<()> {
        write_json(&self.root.join("session.json"), &s.session)
    }
    fn save_prefs(&self, prefs: &Preferences) -> HostResult<()> {
        write_json(&self.root.join("settings.json"), prefs)
    }
    fn status(&self) -> HostResult<Status> {
        let cache = SpeechCache::for_user()?;
        let s = self.state.lock().unwrap();
        Ok(Status {
            enabled: s.prefs.enabled,
            preload: s.prefs.preload,
            shortcut: s.prefs.shortcut.clone(),
            state: if self.download.load(Ordering::SeqCst) {
                "downloading".into()
            } else {
                s.model.clone()
            },
            backend: s.backend.clone(),
            error: s.error.clone(),
            downloaded: cache.downloaded(),
            bytes: 1019141728,
            cache: cache.root.display().to_string(),
            available: cache.ready(),
            session: s.session.as_ref().map(SessionView::from),
        })
    }
    pub fn active(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.load_requested
            || s.model == "loading"
            || s.session.as_ref().is_some_and(|s| {
                s.recording
                    || s.starting
                    || s.processing
                    || s.parts.iter().any(|p| p.text.is_none()) && s.error.is_none()
            })
    }
    fn begin(&self, session: Session, stop: Arc<AtomicBool>) -> HostResult<()> {
        let mut s = self.state.lock().unwrap();
        if !s.prefs.enabled {
            return Err("请先在设置中启用语音输入".into());
        }
        if s.session.is_some() {
            return Err("已有录音草稿，请先完成或舍弃上一段录音".into());
        }
        // Persist first: a failed write must not leave a phantom starting session.
        write_json(&self.root.join("session.json"), &Some(&session))?;
        *self.stop.lock().unwrap() = Some(stop);
        s.session = Some(session);
        s.load_requested = true;
        Ok(())
    }
    fn ensure_engine(&self) -> HostResult<()> {
        let mut engine = self.engine.lock().unwrap();
        if engine.is_none() {
            if !self.state.lock().unwrap().prefs.enabled {
                return Err("语音输入已关闭".into());
            }
            {
                let mut s = self.state.lock().unwrap();
                s.model = "loading".into();
                s.error = None;
            }
            match engine::Engine::load() {
                Ok(worker) => {
                    let mut s = self.state.lock().unwrap();
                    if !s.prefs.enabled {
                        s.model = "unloaded".into();
                        s.backend = None;
                        return Err("语音输入已关闭".into());
                    }
                    s.backend = Some(worker.backend.clone());
                    s.model = "ready".into();
                    *engine = Some(worker);
                }
                Err(e) => {
                    let mut s = self.state.lock().unwrap();
                    s.model = "failed".into();
                    s.error = Some(e.clone());
                    return Err(e);
                }
            }
        }
        self.state.lock().unwrap().last_used = Instant::now();
        Ok(())
    }
    fn pump(&self) {
        if !self.state.lock().unwrap().prefs.enabled {
            // Recheck under the locks: a new enable/start may arrive between ticks.
            let mut engine = self.engine.lock().unwrap();
            let mut s = self.state.lock().unwrap();
            if !s.prefs.enabled {
                s.load_requested = false;
                s.model = "unloaded".into();
                s.backend = None;
                let previous = engine.take();
                drop(s);
                drop(engine);
                drop(previous);
                return;
            }
        }
        let (load, job) = {
            let mut s = self.state.lock().unwrap();
            let load = std::mem::take(&mut s.load_requested);
            let job = s.session.as_mut().and_then(|v| {
                if v.error.is_some() {
                    return None;
                }
                let index = v.parts.iter().position(|p| p.text.is_none())?;
                v.processing = true;
                Some((v.id.clone(), index, v.parts[index].file.clone()))
            });
            (load, job)
        };
        if load || job.is_some() {
            let result = self.ensure_engine().and_then(|_| {
                if let Some((_, _, file)) = &job {
                    let bytes = fs::read(self.root.join(file)).map_err(|_| "暂存录音不可读取")?;
                    if bytes.len() > 16000 * 20 * 4 || bytes.len() % 4 != 0 {
                        return Err("暂存录音格式无效".into());
                    }
                    let samples: Vec<f32> = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b))
                        .collect();
                    self.engine
                        .lock()
                        .unwrap()
                        .as_mut()
                        .ok_or("语音模型未加载")?
                        .transcribe(&samples)
                } else {
                    Ok(String::new())
                }
            });
            if result.is_err() {
                self.engine.lock().unwrap().take();
            }
            let mut s = self.state.lock().unwrap();
            if job.is_none()
                && let Err(e) = &result
            {
                if let Some(v) = s.session.as_mut() {
                    v.error = Some(e.clone());
                }
                self.stop();
                let _ = self.persist(&s);
            }
            s.last_used = Instant::now();
            if let Some((id, index, _)) = job
                && let Some(v) = s.session.as_mut().filter(|v| v.id == id)
            {
                v.processing = false;
                match result {
                    Ok(text) => v.parts[index].text = Some(text),
                    Err(e) => {
                        v.error = Some(e.clone());
                        s.error = Some(e);
                        s.model = "failed".into();
                    }
                }
                if let Err(e) = self.persist(&s)
                    && let Some(v) = s.session.as_mut()
                {
                    v.error = Some(e);
                }
            }
        } else {
            let unload = {
                let s = self.state.lock().unwrap();
                s.model == "ready"
                    && s.last_used.elapsed() > Duration::from_secs(60)
                    && !s
                        .session
                        .as_ref()
                        .is_some_and(|s| s.recording || s.starting)
            };
            if unload {
                self.engine.lock().unwrap().take();
                let mut s = self.state.lock().unwrap();
                s.model = "unloaded".into();
                s.backend = None;
            }
        }
    }
    fn listening(&self, id: &str) {
        let mut s = self.state.lock().unwrap();
        if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
            v.starting = false;
            v.recording = true;
        }
    }
    fn level(&self, id: &str, level: f32, n: usize) {
        let mut s = self.state.lock().unwrap();
        if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
            v.level = level;
            v.seconds += n as f32 / 16000.;
        }
    }
    fn enqueue(&self, id: &str, pcm: Vec<f32>) -> HostResult<()> {
        let mut s = self.state.lock().unwrap();
        let v = s
            .session
            .as_mut()
            .filter(|v| v.id == id)
            .ok_or("录音会话已结束")?;
        let file = format!("{}-{}.pcm", v.id, v.parts.len());
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.root.join(&file))
            .map_err(|_| "录音暂存失败，请检查磁盘空间")?;
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "录音暂存失败，请检查磁盘空间")?;
        v.parts.push(Part { file, text: None });
        self.persist(&s)
    }
    fn stop(&self) {
        if let Some(flag) = self.stop.lock().unwrap().as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }
    fn clear(&self, id: &str) -> HostResult<()> {
        let mut s = self.state.lock().unwrap();
        let Some(v) = s.session.as_ref().filter(|v| v.id == id) else {
            return Ok(());
        };
        if v.recording || v.starting || v.processing {
            return Err("请先结束录音和转写".into());
        }
        let files = v.parts.iter().map(|p| p.file.clone()).collect::<Vec<_>>();
        write_json(&self.root.join("session.json"), &Option::<Session>::None)?;
        s.session = None;
        for f in files {
            let _ = fs::remove_file(self.root.join(f));
        }
        Ok(())
    }
}
pub fn setup(app: &tauri::AppHandle) -> HostResult<()> {
    let root = memivy_core::memory::MemoryStore::environment_root()
        .map_err(|e| e.to_string())?
        .join("voice");
    // A broken optional voice configuration must never prevent ordinary capture.
    let mut setup_error = private_dir(&root).err();
    let prefs = match read_preferences(&root) {
        Ok(p) => p,
        Err(e) => {
            setup_error = Some(e);
            Preferences::default()
        }
    };
    let mut session: Option<Session> = fs::read(root.join("session.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .flatten();
    if let Some(v) = session.as_mut() {
        v.recording = false;
        v.starting = false;
        v.processing = false;
        if v.parts.iter().any(|p| p.text.is_none()) {
            v.error = Some("上次转写中断，录音已保留，请重试。".into());
        }
    }
    let preload = setup_error.is_none() && prefs.enabled && prefs.preload;
    let shortcut = prefs.shortcut.clone();
    let enabled = prefs.enabled;
    let service = Arc::new(Service {
        root,
        state: Mutex::new(State {
            prefs,
            session,
            model: "unloaded".into(),
            backend: None,
            error: setup_error,
            last_used: Instant::now(),
            load_requested: preload,
            shortcut_pending: false,
            shortcut_down: false,
        }),
        engine: Mutex::new(None),
        stop: Mutex::new(None),
        download: AtomicBool::new(false),
        download_cancel: AtomicBool::new(false),
    });
    app.manage(Voice(service.clone()));
    if enabled
        && !shortcut.is_empty()
        && let Err(e) = register_shortcut(app, &shortcut)
    {
        service.state.lock().unwrap().error = Some(e);
    }
    let weak = Arc::downgrade(&service);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(100));
            let Some(service) = weak.upgrade() else {
                break;
            };
            service.pump();
        }
    });
    Ok(())
}
#[tauri::command]
pub fn voice_status(voice: tauri::State<'_, Voice>) -> HostResult<Status> {
    voice.0.status()
}
#[tauri::command]
pub fn voice_start(
    voice: tauri::State<'_, Voice>,
    key: String,
    base: String,
    prefix: String,
    suffix: String,
) -> HostResult<Status> {
    if !["capture", "question", "quick_capture", "quick_question"].contains(&key.as_str())
        || base.len() > 100_000
        || prefix.len() + suffix.len() > 100_000
    {
        return Err("语音输入目标无效".into());
    }
    if !SpeechCache::for_user()?.ready() {
        return Err("请先在设置中下载语音模型".into());
    }
    let service = voice.0.clone();
    let id = uuid::Uuid::new_v4().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    service.begin(
        Session {
            id: id.clone(),
            key,
            base,
            prefix,
            suffix,
            parts: vec![],
            applied: false,
            recording: false,
            starting: true,
            processing: false,
            error: None,
            seconds: 0.,
            level: 0.,
        },
        stop.clone(),
    )?;
    std::thread::spawn(move || {
        let result = audio::record(service.clone(), id.clone(), stop);
        let mut s = service.state.lock().unwrap();
        if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
            v.starting = false;
            v.recording = false;
            v.level = 0.;
            if let Err(e) = result {
                v.error = Some(e);
            }
        }
        let _ = service.persist(&s);
    });
    voice.0.status()
}
#[tauri::command]
pub fn voice_stop(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    if voice
        .0
        .state
        .lock()
        .unwrap()
        .session
        .as_ref()
        .is_some_and(|s| s.id == id)
    {
        voice.0.stop();
    }
    Ok(())
}
#[tauri::command]
pub fn voice_clear(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    voice.0.clear(&id)
}
#[tauri::command]
pub fn voice_retry(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    let mut s = voice.0.state.lock().unwrap();
    if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
        if v.recording || v.starting || v.processing {
            return Err("请先停止录音".into());
        }
        let previous = v.error.take();
        if let Err(e) = voice.0.persist(&s) {
            s.session.as_mut().unwrap().error = previous;
            return Err(e);
        }
        s.error = None;
    }
    Ok(())
}
#[tauri::command]
pub async fn voice_control(
    app: tauri::AppHandle,
    window: tauri::WebviewWindow,
    voice: tauri::State<'_, Voice>,
    action: String,
    value: Option<String>,
) -> HostResult<()> {
    crate::workspace::require_main(&window)?;
    let service = voice.0.clone();
    match action.as_str() {
        "download" => {
            if service.download.swap(true, Ordering::SeqCst) {
                return Ok(());
            }
            service.download_cancel.store(false, Ordering::SeqCst);
            service.state.lock().unwrap().error = None;
            tauri::async_runtime::spawn(async move {
                let result = match SpeechCache::for_user() {
                    Ok(cache) => {
                        cache
                            .download(|| !service.download_cancel.load(Ordering::SeqCst))
                            .await
                    }
                    Err(e) => Err(e),
                };
                let mut s = service.state.lock().unwrap();
                match result {
                    Err(e) => s.error = Some(e),
                    Ok(()) if s.model == "failed" => {
                        s.model = "unloaded".into();
                        s.backend = None;
                    }
                    Ok(()) => {}
                }
                service.download.store(false, Ordering::SeqCst);
            });
        }
        "pause" => service.download_cancel.store(true, Ordering::SeqCst),
        "enable" | "disable" | "shortcut" => {
            crate::desktop::on_main(&app, move |app| configure(&app, &service, &action, value))
                .await?;
        }
        "preload" => {
            let mut s = service.state.lock().unwrap();
            let mut next = s.prefs.clone();
            next.preload = value.as_deref() == Some("true");
            service.save_prefs(&next)?;
            s.prefs = next;
        }
        "load" => {
            let mut s = service.state.lock().unwrap();
            if !s.prefs.enabled {
                return Err("请先启用语音输入".into());
            }
            if !SpeechCache::for_user()?.ready() {
                return Err("请先下载模型".into());
            }
            s.load_requested = true;
        }
        "unload" => {
            if service.active() {
                return Err("请先结束录音和转写".into());
            }
            service.engine.lock().unwrap().take();
            let mut s = service.state.lock().unwrap();
            s.model = "unloaded".into();
            s.backend = None;
        }
        _ => return Err("未知语音操作".into()),
    }
    Ok(())
}
// Registration uses main-thread-only OS APIs. Keep its state transaction on
// that same thread so a hotkey callback cannot deadlock against registration.
fn configure(
    app: &tauri::AppHandle,
    service: &Service,
    action: &str,
    value: Option<String>,
) -> HostResult<()> {
    match action {
        "enable" | "disable" => {
            if action == "disable" && service.active() {
                return Err("请先结束录音和转写".into());
            }
            let mut s = service.state.lock().unwrap();
            let enabled = action == "enable";
            if enabled && !s.prefs.enabled && !s.prefs.shortcut.is_empty() {
                register_shortcut(app, &s.prefs.shortcut)?;
            }
            let mut next = s.prefs.clone();
            next.enabled = enabled;
            if let Err(e) = service.save_prefs(&next) {
                if enabled && !s.prefs.enabled && !s.prefs.shortcut.is_empty() {
                    unregister_shortcut(app, &s.prefs.shortcut);
                }
                return Err(e);
            }
            if !enabled && !s.prefs.shortcut.is_empty() {
                unregister_shortcut(app, &s.prefs.shortcut);
            }
            s.prefs = next;
            if !enabled {
                s.load_requested = false;
                s.shortcut_pending = false;
                s.shortcut_down = false;
            }
        }
        "shortcut" => {
            let text = value.unwrap_or_default();
            if !text.is_empty() {
                crate::desktop::parse_shortcut(&text)?;
            }
            let mut s = service.state.lock().unwrap();
            if text == s.prefs.shortcut {
                return Ok(());
            }
            if s.prefs.enabled && !text.is_empty() {
                register_shortcut(app, &text)?;
            }
            let mut next = s.prefs.clone();
            next.shortcut = text.clone();
            if let Err(e) = service.save_prefs(&next) {
                if s.prefs.enabled && !text.is_empty() {
                    unregister_shortcut(app, &text);
                }
                return Err(e);
            }
            let old = std::mem::replace(&mut s.prefs, next).shortcut;
            s.shortcut_down = false;
            if s.prefs.enabled && !old.is_empty() {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let _ = app
                    .global_shortcut()
                    .unregister(crate::desktop::parse_shortcut(&old)?);
            }
        }
        _ => return Err("未知语音设置".into()),
    }
    Ok(())
}
fn register_shortcut(app: &tauri::AppHandle, text: &str) -> HostResult<()> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcut = crate::desktop::parse_shortcut(text)?;
    if app.global_shortcut().is_registered(shortcut) {
        return Err("快捷键已被占用，请换一个组合".into());
    }
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let voice = handle.state::<Voice>();
                {
                    let mut s = voice.0.state.lock().unwrap();
                    if event.state == ShortcutState::Released {
                        s.shortcut_down = false;
                        return;
                    }
                    if !s.prefs.enabled || s.shortcut_down {
                        return;
                    }
                    s.shortcut_down = true;
                    if s.session
                        .as_ref()
                        .is_some_and(|s| s.recording || s.starting)
                    {
                        drop(s);
                        voice.0.stop();
                        return;
                    }
                    s.shortcut_pending = true;
                }
                if crate::desktop::open(&handle, Some("capture".into()), true).is_ok() {
                    let _ = handle.emit_to("capture", "voice-shortcut", ());
                } else {
                    voice.0.state.lock().unwrap().shortcut_pending = false;
                }
            });
        })
        .map_err(|_| "语音快捷键注册失败，请换一个组合".into())
}
#[tauri::command]
pub fn voice_take_shortcut(window: tauri::WebviewWindow, voice: tauri::State<'_, Voice>) -> bool {
    if window.label() != "capture" {
        return false;
    }
    std::mem::take(&mut voice.0.state.lock().unwrap().shortcut_pending)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Service) {
        let dir = tempfile::tempdir().unwrap();
        let session = Session {
            id: "fixture".into(),
            key: "capture".into(),
            base: "existing text".into(),
            prefix: "existing ".into(),
            suffix: "text".into(),
            parts: vec![],
            applied: false,
            recording: true,
            starting: false,
            processing: false,
            error: None,
            seconds: 0.,
            level: 0.,
        };
        let service = Service {
            root: dir.path().to_owned(),
            state: Mutex::new(State {
                prefs: Preferences::default(),
                session: Some(session),
                model: "unloaded".into(),
                backend: None,
                error: None,
                last_used: Instant::now(),
                load_requested: false,
                shortcut_pending: false,
                shortcut_down: false,
            }),
            engine: Mutex::new(None),
            stop: Mutex::new(None),
            download: AtomicBool::new(false),
            download_cancel: AtomicBool::new(false),
        };
        (dir, service)
    }
    #[test]
    fn failed_session_writes_leave_recording_and_recovery_state_unchanged() {
        let (dir, service) = fixture();
        let session = service.state.lock().unwrap().session.take().unwrap();
        service.state.lock().unwrap().prefs.enabled = true;
        fs::create_dir(dir.path().join("session.json")).unwrap();
        assert!(
            service
                .begin(session.clone(), Arc::new(AtomicBool::new(false)))
                .is_err()
        );
        assert!(service.state.lock().unwrap().session.is_none());
        assert!(!service.state.lock().unwrap().load_requested);
        assert!(service.stop.lock().unwrap().is_none());
        let mut session = session;
        session.recording = false;
        service.state.lock().unwrap().session = Some(session);
        assert!(service.clear("fixture").is_err());
        assert!(service.state.lock().unwrap().session.is_some());
    }
    #[test]
    fn recording_and_failed_tail_cannot_be_mistaken_for_complete() {
        let (_dir, service) = fixture();
        service.enqueue("fixture", vec![0.1; 1600]).unwrap();
        let mut s = service.state.lock().unwrap();
        let v = s.session.as_mut().unwrap();
        assert!(!v.complete());
        v.recording = false;
        assert!(!v.complete());
        v.error = Some("interrupted".into());
        let view = SessionView::from(&*v);
        assert!(!view.processing);
        assert!(!view.complete);
        assert_eq!(view.body, "existing text");
        v.parts[0].text = Some("spoken ".into());
        assert!(!v.complete());
        v.error = None;
        assert!(v.complete());
        assert_eq!(SessionView::from(&*v).body, "existing spoken text");
    }
    #[test]
    fn invalid_preferences_fail_closed_without_overwriting_the_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        fs::write(&p, b"invalid").unwrap();
        assert!(read_preferences(d.path()).is_err());
        assert_eq!(fs::read(&p).unwrap(), b"invalid");
        fs::write(&p, vec![0; 4097]).unwrap();
        assert!(read_preferences(d.path()).is_err());
    }
    #[test]
    fn audio_is_private_and_clear_is_scoped_to_finished_session() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, service) = fixture();
        service.enqueue("fixture", vec![0.1; 1600]).unwrap();
        let file = dir.path().join("fixture-0.pcm");
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(service.clear("fixture").is_err());
        service.clear("other").unwrap();
        assert!(file.exists());
        service
            .state
            .lock()
            .unwrap()
            .session
            .as_mut()
            .unwrap()
            .recording = false;
        fs::write(dir.path().join("unrelated"), b"keep").unwrap();
        service.clear("fixture").unwrap();
        assert!(!file.exists());
        assert!(dir.path().join("unrelated").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("session.json")).unwrap(),
            "null"
        );
    }
}

#[tauri::command]
pub fn voice_applied(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    let mut s = voice.0.state.lock().unwrap();
    if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
        if !v.complete() {
            return Err("转写尚未完成".into());
        }
        let previous = v.applied;
        v.applied = true;
        if let Err(e) = voice.0.persist(&s) {
            s.session.as_mut().unwrap().applied = previous;
            return Err(e);
        }
    }
    Ok(())
}

fn unregister_shortcut(app: &tauri::AppHandle, text: &str) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    if let Ok(shortcut) = crate::desktop::parse_shortcut(text) {
        let _ = app.global_shortcut().unregister(shortcut);
    }
}
fn read_preferences(root: &std::path::Path) -> HostResult<Preferences> {
    let p = root.join("settings.json");
    if !p.exists() {
        return Ok(Preferences::default());
    }
    if fs::metadata(&p).map_err(|_| "语音设置不可读取")?.len() > 4096 {
        return Err("语音设置文件无效，文字输入仍可使用".into());
    }
    serde_json::from_slice(&fs::read(p).map_err(|_| "语音设置不可读取")?)
        .map_err(|_| "语音设置损坏，文字输入仍可使用".into())
}
