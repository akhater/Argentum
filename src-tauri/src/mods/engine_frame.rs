//! How a rendered frame reaches the new interface.
//!
//! A frame crosses as one block: a 12-byte header - ticket, width, height,
//! each a little-endian u32 - then the RGBA rows. `src/shell/engine/tauri.ts`
//! reads it, and `src/shell/engine/wire.fixtures.json` holds one block that
//! both sides' tests check.
//!
//! On Windows the block goes into a WebView2 shared buffer posted to the page.
//! Tauri's IPC hands raw bytes to WebView2 through its custom-protocol bridge
//! at about 65 MB/s: 108 ms for a 1600 x 1067 frame, 223 ms at 2560 x 1440.
//! A fresh shared buffer arrives in 5 and 9.5 ms. (Measured 2026-10-18 on
//! WebView2 with a stand-alone wry page, both ways side by side.) Elsewhere,
//! or if WebView2 refuses the buffer, the block is the IPC answer.

use std::sync::Arc;

use argentum_engine::engine::Frame;
use tauri::WebviewWindow;
use tauri::ipc::Response;

/// Ticket, width, height.
pub const HEADER_BYTES: usize = 12;

/// What `engine_frame` answers.
pub enum Delivery {
    /// A newer frame has replaced the one asked for.
    Replaced,
    /// The block, as the answer itself.
    Bytes(Vec<u8>),
    /// The block went to the page as a shared buffer.
    Shared,
}

impl Delivery {
    /// The IPC answer: JSON `null`, the bytes, or JSON `"shared"`.
    pub fn into_response(self) -> Response {
        match self {
            Delivery::Replaced => Response::new("null".to_string()),
            Delivery::Bytes(bytes) => Response::new(bytes),
            Delivery::Shared => Response::new("\"shared\"".to_string()),
        }
    }
}

/// Send `frame` to `window`'s page the fastest way it takes.
pub async fn deliver(window: &WebviewWindow, frame: Option<Arc<Frame>>) -> Delivery {
    let Some(frame) = frame else {
        return Delivery::Replaced;
    };
    #[cfg(windows)]
    match shared::post(window, &frame).await {
        Ok(()) => return Delivery::Shared,
        Err(e) => log::warn!("[engine] frame not shared, sent as bytes: {e}"),
    }
    #[cfg(not(windows))]
    let _ = window;
    Delivery::Bytes(block(&frame))
}

/// The block's size in bytes.
pub fn block_bytes(frame: &Frame) -> usize {
    HEADER_BYTES + frame.rgba.len()
}

/// The block, in a new Vec.
pub fn block(frame: &Frame) -> Vec<u8> {
    let mut out = vec![0; block_bytes(frame)];
    write_block(frame, &mut out);
    out
}

/// Write the block into `out`, which is `block_bytes(frame)` long.
fn write_block(frame: &Frame, out: &mut [u8]) {
    let (head, pixels) = out.split_at_mut(HEADER_BYTES);
    for (at, v) in [frame.ticket.0, frame.width, frame.height]
        .into_iter()
        .enumerate()
    {
        head[at * 4..at * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    pixels.copy_from_slice(&frame.rgba);
}

/// WebView2 objects live on the window's thread, so that thread makes the
/// buffer and posts it; the copy into it, most of the time, happens off it.
#[cfg(windows)]
mod shared {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use argentum_engine::engine::Frame;
    use tauri::WebviewWindow;
    use tauri::webview::PlatformWebview;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY, ICoreWebView2_17, ICoreWebView2Environment12,
        ICoreWebView2SharedBuffer,
    };
    use windows::core::{HSTRING, Interface, PCWSTR};

    thread_local! {
        /// Buffers being filled, on the window's thread.
        static FILLING: RefCell<HashMap<u64, ICoreWebView2SharedBuffer>> = RefCell::default();
    }

    static NEXT: AtomicU64 = AtomicU64::new(1);

    /// Run `job` on the window's thread and wait for its answer.
    async fn on_window_thread<T: Send + 'static>(
        window: &WebviewWindow,
        job: impl FnOnce(PlatformWebview) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |webview| {
                let _ = tx.send(job(webview));
            })
            .map_err(|e| e.to_string())?;
        rx.await.map_err(|e| e.to_string())?
    }

    pub async fn post(window: &WebviewWindow, frame: &Arc<Frame>) -> Result<(), String> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let size = super::block_bytes(frame);
        let ticket = frame.ticket.0;

        let start = on_window_thread(window, move |webview| {
            // SAFETY: COM calls on the thread the WebView2 objects belong to.
            unsafe {
                let environment: ICoreWebView2Environment12 =
                    webview.environment().cast().map_err(|e| e.to_string())?;
                let buffer = environment
                    .CreateSharedBuffer(size as u64)
                    .map_err(|e| e.to_string())?;
                let mut start = std::ptr::null_mut();
                buffer.Buffer(&mut start).map_err(|e| e.to_string())?;
                FILLING.with(|f| f.borrow_mut().insert(id, buffer));
                Ok(start as usize)
            }
        })
        .await?;

        let source = frame.clone();
        let copied = tokio::task::spawn_blocking(move || {
            // SAFETY: `start` is the first of the buffer's `size` bytes, mapped
            // until the buffer is closed - below, after this copy, by the
            // window's thread, which holds the only reference meanwhile.
            let out = unsafe { std::slice::from_raw_parts_mut(start as *mut u8, size) };
            super::write_block(&source, out);
        })
        .await
        .map_err(|e| e.to_string());

        on_window_thread(window, move |webview| {
            let Some(buffer) = FILLING.with(|f| f.borrow_mut().remove(&id)) else {
                return Err("the shared buffer went missing".into());
            };
            // SAFETY: as above.
            let posted = copied.and_then(|()| unsafe {
                let core: ICoreWebView2_17 = webview
                    .controller()
                    .CoreWebView2()
                    .and_then(|c| c.cast())
                    .map_err(|e| e.to_string())?;
                let about = HSTRING::from(format!("{{\"agFrame\":{ticket}}}"));
                core.PostSharedBufferToScript(
                    &buffer,
                    COREWEBVIEW2_SHARED_BUFFER_ACCESS_READ_ONLY,
                    PCWSTR(about.as_ptr()),
                )
                .map_err(|e| e.to_string())
            });
            // The page keeps its own reference; this side's goes now.
            // SAFETY: as above.
            let _ = unsafe { buffer.Close() };
            posted
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argentum_engine::contract::RenderTicket;

    /// The block the interface's client reads (src/shell/engine/wire.fixtures.json,
    /// whose own test reads the same bytes back).
    #[test]
    fn a_frame_is_a_header_then_its_pixels() {
        let wire: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/shell/engine/wire.fixtures.json"))
                .unwrap();
        let f = &wire["frame"];
        let n = |k: &str| f[k].as_u64().unwrap() as u32;
        let want: Vec<u8> = serde_json::from_value(f["bytes"].clone()).unwrap();
        let frame = Frame {
            ticket: RenderTicket(n("ticket")),
            width: n("width"),
            height: n("height"),
            rgba: want[HEADER_BYTES..].to_vec(),
        };
        assert_eq!(block(&frame), want);
        assert_eq!(
            block_bytes(&frame),
            HEADER_BYTES + (n("width") * n("height") * 4) as usize
        );
    }
}
