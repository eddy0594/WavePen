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

use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use std::ffi::c_void;
use std::sync::OnceLock;

const COLLECTION_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const COLLECTION_MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
const COLLECTION_STATIONARY: usize = 1 << 4;
const COLLECTION_IGNORES_CYCLE: usize = 1 << 6;
const COLLECTION_FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const CAPSULE_COLLECTION_BEHAVIOR: usize = COLLECTION_CAN_JOIN_ALL_SPACES
    | COLLECTION_STATIONARY
    | COLLECTION_IGNORES_CYCLE
    | COLLECTION_FULL_SCREEN_AUXILIARY;
const ASK_COLLECTION_BEHAVIOR: usize =
    COLLECTION_CAN_JOIN_ALL_SPACES | COLLECTION_FULL_SCREEN_AUXILIARY;
const STYLE_NONACTIVATING_PANEL: usize = 1 << 7;
/// `NSStatusWindowLevel`: above normal and floating windows, below menus.
const STATUS_WINDOW_LEVEL: isize = 25;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    origin: Point,
    size: Size,
}

unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}

unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}

unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

impl Rect {
    fn contains(&self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x < self.origin.x + self.size.width
            && point.y >= self.origin.y
            && point.y < self.origin.y + self.size.height
    }
}

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
    apply_capsule_panel_setup(window);
    log_window_state("capsule", window);
}

unsafe fn apply_capsule_panel_setup(window: &mut AnyObject) {
    make_overlay_panel(window);
    let _: () = msg_send![&*window, setBecomesKeyOnlyIfNeeded: Bool::YES];
    let _: () = msg_send![&*window, setCollectionBehavior: CAPSULE_COLLECTION_BEHAVIOR];
    let _: () = msg_send![&*window, setLevel: STATUS_WINDOW_LEVEL];
}

/// Orders the capsule in front on the Space and display the user is looking at.
///
/// `NSWindow.makeKeyAndOrderFront:` (what `show()` calls) can be ignored for a
/// non-activating panel of a background app, so this uses `orderFrontRegardless`.
/// It also re-applies the panel setup (so nothing that reset the window can
/// keep it off full-screen Spaces), moves the capsule to the display under the
/// pointer (a full-screen app on another display would otherwise never show
/// it), and moves it onto the active Space if macOS still left it elsewhere.
///
/// # Safety
/// `ns_window` must be a live `NSWindow` pointer and this must run on the main thread.
pub unsafe fn order_capsule_front(ns_window: *mut c_void) {
    let Some(window) = (ns_window as *mut AnyObject).as_mut() else {
        return;
    };
    apply_capsule_panel_setup(window);
    move_to_pointer_screen(window);
    let _: () = msg_send![&*window, orderFrontRegardless];
    ensure_on_active_space("capsule", window, CAPSULE_COLLECTION_BEHAVIOR);
    log_window_state("capsule (ordered front)", window);
}

/// Moves a just-ordered-front window onto the active Space when macOS left it
/// on other Spaces only.
///
/// WindowServer keeps a per-window list of Spaces even for an all-Spaces
/// window, and that list can stop covering the Space the user is on. A WavePen
/// that had run for days still had the all-Spaces flag on its capsule, but the
/// list held only Desktop 1 while the user was in a full-screen app, so every
/// `orderFrontRegardless` drew it on Desktop 1, and ordering it out and in again
/// kept the same list. On a test panel stranded the same way, switching the
/// visible window to move-to-active-Space, ordering it front, and restoring
/// `behavior` put it on the active Space and back on every Space (8/8 runs).
///
/// AppKit hands these changes to WindowServer when the current Core Animation
/// transaction commits, so each step is flushed; without that the two behavior
/// changes merge into none and the window stays where it was. The move lands
/// asynchronously: `isOnActiveSpace` turns true within ~50 ms, so the next
/// "ordered front" log line is the one that confirms it.
unsafe fn ensure_on_active_space(label: &str, window: &AnyObject, behavior: usize) {
    let on_active_space: bool = msg_send![window, isOnActiveSpace];
    if on_active_space {
        return;
    }
    tracing::warn!("{label} was not on the active Space after ordering front; moving it there");
    let _: () = msg_send![
        window,
        setCollectionBehavior: COLLECTION_MOVE_TO_ACTIVE_SPACE
            | COLLECTION_FULL_SCREEN_AUXILIARY
            | COLLECTION_IGNORES_CYCLE
    ];
    let _: () = msg_send![window, orderFrontRegardless];
    flush_window_changes();
    let _: () = msg_send![window, setCollectionBehavior: behavior];
    flush_window_changes();
}

/// Commits the pending Core Animation transaction, which is when AppKit sends
/// window ordering and collection-behavior changes to WindowServer.
unsafe fn flush_window_changes() {
    if let Some(transaction) = AnyClass::get("CATransaction") {
        let _: () = msg_send![transaction, flush];
    }
}

