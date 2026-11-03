use std::{path::PathBuf, sync::Mutex};

#[cfg(target_os = "windows")]
use tauri::Manager;
use tauri::{State, ipc::Request};
use tauri_plugin_dialog::DialogExt;

use crate::{
    app_settings::{self, Toggle},
    editor_window,
    output::{self, raw_body},
};

/// The path the user picked in the save dialog. `save_image` only ever writes there,
/// so the webview cannot choose arbitrary destinations.
#[derive(Default)]
pub struct PendingSave(Mutex<Option<PathBuf>>);

/// Puts the PNG on the general pasteboard as `public.png`, without re-encoding, then closes
/// the editor when "Cerrar la captura después de copiarla" is on. The setting is read from
/// the settings store in memory. Kept synchronous so AppKit is called from the main thread.
#[cfg(not(target_os = "windows"))]
#[tauri::command]
pub fn copy_png(app: tauri::AppHandle, request: Request<'_>) -> Result<(), String> {
    write_png_to_pasteboard(raw_body(&request)?)?;
    if app_settings::toggle(&app, Toggle::CloseAfterCopy) {
        editor_window::hide(&app);
    }
    Ok(())
}

/// Windows needs its own PNG → RGBA decode before the pasteboard accepts it (`arboard` takes
/// raw pixels, not PNG bytes), which costs real CPU/memory on a scroll-capture-sized image —
/// unlike AppKit, `arboard` has no main-thread requirement, so that work runs off it here.
#[cfg(target_os = "windows")]
#[tauri::command]
pub async fn copy_png(app: tauri::AppHandle, request: Request<'_>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let bytes = raw_body(&request)?.to_vec();
    // Generation is taken at the START: any copy that starts after this one invalidates it.
    let generation = app.state::<CopyGeneration>().next.fetch_add(1, Ordering::SeqCst);
    let worker_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        write_png_to_pasteboard(&worker_app.state::<CopyGeneration>(), generation, &bytes)
    })
    .await
    .map_err(|error| error.to_string())??;
    if app_settings::toggle(&app, Toggle::CloseAfterCopy) {
        editor_window::hide(&app);
    }
    Ok(())
}

/// Shows the native save dialog, starting in the save folder with the default name, and
/// remembers the choice. Returns the path so the editor can pick PNG or JPEG from its
/// extension, or `None` if the user cancelled. `datetime` is the editor's local export time.
#[tauri::command]
pub async fn choose_save_path(
    app: tauri::AppHandle,
    pending: State<'_, PendingSave>,
    datetime: String,
) -> Result<Option<String>, String> {
    let (name, folder) = output::save_dialog_defaults(&app, &datetime)?;
    let mut dialog = app.dialog().file().set_file_name(name);
    if let Some(folder) = folder {
        dialog = dialog.set_directory(folder);
    }
    let chosen = dialog
        .add_filter("PNG", &["png"])
        .add_filter("JPEG", &["jpg", "jpeg"])
        .blocking_save_file()
        .map(|path| path.into_path().map_err(|error| error.to_string()))
        .transpose()?;
    let display = chosen.as_ref().map(|path| path.to_string_lossy().into_owned());
    *pending.0.lock().unwrap() = chosen;
    Ok(display)
}

/// Writes the encoded image to the path from the last `choose_save_path`, off the main thread.
#[tauri::command]
pub async fn save_image(app: tauri::AppHandle, pending: State<'_, PendingSave>, request: Request<'_>) -> Result<(), String> {
    let path = pending.0.lock().unwrap().take().ok_or("no save destination was chosen")?;
    output::write_file(path.clone(), raw_body(&request)?.to_vec()).await?;
    output::remember_saved(&app, path);
    Ok(())
}

#[cfg(target_os = "macos")]
fn write_png_to_pasteboard(bytes: &[u8]) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use objc2_foundation::NSData;

    let data = NSData::with_bytes(bytes);
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: AppKit defines this constant for the lifetime of the process.
    let png_type = unsafe { NSPasteboardTypePNG };
    if pasteboard.setData_forType(Some(&data), png_type) {
        Ok(())
    } else {
        Err("the pasteboard rejected the image".into())
    }
}

/// Replaces the pasteboard contents with plain text. Call it on the main thread.
#[cfg(target_os = "macos")]
pub fn write_text_to_pasteboard(text: &str) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;

    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    // SAFETY: AppKit defines this constant for the lifetime of the process.
    let string_type = unsafe { NSPasteboardTypeString };
    if pasteboard.setString_forType(&NSString::from_str(text), string_type) {
        Ok(())
    } else {
        Err("the pasteboard rejected the text".into())
    }
}

#[cfg(target_os = "windows")]
use arboard::SetExtWindows;

/// Tracks which copy is the most recent one still writing. Without this, a slow copy of a huge
/// scroll capture could finish after a later, smaller copy and overwrite it on the pasteboard.
/// Registered once with `.manage(CopyGeneration::default())` in `lib.rs`, Windows only.
#[cfg(target_os = "windows")]
#[derive(Default)]
pub struct CopyGeneration {
    next: std::sync::atomic::AtomicU64,
    /// Serialises the "am I still the latest?" check with the pasteboard write itself, so no
    /// other worker's check can land in between them.
    write: Mutex<()>,
}

#[cfg(target_os = "windows")]
fn decode_rgba(bytes: &[u8]) -> Result<xcap::image::RgbaImage, String> {
    // The decoder's default `max_alloc` (512MiB) is lower than what this app otherwise allows.
    let mut reader = xcap::image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    reader.limits(crate::import::decode_limits());
    Ok(reader.decode().map_err(|error| error.to_string())?.to_rgba8())
}

/// Runs inside `spawn_blocking`. `generation` was taken when this copy started.
#[cfg(target_os = "windows")]
fn write_png_to_pasteboard(state: &CopyGeneration, generation: u64, bytes: &[u8]) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let rgba = decode_rgba(bytes)?;
    let image_data = arboard::ImageData { width: rgba.width() as usize, height: rgba.height() as usize, bytes: rgba.into_raw().into() };
    let _write = state.write.lock().unwrap();
    if state.next.load(Ordering::SeqCst) != generation + 1 {
        // A later copy already started: this one is obsolete and is dropped silently.
        return Ok(());
    }
    arboard::Clipboard::new()
        .map_err(|error| error.to_string())?
        .set()
        .exclude_from_history()
        .exclude_from_cloud()
        .image(image_data)
        .map_err(|error| error.to_string())
}

/// Replaces the pasteboard contents with plain text, excluded from Clipboard History and Cloud
/// Clipboard like every other copy Snapclone makes on Windows (see Security Considerations in
/// the Windows port plan: a screenshot can carry sensitive data even after auto-redaction).
#[cfg(target_os = "windows")]
pub fn write_text_to_pasteboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .map_err(|error| error.to_string())?
        .set()
        .exclude_from_history()
        .exclude_from_cloud()
        .text(text)
        .map_err(|error| error.to_string())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn write_text_to_pasteboard(_text: &str) -> Result<(), String> {
    Err("copying text is only implemented on macOS and Windows".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn write_png_to_pasteboard(_bytes: &[u8]) -> Result<(), String> {
    Err("copying images is only implemented on macOS and Windows".into())
}
