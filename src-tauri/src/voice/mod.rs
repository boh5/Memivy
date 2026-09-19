mod audio;
mod engine;
use crate::workspace::HostResult;
use memivy_core::models::{Binding, ModelSettings, Source};
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    enabled: bool,
    preload: bool,
    shortcut: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: false,
            preload: false,
            shortcut: "Alt+KeyR".into(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Part {
    file: String,
    text: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(default, serialize_with = "serialize_session_binding")]
    binding: Binding,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    label: String,
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
fn serialize_session_binding<S: serde::Serializer>(
    binding: &Binding,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut snapshot = binding.clone();
    snapshot.api_key = None;
    snapshot.serialize(serializer)
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
            && !self.text().trim().is_empty()
    }
    fn prepare_retry(&mut self) -> Result<(), &'static str> {
        if self.parts.is_empty() {
            return Err("voice_no_audio");
        }
        if self.parts.iter().all(|part| part.text.is_some()) && self.text().trim().is_empty() {
            for part in &mut self.parts {
                part.text = None;
            }
        }
        self.error = None;
        Ok(())
    }
    fn check_finished(&mut self) {
        if self.recording || self.starting || self.processing || self.error.is_some() {
            return;
        }
        let error = if self.parts.is_empty() {
            Some("voice_no_audio")
        } else if self.parts.iter().all(|p| p.text.is_some()) && self.text().trim().is_empty() {
            Some("voice_no_transcription")
        } else {
            None
        };
        if let Some(error) = error {
            self.error = Some(error.into());
            self.applied = false;
        }
    }
}
#[derive(Serialize)]
pub struct SessionView {
    source: Source,
    label: String,
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
            source: s.binding.source.clone(),
            label: s.label.clone(),
            id: s.id.clone(),
            key: s.key.clone(),
            base: s.base.clone(),
            body: if s.text().trim().is_empty() {
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
    recorder: Mutex<Option<std::thread::JoinHandle<()>>>,
    download: AtomicBool,
    download_cancel: AtomicBool,
}
pub struct Voice(pub Arc<Service>);
#[derive(Serialize)]
pub struct Status {
    source: Source,
    label: String,
    local_available: bool,
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
            .map_err(|_| crate::errors::HostError::new("voice_audio_save_failed"))
    }
    fn save_prefs(&self, prefs: &Preferences) -> HostResult<()> {
        write_json(&self.root.join("settings.json"), prefs)
            .map_err(|_| crate::errors::HostError::new("voice_audio_save_failed"))
    }
    fn status(&self) -> HostResult<Status> {
        let cache = SpeechCache::for_user().map_err(|_| "voice_cache_unavailable")?;
        let r = ModelSettings::read(self.root.parent().ok_or("voice_directory_invalid")?)?;
        let remote = r.voice.source == Source::Service;
        let s = self.state.lock().unwrap();
        Ok(Status {
            source: r.voice.source.clone(),
            label: if remote {
                r.voice.base_url.clone()
            } else {
                "voice_local".into()
            },
            local_available: cache.ready(),
            enabled: s.prefs.enabled,
            preload: s.prefs.preload,
            shortcut: s.prefs.shortcut.clone(),
            state: if self.download.load(Ordering::SeqCst) {
                "downloading".into()
            } else {
                if remote {
                    if s.error.is_some() {
                        "failed".into()
                    } else {
                        "ready".into()
                    }
                } else {
                    s.model.clone()
                }
            },
            backend: s.backend.clone(),
            error: s.error.clone(),
            downloaded: cache.downloaded(),
            bytes: 1019141728,
            cache: cache.root.display().to_string(),
            available: remote || cache.ready(),
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
    pub(crate) fn update_busy(&self) -> bool {
        self.active()
            || self.download.load(Ordering::SeqCst)
            || self
                .recorder
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
    }
    fn begin(&self, session: Session, stop: Arc<AtomicBool>) -> HostResult<()> {
        let _update_work = crate::updates::work()?;
        let mut s = self.state.lock().unwrap();
        if !s.prefs.enabled {
            return Err("voice_disabled".into());
        }
        if s.session.is_some() {
            return Err("voice_draft_exists".into());
        }
        // Persist first: a failed write must not leave a phantom starting session.
        write_json(&self.root.join("session.json"), &Some(&session))
            .map_err(|_| "voice_audio_save_failed")?;
        *self.stop.lock().unwrap() = Some(stop);
        s.session = Some(session);
        s.error = None;
        s.load_requested = true;
        Ok(())
    }
    fn ensure_engine(&self) -> HostResult<()> {
        let mut engine = self.engine.lock().unwrap();
        if engine.is_none() {
            if !self.state.lock().unwrap().prefs.enabled {
                return Err("voice_disabled".into());
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
                        return Err("voice_disabled".into());
                    }
                    s.backend = Some(worker.backend.clone());
                    s.model = "ready".into();
                    *engine = Some(worker);
                }
                Err(e) => {
                    let mut s = self.state.lock().unwrap();
                    s.model = "failed".into();
                    s.error = Some(e.clone());
                    return Err(e.into());
                }
            }
        }
        self.state.lock().unwrap().last_used = Instant::now();
        Ok(())
    }
    /// Preserve the one retained recording's destination across model changes.
    /// Only a stopped failed recording accepts a corrected key for that target.
    pub(crate) fn retain_recording_config(
        &self,
        registry: &mut ModelSettings,
        next: &Binding,
    ) -> HostResult<()> {
        let state = self.state.lock().unwrap();
        registry.retained_voice = if let Some(session) = &state.session
            && session.binding.source == Source::Service
        {
            let mut config =
                registry.voice_config(&session.id, &session.endpoint, &session.binding.model)?;
            if session.error.is_some()
                && !session.recording
                && !session.starting
                && !session.processing
                && next.source == Source::Service
                && next.base_url == session.endpoint
                && next.model == session.binding.model
            {
                config.api_key = next.api_key.clone();
            }
            Some(memivy_core::models::RetainedVoice {
                session_id: session.id.clone(),
                config,
            })
        } else {
            None
        };
        Ok(())
    }
    fn pump(&self) {
        let Ok(_update_work) = crate::updates::work() else {
            return;
        };
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
                Some((
                    v.id.clone(),
                    index,
                    v.parts[index].file.clone(),
                    v.binding.clone(),
                    v.endpoint.clone(),
                ))
            });
            (load, job)
        };
        if load || job.is_some() {
            let result = (|| {
                if let Some((id, _, file, binding, endpoint)) = &job {
                    let bytes =
                        fs::read(self.root.join(file)).map_err(|_| "voice_audio_unreadable")?;
                    if bytes.len() > 16000 * 20 * 4 || bytes.len() % 4 != 0 {
                        return Err("voice_audio_invalid".into());
                    }
                    let samples: Vec<f32> = bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b))
                        .collect();
                    if binding.source == Source::Service {
                        let r = ModelSettings::read(
                            self.root.parent().ok_or("voice_directory_invalid")?,
                        )?;
                        let m = r.voice_config(id, endpoint, &binding.model)?;
                        return memivy_core::models::transcribe(&m, &samples).map_err(String::from);
                    }
                    self.ensure_engine()?;
                    self.engine
                        .lock()
                        .unwrap()
                        .as_mut()
                        .ok_or("voice_not_loaded")?
                        .transcribe(&samples)
                } else {
                    if ModelSettings::read(self.root.parent().ok_or("voice_directory_invalid")?)?
                        .voice
                        .source
                        == Source::Local
                    {
                        self.ensure_engine()?;
                    }
                    Ok(String::new())
                }
            })();
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
            if let Some((id, index, _, _, _)) = job
                && let Some(v) = s.session.as_mut().filter(|v| v.id == id)
            {
                v.processing = false;
                match result {
                    Ok(text) => v.parts[index].text = Some(text),
                    Err(e) => {
                        v.error = Some(e.clone());
                        s.error = Some(e.to_string());
                        s.model = "failed".into();
                    }
                }
                if let Some(v) = s.session.as_mut() {
                    v.check_finished();
                }
                if let Err(e) = self.persist(&s)
                    && let Some(v) = s.session.as_mut()
                {
                    v.error = Some(e.to_string());
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
    fn start_recording(
        self: &Arc<Self>,
        session: Session,
        timeout: Duration,
        record: impl FnOnce(Arc<Self>, String, Arc<AtomicBool>) -> Result<(), String> + Send + 'static,
    ) -> HostResult<()> {
        // A timed-out OS call cannot be killed safely. Keep one worker until it
        // returns, so retries cannot accumulate blocked threads or open devices.
        let mut worker = self.recorder.lock().unwrap();
        if worker.as_ref().is_some_and(|worker| !worker.is_finished()) {
            return Err("microphone_start_timeout".into());
        }
        let id = session.id.clone();
        let stop = Arc::new(AtomicBool::new(false));
        self.begin(session, stop.clone())?;
        let service = self.clone();
        let watchdog = self.clone();
        let watched_id = id.clone();
        let watched_stop = stop.clone();
        let (done, finished) = std::sync::mpsc::channel();
        *worker = Some(std::thread::spawn(move || {
            let result = record(service.clone(), id.clone(), stop);
            let mut s = service.state.lock().unwrap();
            if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
                v.starting = false;
                v.recording = false;
                v.level = 0.;
                if v.error.is_none() {
                    v.error = result.err();
                }
                v.check_finished();
            }
            let _ = service.persist(&s);
            let _ = done.send(());
        }));
        std::thread::spawn(move || {
            if finished.recv_timeout(timeout) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                let mut s = watchdog.state.lock().unwrap();
                if let Some(v) = s
                    .session
                    .as_mut()
                    .filter(|v| v.id == watched_id && v.starting)
                {
                    watched_stop.store(true, Ordering::SeqCst);
                    v.starting = false;
                    v.error = Some("microphone_start_timeout".into());
                    let _ = watchdog.persist(&s);
                }
            }
        });
        Ok(())
    }
    fn listening(&self, id: &str) -> bool {
        let mut s = self.state.lock().unwrap();
        if let Some(v) = s
            .session
            .as_mut()
            .filter(|v| v.id == id && v.starting && v.error.is_none())
        {
            v.starting = false;
            v.recording = true;
            true
        } else {
            false
        }
    }
    fn stop_session(&self, id: &str) {
        let mut s = self.state.lock().unwrap();
        if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
            self.stop();
            // No audio exists during device initialization. Restore text input
            // immediately even when CoreAudio has not returned from its call.
            if v.starting {
                v.starting = false;
                v.check_finished();
                let _ = self.persist(&s);
            }
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
            .ok_or("voice_session_ended")?;
        let file = format!("{}-{}.pcm", v.id, v.parts.len());
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.root.join(&file))
            .map_err(|_| "voice_audio_save_failed")?;
        let bytes: Vec<u8> = pcm.iter().flat_map(|v| v.to_le_bytes()).collect();
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "voice_audio_save_failed")?;
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
            return Err("voice_busy".into());
        }
        let files = v.parts.iter().map(|p| p.file.clone()).collect::<Vec<_>>();
        write_json(&self.root.join("session.json"), &Option::<Session>::None)
            .map_err(|_| "voice_audio_save_failed")?;
        s.session = None;
        for f in files {
            let _ = fs::remove_file(self.root.join(f));
        }
        Ok(())
    }
}
pub fn setup(app: &tauri::AppHandle) -> HostResult<()> {
    let root = app
        .state::<crate::workspace::Workspace>()
        .store
        .database_path()
        .with_file_name("voice");
    // A broken optional voice configuration must never prevent ordinary capture.
    let mut setup_error = private_dir(&root)
        .err()
        .map(|_| "voice_directory_invalid".to_string());
    let mut prefs = match read_preferences(&root) {
        Ok(p) => p,
        Err(e) => {
            setup_error = Some(e.to_string());
            Preferences::default()
        }
    };
    if app.config().identifier == crate::storage::DEVELOPMENT_IDENTIFIER
        && !root.join("settings.json").exists()
    {
        prefs.shortcut = "Alt+Shift+KeyR".into();
    }
    let mut session: Option<Session> = fs::read(root.join("session.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .flatten();
    if let Some(v) = session.as_mut() {
        v.recording = false;
        v.starting = false;
        v.processing = false;
        if v.parts.iter().any(|p| p.text.is_none()) {
            v.error = Some("voice_interrupted".into());
        }
        v.check_finished();
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
        recorder: Mutex::new(None),
        download: AtomicBool::new(false),
        download_cancel: AtomicBool::new(false),
    });
    app.manage(Voice(service.clone()));
    if enabled
        && !shortcut.is_empty()
        && let Err(e) = register_shortcut(app, &shortcut)
    {
        service.state.lock().unwrap().error = Some(e.to_string());
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
    let _update_work = crate::updates::work()?;
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
    let _update_work = crate::updates::work()?;
    let valid_target = ["input", "quick_input"].contains(&key.as_str())
        || key
            .strip_prefix("discussion:")
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
    if !valid_target || base.len() > 100_000 || prefix.len() + suffix.len() > 100_000 {
        return Err("voice_target_invalid".into());
    }
    let registry = ModelSettings::read(voice.0.root.parent().ok_or("voice_directory_invalid")?)?;
    let mut binding = registry.voice.clone();
    binding.api_key = None;
    let (endpoint, label) = if binding.source == Source::Service {
        let m = binding.model_config()?;
        (m.base_url.clone(), m.base_url)
    } else {
        if !SpeechCache::for_user()
            .map_err(|_| "voice_cache_unavailable")?
            .ready()
        {
            return Err("voice_download_required".into());
        }
        (String::new(), "voice_local".into())
    };
    let service = voice.0.clone();
    let id = uuid::Uuid::new_v4().to_string();
    service.start_recording(
        Session {
            binding,
            endpoint,
            label,
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
        Duration::from_secs(10),
        audio::record,
    )?;
    voice.0.status()
}
#[tauri::command]
pub fn voice_stop(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    voice.0.stop_session(&id);
    Ok(())
}
#[tauri::command]
pub fn voice_clear(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    voice.0.clear(&id)
}
#[tauri::command]
pub fn voice_retry(voice: tauri::State<'_, Voice>, id: String) -> HostResult<()> {
    let _update_work = crate::updates::work()?;
    let mut s = voice.0.state.lock().unwrap();
    if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
        if v.recording || v.starting || v.processing {
            return Err("voice_recording_active".into());
        }
        let previous = v.clone();
        v.prepare_retry()?;
        if let Err(e) = voice.0.persist(&s) {
            s.session = Some(previous);
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
    let _update_work = crate::updates::work()?;
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
                    Err(_) => {
                        s.error = Some(
                            if service.download_cancel.load(Ordering::SeqCst) {
                                "cancelled"
                            } else {
                                "voice_download_failed"
                            }
                            .into(),
                        )
                    }
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
                return Err("voice_disabled".into());
            }
            if !SpeechCache::for_user()
                .map_err(|_| "voice_cache_unavailable")?
                .ready()
            {
                return Err("voice_download_required".into());
            }
            s.load_requested = true;
        }
        "unload" => {
            if service.active() {
                return Err("voice_busy".into());
            }
            service.engine.lock().unwrap().take();
            let mut s = service.state.lock().unwrap();
            s.model = "unloaded".into();
            s.backend = None;
        }
        _ => return Err("voice_action_invalid".into()),
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
                return Err("voice_busy".into());
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
            s.shortcut_pending = false;
            if s.prefs.enabled && !old.is_empty() {
                use tauri_plugin_global_shortcut::GlobalShortcutExt;
                let _ = app
                    .global_shortcut()
                    .unregister(crate::desktop::parse_shortcut(&old)?);
            }
        }
        _ => return Err("voice_action_invalid".into()),
    }
    Ok(())
}
fn register_shortcut(app: &tauri::AppHandle, text: &str) -> HostResult<()> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcut = crate::desktop::parse_shortcut(text)?;
    if app.global_shortcut().is_registered(shortcut) {
        return Err("shortcut_taken".into());
    }
    app.global_shortcut()
        .on_shortcut(shortcut, |app, shortcut, event| {
            let shortcut = *shortcut;
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                let Ok(_update_work) = crate::updates::work() else {
                    return;
                };
                let voice = handle.state::<Voice>();
                {
                    let mut s = voice.0.state.lock().unwrap();
                    if crate::desktop::parse_shortcut(&s.prefs.shortcut).ok() != Some(shortcut) {
                        return;
                    }
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
                        let id = s.session.as_ref().unwrap().id.clone();
                        drop(s);
                        voice.0.stop_session(&id);
                        return;
                    }
                    s.shortcut_pending = true;
                }
                if crate::desktop::open(&handle, true).is_ok() {
                    let _ = handle.emit_to("capture", "voice-shortcut", ());
                } else {
                    voice.0.state.lock().unwrap().shortcut_pending = false;
                }
            });
        })
        .map_err(|_| "voice_shortcut_failed".into())
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
            binding: Binding::default(),
            endpoint: String::new(),
            label: "voice_local".into(),
            id: "fixture".into(),
            key: "input".into(),
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
            recorder: Mutex::new(None),
            download: AtomicBool::new(false),
            download_cancel: AtomicBool::new(false),
        };
        (dir, service)
    }
    #[test]
    fn default_shortcut_preserves_an_explicitly_cleared_binding() {
        assert_eq!(Preferences::default().shortcut, "Alt+KeyR");
        let prefs: Preferences = serde_json::from_str(r#"{"shortcut":""}"#).unwrap();
        assert!(prefs.shortcut.is_empty());
    }
    #[test]
    fn stalled_device_start_times_out_without_losing_text_or_spawning_more_workers() {
        let (_dir, service) = fixture();
        let mut session = service.state.lock().unwrap().session.take().unwrap();
        session.starting = true;
        session.recording = false;
        service.state.lock().unwrap().prefs.enabled = true;
        let service = Arc::new(service);
        let (release, blocked) = std::sync::mpsc::channel();
        let (result, returned) = std::sync::mpsc::channel();
        service
            .start_recording(
                session.clone(),
                Duration::from_millis(10),
                move |s, id, stop| {
                    blocked.recv().unwrap();
                    result
                        .send((stop.load(Ordering::SeqCst), s.listening(&id)))
                        .unwrap();
                    Ok(())
                },
            )
            .unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while service
            .state
            .lock()
            .unwrap()
            .session
            .as_ref()
            .unwrap()
            .starting
        {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
        {
            let state = service.state.lock().unwrap();
            let v = state.session.as_ref().unwrap();
            assert_eq!(v.error.as_deref(), Some("microphone_start_timeout"));
            assert_eq!(v.base, "existing text");
            assert!(!v.recording);
        }
        service.state.lock().unwrap().load_requested = false;
        assert!(
            service.update_busy(),
            "An unfinished recorder must block installation even after its watchdog times out"
        );
        service.clear(&session.id).unwrap();
        assert!(
            service
                .start_recording(session, Duration::from_secs(1), |_, _, _| panic!(
                    "second recorder"
                ))
                .is_err()
        );
        assert!(service.state.lock().unwrap().session.is_none());
        release.send(()).unwrap();
        assert_eq!(
            returned.recv_timeout(Duration::from_secs(2)).unwrap(),
            (true, false)
        );
        service
            .recorder
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .join()
            .unwrap();
        assert!(service.state.lock().unwrap().session.is_none());
    }
    #[test]
    fn stopping_device_start_is_immediate_and_late_worker_cannot_restart_it() {
        let (_dir, service) = fixture();
        let stop = Arc::new(AtomicBool::new(false));
        *service.stop.lock().unwrap() = Some(stop.clone());
        {
            let mut state = service.state.lock().unwrap();
            let v = state.session.as_mut().unwrap();
            v.recording = false;
            v.starting = true;
        }
        service.stop_session("other-session");
        assert!(!stop.load(Ordering::SeqCst));
        service.stop_session("fixture");
        assert!(stop.load(Ordering::SeqCst));
        assert!(!service.listening("fixture"));
        let state = service.state.lock().unwrap();
        let v = state.session.as_ref().unwrap();
        assert!(!v.starting && !v.recording);
        assert_eq!(v.error.as_deref(), Some("voice_no_audio"));
        assert_eq!(v.base, "existing text");
    }
    #[test]
    fn device_start_deadline_does_not_stop_a_live_recording() {
        let (_dir, service) = fixture();
        let mut session = service.state.lock().unwrap().session.take().unwrap();
        session.recording = false;
        session.starting = true;
        service.state.lock().unwrap().prefs.enabled = true;
        let service = Arc::new(service);
        let (ready, listening) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        service
            .start_recording(session, Duration::from_millis(10), move |s, id, stop| {
                assert!(s.listening(&id));
                ready.send(()).unwrap();
                blocked.recv().unwrap();
                assert!(!stop.load(Ordering::SeqCst));
                Ok(())
            })
            .unwrap();
        listening.recv_timeout(Duration::from_secs(2)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert!(
            service
                .state
                .lock()
                .unwrap()
                .session
                .as_ref()
                .unwrap()
                .recording
        );
        release.send(()).unwrap();
        service
            .recorder
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .join()
            .unwrap();
    }
    #[test]
    fn retained_recording_keeps_original_model_without_copying_credentials() {
        let (dir, mut service) = fixture();
        service.root = dir.path().join("voice");
        fs::create_dir(&service.root).unwrap();
        let mut registry = ModelSettings {
            voice: Binding {
                source: Source::Service,
                base_url: "http://127.0.0.1:1234/v1".into(),
                api_key: Some("fixture-private-key".into()),
                model: "first-model".into(),
                ..Binding::default()
            },
            ..ModelSettings::default()
        };
        registry.save(dir.path(), "initial").unwrap();
        {
            let mut state = service.state.lock().unwrap();
            let session = state.session.as_mut().unwrap();
            session.binding = registry.voice.clone();
            session.endpoint = registry.voice.base_url.clone();
            service.persist(&state).unwrap();
        }
        let revision = registry.revision.clone();
        let mut next = registry.voice.clone();
        next.model = "next-recording-model".into();
        service
            .retain_recording_config(&mut registry, &next)
            .unwrap();
        registry.voice = next;
        registry.save(dir.path(), &revision).unwrap();
        let bytes = fs::read(service.root.join("session.json")).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("fixture-private-key"));
        let retained: Session = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(retained.binding.model, "first-model");
        assert_eq!(
            ModelSettings::read(dir.path())
                .unwrap()
                .voice_config(&retained.id, &retained.endpoint, &retained.binding.model)
                .unwrap()
                .model,
            "first-model"
        );
        assert_eq!(
            registry.retained_voice.unwrap().config.api_key.as_deref(),
            Some("fixture-private-key")
        );
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
    fn corrected_key_repairs_only_the_same_failed_recording_endpoint_and_model() {
        let (_dir, service) = fixture();
        let binding = Binding {
            source: Source::Service,
            base_url: "http://127.0.0.1:1234/v1".into(),
            api_key: Some("old-key".into()),
            model: "qa-asr".into(),
            ..Binding::default()
        };
        {
            let mut state = service.state.lock().unwrap();
            let session = state.session.as_mut().unwrap();
            session.binding = binding.clone();
            session.endpoint = binding.base_url.clone();
            session.recording = false;
            session.error = Some("authentication failed".into());
        }
        let mut r = ModelSettings {
            voice: binding.clone(),
            ..ModelSettings::default()
        };
        let mut next = Binding {
            api_key: Some("fixed-key".into()),
            model: "different".into(),
            ..binding.clone()
        };
        service.retain_recording_config(&mut r, &next).unwrap();
        assert_eq!(
            r.retained_voice.as_ref().unwrap().config.api_key.as_deref(),
            Some("old-key")
        );
        next.model = binding.model.clone();
        next.base_url = "http://127.0.0.1:9999/v1".into();
        service.retain_recording_config(&mut r, &next).unwrap();
        assert_eq!(
            r.retained_voice.as_ref().unwrap().config.api_key.as_deref(),
            Some("old-key")
        );
        next.base_url = binding.base_url;
        service.retain_recording_config(&mut r, &next).unwrap();
        assert_eq!(
            r.retained_voice.as_ref().unwrap().config.api_key.as_deref(),
            Some("fixed-key")
        );
    }

    #[test]
    fn new_recording_clears_stale_error_after_session_is_persisted() {
        let (_dir, service) = fixture();
        let session = {
            let mut state = service.state.lock().unwrap();
            state.prefs.enabled = true;
            state.error = Some("old failure".into());
            state.session.take().unwrap()
        };
        service
            .begin(session, Arc::new(AtomicBool::new(false)))
            .unwrap();
        assert!(service.state.lock().unwrap().error.is_none());
    }
    #[test]
    fn stopped_empty_capture_is_not_a_successful_transcription() {
        let (_dir, service) = fixture();
        let mut s = service.state.lock().unwrap();
        let v = s.session.as_mut().unwrap();
        v.recording = false;
        v.applied = true;
        v.check_finished();
        assert!(!v.complete());
        assert!(!v.applied);
        assert_eq!(v.error.as_deref(), Some("voice_no_audio"));
        assert_eq!(SessionView::from(&*v).body, "existing text");
    }
    #[test]
    fn empty_model_result_keeps_audio_and_reports_no_transcription() {
        let (dir, service) = fixture();
        service.enqueue("fixture", vec![0.001; 1600]).unwrap();
        let mut s = service.state.lock().unwrap();
        let v = s.session.as_mut().unwrap();
        v.recording = false;
        v.parts[0].text = Some("  ".into());
        v.check_finished();
        assert!(!v.complete());
        assert_eq!(v.error.as_deref(), Some("voice_no_transcription"));
        assert_eq!(SessionView::from(&*v).body, "existing text");
        assert!(dir.path().join(&v.parts[0].file).exists());
    }
    #[test]
    fn empty_transcription_can_retry_after_a_save_failure() {
        let (dir, service) = fixture();
        service.enqueue("fixture", vec![0.001; 1600]).unwrap();
        let mut s = service.state.lock().unwrap();
        let v = s.session.as_mut().unwrap();
        v.recording = false;
        v.parts[0].text = Some("  ".into());
        v.error = Some("voice_audio_save_failed".into());
        v.prepare_retry().unwrap();
        assert!(v.parts[0].text.is_none());
        assert!(SessionView::from(&*v).processing);
        assert_eq!(SessionView::from(&*v).body, "existing text");
        assert!(dir.path().join(&v.parts[0].file).exists());
        v.parts[0].text = Some("spoken ".into());
        v.check_finished();
        assert!(v.complete());
        assert_eq!(SessionView::from(&*v).body, "existing spoken text");
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
    let _update_work = crate::updates::work()?;
    let mut s = voice.0.state.lock().unwrap();
    if let Some(v) = s.session.as_mut().filter(|v| v.id == id) {
        if !v.complete() {
            return Err("voice_incomplete".into());
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
    if fs::metadata(&p)
        .map_err(|_| "voice_settings_unreadable")?
        .len()
        > 4096
    {
        return Err("voice_settings_invalid".into());
    }
    serde_json::from_slice(&fs::read(p).map_err(|_| "voice_settings_unreadable")?)
        .map_err(|_| "voice_settings_invalid".into())
}

pub(crate) async fn set_enabled(app: &tauri::AppHandle, enabled: bool) -> HostResult<()> {
    let service = app.state::<Voice>().0.clone();
    crate::desktop::on_main(app, move |app| {
        configure(
            &app,
            &service,
            if enabled { "enable" } else { "disable" },
            None,
        )
    })
    .await
}

pub(crate) async fn clear_model(app: &tauri::AppHandle) -> HostResult<()> {
    let service = app.state::<Voice>().0.clone();
    if service.active() || service.download.load(Ordering::SeqCst) {
        return Err("voice_busy".into());
    }
    if ModelSettings::read(service.root.parent().ok_or("voice_directory_invalid")?)?
        .voice
        .source
        == Source::Local
    {
        set_enabled(app, false).await?;
    }
    service.engine.lock().unwrap().take();
    SpeechCache::for_user()
        .map_err(|_| "voice_cache_unavailable")?
        .clear()?;
    let mut s = service.state.lock().unwrap();
    s.model = "unloaded".into();
    s.backend = None;
    Ok(())
}
