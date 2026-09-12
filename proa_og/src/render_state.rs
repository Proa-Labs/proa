// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Shared render-loop state and RAII guards for the WebKit renderers.
//!
//! Both [`crate::renderer`] (WebKitGTK) and [`crate::wpe_renderer`] (WPE) drive
//! a GLib main loop, connect a `load-changed` signal, and arm a timeout source.
//! The pointer that GLib hands back to the C callbacks is a `*mut c_void` that
//! both renderers cast to `*mut State`. Defining a single `#[repr(C)]` struct
//! here (instead of two look-alike structs declared independently at each site)
//! guarantees the connect site and the callback agree on the layout even under
//! `-Z randomize-layout`.

#![cfg(not(wpe_stub))]

use crate::ffi;
use std::os::raw::c_void;

/// State shared between the render driver and the GLib C callbacks.
///
/// `#[repr(C)]` is load-bearing: the same allocation is reinterpreted from a
/// raw `*mut c_void` inside `extern "C"` callbacks, so a stable, non-randomized
/// field layout is required for soundness.
#[repr(C)]
pub(crate) struct State {
    /// Loop the callbacks quit once a snapshot (or error, or timeout) lands.
    pub(crate) main_loop: *mut ffi::GMainLoop,
    /// Error message recorded by a callback, if any.
    pub(crate) error: Option<String>,
    /// The copied snapshot surface owned by this struct once captured.
    pub(crate) surface: *mut ffi::cairo_surface_t,
    /// Set by the timeout callback so the driver can distinguish a timeout from
    /// a snapshot failure and return [`crate::OgImageError::Timeout`].
    pub(crate) timed_out: bool,
}

impl State {
    pub(crate) fn new(main_loop: *mut ffi::GMainLoop) -> Self {
        Self {
            main_loop,
            error: None,
            surface: std::ptr::null_mut(),
            timed_out: false,
        }
    }
}

/// RAII guard that disconnects the `load-changed` handler and unrefs the main
/// loop on drop.
///
/// Connecting a signal handler whose `user_data` points at a stack local is
/// only sound while that stack frame is alive. Any early return between the
/// `g_signal_connect_data` call and an explicit disconnect (a `?` on a fallible
/// FFI input, or a panic) would otherwise leave a stale handler installed on
/// the persistent `WebView`, and the next render would fire it against freed
/// memory: a write-what-where use-after-free. This guard makes the disconnect
/// and unref unconditional, so no control-flow path can leak the handler.
pub(crate) struct SignalGuard {
    web_view: *mut c_void,
    handler_id: ffi::gulong,
    main_loop: *mut ffi::GMainLoop,
}

impl SignalGuard {
    /// Take ownership of the disconnect/unref responsibility.
    ///
    /// # Safety
    ///
    /// `web_view` must be the object the handler was connected to, `handler_id`
    /// the id returned by `g_signal_connect_data`, and `main_loop` a loop owned
    /// by the caller (one reference is consumed on drop).
    pub(crate) unsafe fn new(
        web_view: *mut c_void,
        handler_id: ffi::gulong,
        main_loop: *mut ffi::GMainLoop,
    ) -> Self {
        Self {
            web_view,
            handler_id,
            main_loop,
        }
    }
}

impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            ffi::g_signal_handler_disconnect(self.web_view, self.handler_id);
            ffi::g_main_loop_unref(self.main_loop);
        }
    }
}

/// RAII guard that removes a GLib timeout source on drop.
///
/// The timeout source must be removed on the normal completion path so it does
/// not later fire into a freed main loop. A guard guarantees removal on every
/// exit path, including early returns and panics.
pub(crate) struct TimeoutGuard {
    /// Source tag from `g_timeout_add_seconds`, or 0 if no source is armed.
    source_id: ffi::guint,
}

impl TimeoutGuard {
    /// Hold a timeout source tag for removal on drop.
    pub(crate) fn new(source_id: ffi::guint) -> Self {
        Self { source_id }
    }
}

impl Drop for TimeoutGuard {
    fn drop(&mut self) {
        if self.source_id != 0 {
            unsafe {
                ffi::g_source_remove(self.source_id);
            }
        }
    }
}

/// GLib timeout callback. Records a timeout, quits the loop, and removes itself
/// by returning `G_SOURCE_REMOVE` (0).
///
/// # Safety
///
/// `user_data` must point at a live [`State`] for the duration of the render,
/// which the [`SignalGuard`]/[`TimeoutGuard`] ordering guarantees: both guards
/// live in the same scope as the `State`, and the loop only returns once they
/// are about to drop.
pub(crate) unsafe extern "C" fn on_timeout(user_data: ffi::gpointer) -> ffi::gboolean {
    let state = &mut *(user_data as *mut State);
    state.timed_out = true;
    ffi::g_main_loop_quit(state.main_loop);
    0 // G_SOURCE_REMOVE: do not fire again
}
