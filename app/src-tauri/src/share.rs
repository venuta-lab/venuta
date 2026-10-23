//! Sharing a link: its QR code, and Windows' own share panel (WhatsApp, Mail, Phone Link,
//! Nearby Share…), opened for the window it was asked from. Nothing is sent by Venuta: the app
//! the listener picks in the panel sends it.

/// The link as a QR code (an SVG), to scan with a phone.
#[tauri::command]
pub fn share_qr(text: String) -> String {
    crate::together::qr_svg(&text)
}

/// Windows' share panel for this window, with the link and its title. Desktop apps reach it
/// through `IDataTransferManagerInterop` on the window's own thread; the panel asks for the data
/// when it opens (`DataRequested`), and the last request's handler is let go at the next one.
#[tauri::command]
pub fn share_to_app(
    window: tauri::WebviewWindow,
    title: String,
    url: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        let w = window.clone();
        window
            .run_on_main_thread(move || {
                let _ = tx.send(windows_share::show(&w, &title, &url));
            })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = (window, title, url);
        Err("Sharing to apps is on Windows only.".into())
    }
}

#[cfg(windows)]
mod windows_share {
    use std::cell::RefCell;
    use windows::ApplicationModel::DataTransfer::{DataRequestedEventArgs, DataTransferManager};
    use windows::Foundation::{TypedEventHandler, Uri};
    use windows::Win32::UI::Shell::IDataTransferManagerInterop;
    use windows::core::{HSTRING, factory};

    thread_local! {
        /// The manager and its handler from the last share, let go at the next (the window's
        /// own thread: the only one that shares).
        static LAST: RefCell<Option<(DataTransferManager, i64)>> = const { RefCell::new(None) };
    }

    pub fn show(window: &tauri::WebviewWindow, title: &str, url: &str) -> Result<(), String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        let interop = factory::<DataTransferManager, IDataTransferManagerInterop>()
            .map_err(|e| e.message())?;
        let manager: DataTransferManager =
            unsafe { interop.GetForWindow(hwnd) }.map_err(|e| e.message())?;
        let (title, url) = (HSTRING::from(title), HSTRING::from(url));
        let handler = TypedEventHandler::<DataTransferManager, DataRequestedEventArgs>::new(
            move |_, args| {
                if let Some(args) = args.as_ref() {
                    let data = args.Request()?.Data()?;
                    data.Properties()?.SetTitle(&title)?;
                    data.SetWebLink(&Uri::CreateUri(&url)?)?;
                    data.SetText(&url)?;
                }
                Ok(())
            },
        );
        let token = manager.DataRequested(&handler).map_err(|e| e.message())?;
        let kept = manager.clone();
        LAST.with(|last| {
            if let Some((m, t)) = last.borrow_mut().replace((kept, token)) {
                let _ = m.RemoveDataRequested(t);
            }
        });
        unsafe { interop.ShowShareUIForWindow(hwnd) }.map_err(|e| e.message())
    }
}