/// Frame of the screen that contains the mouse pointer, in Cocoa coordinates.
unsafe fn pointer_screen_frame() -> Option<Rect> {
    let pointer: Point = msg_send![class!(NSEvent), mouseLocation];
    let screens: *mut AnyObject = msg_send![class!(NSScreen), screens];
    let screens = screens.as_ref()?;
    let count: usize = msg_send![screens, count];
    (0..count).find_map(|index| {
        let screen: *mut AnyObject = msg_send![screens, objectAtIndex: index];
        let frame: Rect = msg_send![&*screen, frame];
        frame.contains(pointer).then_some(frame)
    })
}

unsafe fn window_screen_frame(window: &AnyObject) -> Option<Rect> {
    let screen: *mut AnyObject = msg_send![window, screen];
    let screen = screen.as_ref()?;
    Some(msg_send![screen, frame])
}

/// Where a window at `frame` on `from` lands on `to`: same offset from the
/// screen's bottom-left corner, kept fully inside `to`.
fn origin_on_screen(frame: Rect, from: Rect, to: Rect) -> Point {
    let clamp = |value: f64, min: f64, max: f64| value.max(min).min(max.max(min));
    Point {
        x: clamp(
            to.origin.x + (frame.origin.x - from.origin.x),
            to.origin.x,
            to.origin.x + to.size.width - frame.size.width,
        ),
        y: clamp(
            to.origin.y + (frame.origin.y - from.origin.y),
            to.origin.y,
            to.origin.y + to.size.height - frame.size.height,
        ),
    }
}

unsafe fn move_to_pointer_screen(window: &mut AnyObject) {
    let Some(target) = pointer_screen_frame() else {
        return;
    };
    let frame: Rect = msg_send![&*window, frame];
    let current = window_screen_frame(window).unwrap_or(target);
    if current == target && target.contains(frame.origin) {
        return;
    }
    let origin = origin_on_screen(frame, current, target);
    let _: () = msg_send![&*window, setFrameOrigin: origin];
    tracing::info!(
        "Moved capsule to the display under the pointer: ({:.0}, {:.0}) -> ({:.0}, {:.0})",
        frame.origin.x,
        frame.origin.y,
        origin.x,
        origin.y
    );
}

/// Logs the window class and the macOS flags that decide whether a window can
/// appear over another app's full-screen Space, so problems are visible in logs.
unsafe fn log_window_state(label: &str, window: &AnyObject) {
    let behavior: usize = msg_send![window, collectionBehavior];
    let style: usize = msg_send![window, styleMask];
    let level: isize = msg_send![window, level];
    let visible: bool = msg_send![window, isVisible];
    let on_active_space: bool = msg_send![window, isOnActiveSpace];
    let frame: Rect = msg_send![window, frame];
    let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
    let activation_policy: isize = msg_send![&*app, activationPolicy];
    tracing::info!(
        "{label} window: class={}, nonactivating_panel={}, can_join_all_spaces={}, full_screen_auxiliary={}, level={level}, visible={visible}, on_active_space={on_active_space}, frame=({:.0}, {:.0}, {:.0}x{:.0}), screen={:?}, pointer_screen={:?}, activation_policy={activation_policy}",
        window.class().name(),
        style & STYLE_NONACTIVATING_PANEL != 0,
        behavior & COLLECTION_CAN_JOIN_ALL_SPACES != 0,
        behavior & COLLECTION_FULL_SCREEN_AUXILIARY != 0,
        frame.origin.x,
        frame.origin.y,
        frame.size.width,
        frame.size.height,
        window_screen_frame(window).map(|r| (r.origin.x, r.origin.y, r.size.width, r.size.height)),
        pointer_screen_frame().map(|r| (r.origin.x, r.origin.y, r.size.width, r.size.height)),
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
    let _: () = msg_send![&*window, setCollectionBehavior: ASK_COLLECTION_BEHAVIOR];
    let _: () = msg_send![&*window, setLevel: STATUS_WINDOW_LEVEL];
    let _: () = msg_send![&*window, makeKeyAndOrderFront: std::ptr::null::<AnyObject>()];
    ensure_on_active_space("ask", window, ASK_COLLECTION_BEHAVIOR);
    log_window_state("ask", window);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            origin: Point { x, y },
            size: Size { width, height },
        }
    }

    #[test]
    fn capsule_keeps_its_offset_when_moving_to_another_display() {
        let laptop = rect(0.0, 0.0, 1470.0, 956.0);
        let external = rect(1470.0, 0.0, 1920.0, 1080.0);
        let capsule = rect(623.0, 80.0, 224.0, 60.0);

        assert_eq!(
            origin_on_screen(capsule, laptop, external),
            Point { x: 2093.0, y: 80.0 }
        );
    }

    #[test]
    fn capsule_stays_inside_a_smaller_display() {
        let external = rect(0.0, 0.0, 1920.0, 1080.0);
        let laptop = rect(-1470.0, 0.0, 1470.0, 956.0);
        let capsule = rect(1800.0, 1000.0, 224.0, 60.0);

        assert_eq!(
            origin_on_screen(capsule, external, laptop),
            Point { x: -224.0, y: 896.0 }
        );
    }
}
