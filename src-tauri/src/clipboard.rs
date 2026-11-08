use crate::errors::HostError;
use crate::workspace::HostResult;

pub(crate) async fn write_text(
    app: &tauri::AppHandle,
    text: String,
    error_code: &'static str,
) -> HostResult<()> {
    crate::desktop::on_main(app, move |_| {
        use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
        use objc2_foundation::NSString;
        let clipboard = NSPasteboard::generalPasteboard();
        clipboard.clearContents();
        // AppKit owns this process-lifetime constant and runs on the main thread.
        if clipboard.setString_forType(&NSString::from_str(&text), unsafe {
            NSPasteboardTypeString
        }) {
            Ok(())
        } else {
            Err(HostError::new(error_code))
        }
    })
    .await
    .map_err(|_| HostError::new(error_code))
}
