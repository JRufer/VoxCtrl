//! Windows clipboard: every `HGLOBAL` format, saved and restored.
//!
//! Formats whose handle is a GDI object (`CF_BITMAP`, `CF_PALETTE`, metafiles)
//! are skipped: they are not memory blocks that can be copied byte for byte,
//! and Windows synthesises them from the memory formats (`CF_DIB` →
//! `CF_BITMAP`) when asked, so restoring the memory formats restores them too.

use std::ffi::c_void;
use std::time::Duration;

use anyhow::{bail, Result};
use windows_sys::Win32::Foundation::{GlobalFree, HANDLE, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE};

const CF_UNICODETEXT: u32 = 13;
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_PALETTE: u32 = 9;
const CF_ENHMETAFILE: u32 = 14;
const CF_OWNERDISPLAY: u32 = 0x0080;
const CF_DSPBITMAP: u32 = 0x0082;
const CF_DSPMETAFILEPICT: u32 = 0x0083;
const CF_DSPENHMETAFILE: u32 = 0x008E;
const CF_PRIVATEFIRST: u32 = 0x0200;
const CF_GDIOBJLAST: u32 = 0x03FF;

/// Formats that are not plain memory blocks.
fn is_handle_format(f: u32) -> bool {
    matches!(
        f,
        CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | CF_ENHMETAFILE | CF_OWNERDISPLAY
            | CF_DSPBITMAP | CF_DSPMETAFILEPICT | CF_DSPENHMETAFILE
    ) || (CF_PRIVATEFIRST..=CF_GDIOBJLAST).contains(&f)
}

pub struct Snapshot {
    items: Vec<(u32, Vec<u8>)>,
}

impl Snapshot {
    pub fn format_count(&self) -> usize {
        self.items.len()
    }
}

/// A message-only window to own the clipboard: with a null owner,
/// `SetClipboardData` fails after `EmptyClipboard`.
struct Owner(HWND);

impl Owner {
    fn new() -> Result<Self> {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            bail!("could not create a clipboard owner window");
        }
        Ok(Owner(hwnd))
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.0) };
    }
}

/// Another process may hold the clipboard open for a moment.
struct Open;

impl Open {
    fn new(owner: HWND) -> Result<Self> {
        for _ in 0..40 {
            if unsafe { OpenClipboard(owner) } != 0 {
                return Ok(Open);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        bail!("the clipboard is held open by another application")
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe { CloseClipboard() };
    }
}

fn read_global(h: HANDLE) -> Option<Vec<u8>> {
    if h.is_null() {
        return None;
    }
    unsafe {
        let size = GlobalSize(h);
        if size == 0 {
            return None;
        }
        let p = GlobalLock(h) as *const u8;
        if p.is_null() {
            return None;
        }
        let data = std::slice::from_raw_parts(p, size).to_vec();
        GlobalUnlock(h);
        Some(data)
    }
}

/// Copy `data` into a fresh global block and hand it to the clipboard, which
/// takes ownership on success.
fn put(format: u32, data: &[u8]) -> Result<()> {
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1));
        if h.is_null() {
            bail!("out of memory placing clipboard data");
        }
        let p = GlobalLock(h) as *mut u8;
        if p.is_null() {
            GlobalFree(h);
            bail!("could not lock clipboard memory");
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        GlobalUnlock(h);
        if SetClipboardData(format, h as *mut c_void).is_null() {
            GlobalFree(h);
            bail!("SetClipboardData({format}) failed: {}", std::io::Error::last_os_error());
        }
    }
    Ok(())
}

pub fn snapshot() -> Result<Snapshot> {
    let owner = Owner::new()?;
    let _open = Open::new(owner.0)?;
    let mut items = Vec::new();
    let mut f = 0u32;
    loop {
        f = unsafe { EnumClipboardFormats(f) };
        if f == 0 {
            break;
        }
        if is_handle_format(f) {
            continue;
        }
        let h = unsafe { GetClipboardData(f) };
        if let Some(data) = read_global(h) {
            items.push((f, data));
        }
    }
    Ok(Snapshot { items })
}

/// The clipboard's Unicode text, if it holds any.
pub fn current_text() -> Option<String> {
    let owner = Owner::new().ok()?;
    let _open = Open::new(owner.0).ok()?;
    let h = unsafe { GetClipboardData(CF_UNICODETEXT) };
    let bytes = read_global(h)?;
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

pub struct Held {
    seq: u32,
}

impl Held {
    pub fn restore(&self, s: Snapshot) -> Result<()> {
        restore(s)
    }

    pub fn still_current(&self) -> bool {
        unsafe { GetClipboardSequenceNumber() == self.seq }
    }
    pub fn mark(&self) -> usize {
        0
    }
    pub fn tracks_reads(&self) -> bool {
        false
    }
    pub fn read_since(&self, _mark: usize) -> bool {
        false
    }
}

fn register(name: &str) -> u32 {
    let w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { RegisterClipboardFormatW(w.as_ptr()) }
}

pub fn set_text(text: &str) -> Result<Held> {
    let owner = Owner::new()?;
    {
        let _open = Open::new(owner.0)?;
        unsafe { EmptyClipboard() };

        let mut wide: Vec<u8> = Vec::with_capacity(text.len() * 2 + 2);
        for u in text.encode_utf16().chain(std::iter::once(0)) {
            wide.extend_from_slice(&u.to_le_bytes());
        }
        put(CF_UNICODETEXT, &wide)?;

        // Keep dictation out of Win+V history, the cloud clipboard, and
        // clipboard-monitoring software that honours the standard hints.
        let zero = 0u32.to_le_bytes();
        let _ = put(register("CanIncludeInClipboardHistory"), &zero);
        let _ = put(register("CanUploadToCloudClipboard"), &zero);
        let _ = put(register("ExcludeClipboardContentFromMonitorProcessing"), &zero);
    }
    // Read after closing, so it is the value the clipboard now holds.
    Ok(Held { seq: unsafe { GetClipboardSequenceNumber() } })
}

pub fn restore(s: Snapshot) -> Result<()> {
    let owner = Owner::new()?;
    let _open = Open::new(owner.0)?;
    unsafe { EmptyClipboard() };
    let mut last_err = None;
    for (format, data) in &s.items {
        if let Err(e) = put(*format, data) {
            last_err = Some(e);
        }
    }
    match last_err {
        // Restored what could be; report the first failure for the log.
        Some(e) if s.items.len() == 1 => Err(e),
        _ => Ok(()),
    }
}
