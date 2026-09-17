//! Keeps WavePen's floating windows visible on every macOS Space, including
//! Spaces that belong to another app's full-screen window.
//!
//! A regular `NSWindow` owned by a normal (Dock) app is never drawn on top of
//! another app's full-screen Space, so the recording capsule silently vanished
//! when the user swiped away from the desktop. A non-activating `NSPanel` with
//! the full-screen-auxiliary collection behavior is what macOS expects for this
//! kind of HUD. Tauri/tao creates the window as its own `TaoWindow` subclass, so
//! the capsule is re-classed into an `NSPanel` subclass with the identical
//! `focusable` ivar layout that tao reads and writes.

use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::sel;
use std::ffi::c_void;
use std::sync::OnceLock;

const COLLECTION_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const COLLECTION_MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
const COLLECTION_STATIONARY: usize = 1 << 4;
const COLLECTION_IGNORES_CYCLE: usize = 1 << 6;
const COLLECTION_FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const STYLE_NONACTIVATING_PANEL: usize = 1 << 7;
/// `NSStatusWindowLevel`: above normal and floating windows, below menus.
const STATUS_WINDOW_LEVEL: isize = 25;

const TAO_FOCUSABLE_IVAR: &str = "focusable";
const PANEL_CLASS_NAME: &str = "WavePenOverlayPanel";

extern "C" fn overlay_panel_is_focusable(this: &AnyObject, _sel: Sel) -> Bool {
    this.class()
        .instance_variable(TAO_FOCUSABLE_IVAR)
        .map(|ivar| unsafe { *ivar.load::<Bool>(this) })
        .unwrap_or(Bool::NO)
}

fn overlay_panel_class() -> Option<&'static AnyClass> {
    static CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();
    *CLASS.get_or_init(|| {
        if let Some(existing) = AnyClass::get(PANEL_CLASS_NAME) {
            return Some(existing);
        }
        let mut builder = ClassBuilder::new(PANEL_CLASS_NAME, AnyClass::get("NSPanel")?)?;
        builder.add_ivar::<Bool>(TAO_FOCUSABLE_IVAR);
        unsafe {
            builder.add_method(
                sel!(canBecomeKeyWindow),
                overlay_panel_is_focusable as extern "C" fn(_, _) -> _,
            );
            builder.add_method(
                sel!(canBecomeMainWindow),
                overlay_panel_is_focusable as extern "C" fn(_, _) -> _,
            );
        }
        Some(builder.register())
    })
}

/// Re-classing is only safe when the panel subclass has exactly the same
/// instance layout as the window's current class.
fn layouts_match(current: &AnyClass, panel: &AnyClass) -> bool {
    let offset = |class: &AnyClass| {
        class
            .instance_variable(TAO_FOCUSABLE_IVAR)
            .map(|ivar| ivar.offset())
    };
    current.instance_size() == panel.instance_size()
        && offset(current).is_some()
        && offset(current) == offset(panel)
}

/// Turns the recording capsule into a non-activating HUD panel that follows
/// the user onto every Space, including full-screen apps.
///
/// # Safety
/// `ns_window` must be a live `NSWindow` pointer and this must run on the main thread.
pub unsafe fn configure_capsule_window(ns_window: *mut c_void) {
    let Some(window) = (ns_window as *mut AnyObject).as_mut() else {
        return;
    };

    match overlay_panel_class() {
        Some(panel) if layouts_match(window.class(), panel) => {
            objc2::ffi::object_setClass(
                window as *mut AnyObject as *mut _,
                panel as *const AnyClass as *const _,
            );
            let style: usize = msg_send![&*window, styleMask];
            let _: () = msg_send![&*window, setStyleMask: style | STYLE_NONACTIVATING_PANEL];
            let _: () = msg_send![&*window, setFloatingPanel: Bool::YES];
            let _: () = msg_send![&*window, setHidesOnDeactivate: Bool::NO];
            let _: () = msg_send![&*window, setBecomesKeyOnlyIfNeeded: Bool::YES];
        }
        _ => tracing::warn!(
            "Capsule window layout differs from NSPanel; it may hide over full-screen apps"
        ),
    }

    let _: () = msg_send![
        &*window,
        setCollectionBehavior: COLLECTION_CAN_JOIN_ALL_SPACES
            | COLLECTION_STATIONARY
            | COLLECTION_IGNORES_CYCLE
            | COLLECTION_FULL_SCREEN_AUXILIARY
    ];
    let _: () = msg_send![&*window, setLevel: STATUS_WINDOW_LEVEL];
}

/// Lets the Ask answer window open on the Space the user is currently on,
/// including another app's full-screen Space, instead of switching Spaces.
///
/// # Safety
/// `ns_window` must be a live `NSWindow` pointer and this must run on the main thread.
pub unsafe fn configure_ask_window(ns_window: *mut c_void) {
    let Some(window) = (ns_window as *mut AnyObject).as_ref() else {
        return;
    };
    let _: () = msg_send![
        window,
        setCollectionBehavior: COLLECTION_MOVE_TO_ACTIVE_SPACE | COLLECTION_FULL_SCREEN_AUXILIARY
    ];
    let _: () = msg_send![window, setLevel: STATUS_WINDOW_LEVEL];
}
