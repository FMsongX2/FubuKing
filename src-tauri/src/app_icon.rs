// Modified by FubuKing from upstream Atlas (Apache-2.0).
//! The app icon, from the persisted `appIcon` setting.
//!
//! The selectable icons are listed in `icons/app-icons/app-icons.json`, and
//! rendered by `scripts/app-icons.mjs`. The manifest's default is the bundle's
//! own icon (`Icon.icns`, rendered from an SVG source). Every other icon ships
//! as a flat `icons/app-icons/<id>.icns` resource and is applied at runtime in
//! two places:
//!
//! - the Dock and app switcher while Atlas runs, via `NSApplication
//!   setApplicationIconImage:`;
//! - Finder, Launchpad and Spotlight, via `NSWorkspace setIcon:forFile:` on the
//!   `.app` itself, so the choice outlives the process. Arc does the same. It
//!   adds a custom-icon file to the bundle root, which `codesign --strict`
//!   flags but Gatekeeper accepts; an update replaces the bundle and drops it,
//!   so it is re-applied at every launch.
//!
//! The default resets both to `nil` rather than loading a copy of the icon,
//! so the Liquid Glass rendering stays the system's own. An id the manifest
//! does not know (a newer Atlas wrote it, or an icon was retired) falls back to
//! the default for this session; `config.toml` keeps what the user chose.

use serde::Deserialize;
use std::sync::{Mutex, OnceLock, PoisonError};
use tauri::AppHandle;

#[derive(Deserialize)]
struct Manifest {
    default: String,
    icons: Vec<ManifestIcon>,
}

#[derive(Deserialize)]
struct ManifestIcon {
    id: String,
}

fn manifest() -> &'static Manifest {
    static MANIFEST: OnceLock<Manifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../icons/app-icons/app-icons.json"))
            .expect("icons/app-icons/app-icons.json is valid (tests/app-icons.test.ts)")
    })
}

/// The id of the bundle's own icon.
pub fn default_id() -> &'static str {
    &manifest().default
}

/// An icon id names a file (`<id>.icns`), so it must be a plain id: a value
/// with a separator or `..` in it would name a path outside the icons dir.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The last icon applied this session — `Some(None)` is the default — so the
/// settings commit path, which runs on every change to any key, only touches
/// the bundle when the icon actually changed.
static APPLIED: Mutex<Option<Option<String>>> = Mutex::new(None);

/// Apply the icon `id`. Safe to call from any thread and on every settings
/// commit — AppKit work is dispatched to the main thread, and re-applying the
/// current icon is a no-op. No-op off macOS.
pub fn apply(app: &AppHandle, id: &str) {
    let custom = if id == default_id() {
        None
    } else if manifest().icons.iter().any(|icon| icon.id == id) {
        Some(id.to_string())
    } else {
        tracing::warn!(target: "atlas::app_icon", id, "unknown app icon; using the default");
        None
    };
    {
        let mut applied = APPLIED.lock().unwrap_or_else(PoisonError::into_inner);
        if applied.as_ref() == Some(&custom) {
            return;
        }
        *applied = Some(custom.clone());
    }

    #[cfg(target_os = "macos")]
    {
        use tauri::Manager;

        let path = match custom {
            None => None,
            Some(id) => match app.path().resource_dir() {
                Ok(dir) => Some(dir.join("icons/app-icons").join(format!("{id}.icns"))),
                Err(e) => {
                    tracing::warn!(target: "atlas::app_icon", "no resource dir: {e}");
                    return;
                }
            },
        };
        let _ = app.run_on_main_thread(move || macos::set_icon(path.as_deref()));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, custom);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::msg_send;
    use objc2::rc::autoreleasepool;
    use objc2::runtime::{AnyClass, AnyObject};
    use std::ffi::CString;
    use std::path::Path;

    /// Set the Dock icon and the bundle's Finder icon to the image at `path`,
    /// or back to the bundle's own icon when `path` is `None`. Main thread only.
    pub(super) fn set_icon(path: Option<&Path>) {
        autoreleasepool(|_| unsafe {
            let image = match path {
                Some(path) => {
                    let image = load_image(path);
                    if image.is_null() {
                        tracing::warn!(target: "atlas::app_icon", "could not load {}", path.display());
                        return;
                    }
                    image
                }
                None => std::ptr::null_mut(),
            };
            set_dock_icon(image);
            set_bundle_icon(image);
        });
    }

    unsafe fn ns_string(s: &str) -> *mut AnyObject {
        let (Some(class), Ok(c)) = (AnyClass::get(c"NSString"), CString::new(s)) else {
            return std::ptr::null_mut();
        };
        msg_send![class, stringWithUTF8String: c.as_ptr()]
    }

    /// An autoreleased `NSImage`, or null.
    unsafe fn load_image(path: &Path) -> *mut AnyObject {
        let Some(class) = AnyClass::get(c"NSImage") else {
            return std::ptr::null_mut();
        };
        let ns_path = ns_string(&path.to_string_lossy());
        if ns_path.is_null() {
            return std::ptr::null_mut();
        }
        let allocated: *mut AnyObject = msg_send![class, alloc];
        let image: *mut AnyObject = msg_send![allocated, initWithContentsOfFile: ns_path];
        if !image.is_null() {
            // `init…` returns +1; whoever keeps it retains it.
            let _: *mut AnyObject = msg_send![image, autorelease];
        }
        image
    }

    /// `-[NSApplication setApplicationIconImage:]`; `nil` is the bundle icon.
    unsafe fn set_dock_icon(image: *mut AnyObject) {
        let Some(class) = AnyClass::get(c"NSApplication") else {
            return;
        };
        let ns_app: *mut AnyObject = msg_send![class, sharedApplication];
        if !ns_app.is_null() {
            let _: () = msg_send![ns_app, setApplicationIconImage: image];
        }
    }

    /// `-[NSWorkspace setIcon:forFile:options:]` on the running `.app`.
    /// Skipped outside a bundle (`tauri dev` runs a bare binary) and, when
    /// resetting, if there is no custom icon to remove — so a default-icon
    /// user's bundle is never written to.
    unsafe fn set_bundle_icon(image: *mut AnyObject) {
        let (Some(bundle_class), Some(workspace_class)) =
            (AnyClass::get(c"NSBundle"), AnyClass::get(c"NSWorkspace"))
        else {
            return;
        };
        let bundle: *mut AnyObject = msg_send![bundle_class, mainBundle];
        if bundle.is_null() {
            return;
        }
        let ns_bundle_path: *mut AnyObject = msg_send![bundle, bundlePath];
        if ns_bundle_path.is_null() {
            return;
        }
        let utf8: *const std::ffi::c_char = msg_send![ns_bundle_path, UTF8String];
        if utf8.is_null() {
            return;
        }
        let bundle_path = std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned();
        if !bundle_path.ends_with(".app") {
            return;
        }
        // Finder keeps a bundle's custom icon in a file named "Icon\r".
        if image.is_null() && !Path::new(&bundle_path).join("Icon\r").exists() {
            return;
        }
        let workspace: *mut AnyObject = msg_send![workspace_class, sharedWorkspace];
        if workspace.is_null() {
            return;
        }
        let ok: bool =
            msg_send![workspace, setIcon: image, forFile: ns_bundle_path, options: 0usize];
        if !ok {
            // Read-only location (App Translocation, a mounted dmg) or no
            // write access: the Dock icon still applies for this session.
            tracing::warn!(target: "atlas::app_icon", bundle = %bundle_path, "could not set the Finder icon");
        }
    }
}
