//! The text on the clipboard, for Paste in Venuta's own right-click menu on text fields. The
//! page can't read the clipboard itself (WebView2 asks for a permission nothing can give), so
//! the window asks Windows, only when Paste is chosen.

/// The clipboard's text, or empty when it holds none.
#[tauri::command]
pub fn clipboard_text() -> Result<String, String> {
    read()
}

#[cfg(windows)]
fn read() -> Result<String, String> {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    };
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};

    /// CF_UNICODETEXT: text as UTF-16, ending in a zero.
    const UNICODE_TEXT: u32 = 13;

    // SAFETY: the clipboard is opened, read through a locked handle that is only used while
    // locked, and closed again on every path.
    unsafe {
        if IsClipboardFormatAvailable(UNICODE_TEXT).is_err() {
            return Ok(String::new());
        }
        OpenClipboard(None).map_err(|e| format!("The clipboard is busy: {e}"))?;
        let text = (|| {
            let handle: HANDLE = GetClipboardData(UNICODE_TEXT).map_err(|e| e.to_string())?;
            let global = HGLOBAL(handle.0);
            let ptr = GlobalLock(global) as *const u16;
            if ptr.is_null() {
                return Ok(String::new());
            }
            // Never past the memory the clipboard holds, even if the text has no ending zero.
            let max = GlobalSize(global) / 2;
            let len = (0..max).take_while(|&i| *ptr.add(i) != 0).count();
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(global);
            Ok(text)
        })();
        let _ = CloseClipboard();
        text
    }
}

#[cfg(not(windows))]
fn read() -> Result<String, String> {
    Ok(String::new())
}
