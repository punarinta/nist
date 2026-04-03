/// Wakes the main SDL event loop when PTY data arrives.
///
/// Without this, `wait_event_timeout` would sleep for up to 500ms (cursor blink interval)
/// even when a background process writes output, causing visible latency.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    OnceLock,
};

use sdl3::event::{Event, EventSender};

struct PtyWaker {
    sender: EventSender,
    event_type: u32,
    /// True while there is already an unprocessed User event in the SDL queue.
    pending: AtomicBool,
}

// EventSender is `{ _priv: () }` – trivially Send.
// PtyWaker therefore also satisfies Send + Sync.
unsafe impl Send for PtyWaker {}
unsafe impl Sync for PtyWaker {}

static PTY_WAKER: OnceLock<PtyWaker> = OnceLock::new();

/// Call once from the main thread after SDL is initialised.
pub fn init(sender: EventSender, event_type: u32) {
    let _ = PTY_WAKER.set(PtyWaker {
        sender,
        event_type,
        pending: AtomicBool::new(false),
    });
}

/// Push a User event so that `wait_event_timeout` returns immediately.
/// Safe to call from any thread. Coalesces multiple calls into one SDL event.
pub fn wake() {
    let Some(waker) = PTY_WAKER.get() else { return };
    if waker.event_type == 0 { return }

    // Avoid flooding the SDL event queue: only push one event while the
    // previous one hasn't been consumed yet.
    if waker.pending.swap(true, Ordering::Relaxed) {
        return; // event already queued
    }

    let _ = waker.sender.push_event(Event::User {
        timestamp: 0,
        window_id: 0,
        type_: waker.event_type,
        code: 0,
        data1: std::ptr::null_mut(),
        data2: std::ptr::null_mut(),
    });
}

/// Call from the main loop after waking to allow the next PTY write to push again.
pub fn acknowledge() {
    if let Some(waker) = PTY_WAKER.get() {
        waker.pending.store(false, Ordering::Relaxed);
    }
}
