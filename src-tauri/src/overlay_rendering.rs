//! Keep the overlay window fully painted on WebKitGTK 2.50 and newer.
//!
//! WebKitGTK 2.50 turned on *damage propagation* by default: each frame tells
//! the desktop's compositor only which regions changed, and only those are
//! redrawn. The overlay is a transparent, always-on-top window, and on some
//! desktops (reported with WebKitGTK 2.54 on Arch) its unchanged regions are
//! then never redrawn: only the parts that animate every frame stay visible,
//! everything static vanishes, and the overlay looks broken up and flickering.
//!
//! WebKit exposes damage propagation as a per-page preference,
//! `PropagateDamagingInformation`, switchable through the public
//! `webkit_settings_set_feature_enabled` API (WebKitGTK 2.42+). This turns it
//! off for the overlay window only; Settings and the rest of the system are
//! untouched. The overlay repaints its whole (small) surface each frame, which
//! is what it did before 2.50.
//!
//! The feature API is looked up at runtime rather than linked, so VoxCtrl
//! still starts on WebKitGTK older than 2.42, where there is nothing to do.

/// The WebKit preference behind damage propagation.
pub const DAMAGE_FEATURE: &str = "PropagateDamagingInformation";

/// The plugin that applies this to the overlay window as it is created.
pub fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("overlay-rendering")
        .on_webview_ready(|webview| {
            if webview.label() == crate::window::OVERLAY_WINDOW {
                apply(&webview);
            }
        })
        .build()
}

#[cfg(target_os = "linux")]
fn apply<R: tauri::Runtime>(webview: &tauri::Webview<R>) {
    let result = webview.with_webview(|platform| {
        use gtk::glib::translate::ToGlibPtr;
        use webkit2gtk::WebViewExt;

        let view = platform.inner();
        let Some(settings) = WebViewExt::settings(&view) else {
            return;
        };
        let settings_ptr: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;
        // SAFETY: `settings_ptr` is a live WebKitSettings owned by `settings`,
        // which outlives the call; the feature API only reads and toggles it.
        match unsafe { ffi::disable_feature(settings_ptr.cast(), DAMAGE_FEATURE) } {
            Ok(true) => {
                // Push the change to the page and start it again so the
                // compositor is set up without damage propagation.
                view.set_settings(&settings);
                view.reload();
                tracing::info!("Overlay: WebKit damage propagation turned off for this window");
            }
            Ok(false) => {
                tracing::info!("Overlay: this WebKitGTK has no {DAMAGE_FEATURE} preference; nothing to change");
            }
            Err(missing) => {
                tracing::info!("Overlay: WebKitGTK feature API unavailable ({missing}); nothing to change");
            }
        }
    });
    if let Err(e) = result {
        tracing::warn!("Overlay: could not adjust WebKit rendering settings: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
fn apply<R: tauri::Runtime>(_webview: &tauri::Webview<R>) {}

/// Runtime lookup of WebKitGTK's feature API (2.42+), so an older WebKitGTK
/// without these symbols still loads VoxCtrl.
#[cfg(target_os = "linux")]
mod ffi {
    use std::ffi::{c_char, c_int, c_void, CStr, CString};

    type GetAllFeatures = unsafe extern "C" fn() -> *mut c_void;
    type ListLength = unsafe extern "C" fn(*mut c_void) -> usize;
    type ListGet = unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void;
    type ListUnref = unsafe extern "C" fn(*mut c_void);
    type Identifier = unsafe extern "C" fn(*mut c_void) -> *const c_char;
    type SetEnabled = unsafe extern "C" fn(*mut c_void, *mut c_void, c_int);

    unsafe fn symbol<T: Copy>(name: &str) -> Result<T, String> {
        let c_name = CString::new(name).map_err(|_| name.to_string())?;
        let ptr = libc::dlsym(libc::RTLD_DEFAULT, c_name.as_ptr());
        if ptr.is_null() {
            return Err(name.to_string());
        }
        // SAFETY: the caller names a symbol whose C signature matches `T`.
        Ok(std::mem::transmute_copy::<*mut c_void, T>(&ptr))
    }

    /// Turn off the WebKit feature `identifier` in `settings`. `Ok(true)` when
    /// it was found and disabled, `Ok(false)` when this WebKitGTK has no such
    /// feature, `Err(symbol)` when the feature API itself is missing.
    ///
    /// # Safety
    /// `settings` must point to a live `WebKitSettings`.
    pub unsafe fn disable_feature(settings: *mut c_void, identifier: &str) -> Result<bool, String> {
        let get_all: GetAllFeatures = symbol("webkit_settings_get_all_features")?;
        let len: ListLength = symbol("webkit_feature_list_get_length")?;
        let get: ListGet = symbol("webkit_feature_list_get")?;
        let unref: ListUnref = symbol("webkit_feature_list_unref")?;
        let ident: Identifier = symbol("webkit_feature_get_identifier")?;
        let set_enabled: SetEnabled = symbol("webkit_settings_set_feature_enabled")?;

        let list = get_all();
        if list.is_null() {
            return Ok(false);
        }
        let mut found = false;
        for i in 0..len(list) {
            let feature = get(list, i);
            if feature.is_null() {
                continue;
            }
            let name = ident(feature);
            if !name.is_null() && CStr::from_ptr(name).to_bytes() == identifier.as_bytes() {
                set_enabled(settings, feature, 0);
                found = true;
                break;
            }
        }
        unref(list);
        Ok(found)
    }
}
