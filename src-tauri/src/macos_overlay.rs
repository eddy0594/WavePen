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

/// Converts a tao window into a non-activating panel so it can appear over any
/// Space, including another app's full-screen Space, without switching Spaces
/// or activating WavePen.
unsafe fn make_overlay_panel(window: &mut AnyObject) {
    match overlay_panel_class() {
        Some(panel) if layouts_match(window.class(), panel) => {
            if !std::ptr::eq(window.class(), panel) {
                objc2::ffi::object_setClass(
                    window as *mut AnyObject as *mut _,
                    panel as *const AnyClass as *const _,
                );
            }
            let style: usize = msg_send![&*window, styleMask];
            let _: () = msg_send![&*window, setStyleMask: style | STYLE_NONACTIVATING_PANEL];
            let _: () = msg_send![&*window, setFloatingPanel: Bool::YES];
            let _: () = msg_send![&*window, setHidesOnDeactivate: Bool::NO];
        }
        _ => {
            tracing::warn!("Window layout differs from NSPanel; it may hide over full-screen apps")
        }
    }
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
    make_overlay_panel(window);
    let _: () = msg_send![&*window, setBecomesKeyOnlyIfNeeded: Bool::YES];
    let _: () = msg_send![
        &*window,
        setCollectionBehavior: COLLECTION_CAN_JOIN_ALL_SPACES
            | COLLECTION_STATIONARY
            | COLLECTION_IGNORES_CYCLE
            | COLLECTION_FULL_SCREEN_AUXILIARY
    ];
    let _: () = msg_send![&*window, setLevel: STATUS_WINDOW_LEVEL];
    log_window_state("capsule", window);
}

/// Orders the capsule in front on the Space the user is looking at.
///
/// `NSWindow.makeKeyAndOrderFront:` (what `show()` calls) can be ignored for a
/// non-activating panel of a background app, which left the capsule invisible
/// over full-screen apps. `orderFrontRegardless` has no such condition.
///
/// # Safety
/// `ns_window` must be a live `NSWindow` pointer and this must run on the main thread.
pub unsafe fn order_capsule_front(ns_window: *mut c_void) {
    let Some(window) = (ns_window as *mut AnyObject).as_ref() else {
        return;
    };
    let _: () = msg_send![window, orderFrontRegardless];
    log_window_state("capsule (ordered front)", window);
}

/// Logs the window class and the macOS flags that decide whether a window can
/// appear over another app's full-screen Space, so problems are visible in logs.
unsafe fn log_window_state(label: &str, window: &AnyObject) {
    let behavior: usize = msg_send![window, collectionBehavior];
    let style: usize = msg_send![window, styleMask];
    let level: isize = msg_send![window, level];
    tracing::info!(
        "{label} window: class={}, nonactivating_panel={}, can_join_all_spaces={}, full_screen_auxiliary={}, level={level}",
        window.class().name(),
        style & STYLE_NONACTIVATING_PANEL != 0,
        behavior & COLLECTION_CAN_JOIN_ALL_SPACES != 0,
        behavior & COLLECTION_FULL_SCREEN_AUXILIARY != 0,
    );
}

/// Shows the Ask answer window on the Space the user is currently on (including
/// full-screen apps) and gives it keyboard focus without activating WavePen,
/// which would otherwise switch back to the desktop Space.
///
/// # Safety
/// `ns_window` must be a live `NSWindow` pointer and this must run on the main thread.
pub unsafe fn show_ask_window(ns_window: *mut c_void) {
    let Some(window) = (ns_window as *mut AnyObject).as_mut() else {
        return;
    };
    make_overlay_panel(window);
    let _: () = msg_send![
        &*window,
        setCollectionBehavior: COLLECTION_CAN_JOIN_ALL_SPACES | COLLECTION_FULL_SCREEN_AUXILIARY
    ];
    let _: () = msg_send![&*window, setLevel: STATUS_WINDOW_LEVEL];
    let _: () = msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    log_window_state("ask", window);
}
