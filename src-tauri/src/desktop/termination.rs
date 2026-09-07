//! AppKit's standard Quit / Dock actions call NSApplication.terminate directly;
//! Tao's applicationWillTerminate notification is too late to preserve drafts.
use super::{HostResult, Workspace};
use objc2::{
    class, msg_send,
    rc::Retained,
    runtime::{AnyObject, ClassBuilder, Sel},
    sel,
};
use objc2_app_kit::NSApplicationTerminateReply;
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tauri::Manager;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static WAITING: AtomicBool = AtomicBool::new(false);

extern "C-unwind" fn should_terminate(
    _: &AnyObject,
    _: Sel,
    _: &AnyObject,
) -> NSApplicationTerminateReply {
    let Some(app) = APP.get() else {
        return NSApplicationTerminateReply::TerminateCancel;
    };
    if app.state::<Workspace>().exiting.load(Ordering::Relaxed) {
        return NSApplicationTerminateReply::TerminateNow;
    }
    if !WAITING.swap(true, Ordering::Relaxed) {
        let h = app.clone();
        // Defer the reply until AppKit has received TerminateLater, including
        // the startup case where no WebView has registered yet.
        tauri::async_runtime::spawn(async move {
            let _ = super::on_main(&h, |app| {
                super::request_quit(&app);
                Ok(())
            })
            .await;
        });
    }
    NSApplicationTerminateReply::TerminateLater
}

pub(super) fn install(app: &tauri::AppHandle) -> HostResult<()> {
    // Setup runs on the main thread. Preserve Tao's delegate and all its
    // methods/ivars; the same-size subclass only adds this termination hook.
    unsafe {
        let application: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let delegate: Retained<AnyObject> = msg_send![&*application, delegate];
        let mut builder = ClassBuilder::new(c"MemivyApplicationDelegate", delegate.class())
            .ok_or("无法安装退出时的草稿保护")?;
        builder.add_method(
            sel!(applicationShouldTerminate:),
            should_terminate as extern "C-unwind" fn(_, _, _) -> _,
        );
        APP.set(app.clone()).map_err(|_| "退出保护已初始化")?;
        AnyObject::set_class(&delegate, builder.register());
        // Refresh AppKit's cached optional delegate-method availability.
        let _: () = msg_send![&*application, setDelegate: &*delegate];
    }
    Ok(())
}

/// Called on the main thread after every WebView has acknowledged, or on
/// cancellation. Returns whether an AppKit termination request was answered.
pub(super) fn reply(allow: bool) -> bool {
    if !WAITING.swap(false, Ordering::Relaxed) {
        return false;
    }
    unsafe {
        let application: Retained<AnyObject> = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![&*application, replyToApplicationShouldTerminate: allow];
    }
    true
}
