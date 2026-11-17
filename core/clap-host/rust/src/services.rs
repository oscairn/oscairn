// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the process gives every plugin host: its accelerators (oscairn.hardware, RFC 0004: the hardware service, in
//! C++, sets its table) and the observer told of each activation before it allocates (RFC 0025: a worker's engine
//! refits its memory limit, or refuses). Both are set once, before any plugin loads.
use crate::bindings::oscairn_clap_hardware;
use oscairn_clap_sys::oscairn_memory_t;
use std::ffi::{c_char, c_void, CStr};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering::Acquire, Ordering::Release};
use std::sync::Mutex;

static HARDWARE: AtomicPtr<oscairn_clap_hardware> = AtomicPtr::new(null_mut());

/// The process's accelerators; None: no accelerators (count 0, every reservation refused as unavailable).
pub fn hardware() -> Option<&'static oscairn_clap_hardware> {
    // SAFETY: set_hardware's table, which its setter keeps until after the last plugin is unloaded.
    unsafe { HARDWARE.load(Acquire).as_ref() }
}
/// # Safety
/// `table` null or kept, unchanged, until after the last plugin is unloaded.
pub unsafe fn set_hardware(table: *const oscairn_clap_hardware) {
    HARDWARE.store(table.cast_mut(), Release);
}

/// Told of an activation with what the plugin declared (null: nothing): null to go on, else why it is refused.
pub type Activating = unsafe extern "C" fn(context: *mut c_void, declared: *const oscairn_memory_t) -> *const c_char;
#[derive(Clone, Copy)]
struct Observer(Activating, *mut c_void);
// SAFETY: the observer and its context are the setter's to make callable from the hosts' main threads.
unsafe impl Send for Observer {}
static OBSERVER: Mutex<Option<Observer>> = Mutex::new(None);

/// # Safety
/// `activating` callable with `context` from any host's main thread until it is replaced.
pub unsafe fn observe_activations(activating: Option<Activating>, context: *mut c_void) {
    *OBSERVER.lock().unwrap_or_else(|e| e.into_inner()) = activating.map(|f| Observer(f, context));
}
/// The observer's word on an activation: Ok to go on, else why it is refused.
pub fn activating(declared: Option<&oscairn_memory_t>) -> Result<(), String> {
    let observer = *OBSERVER.lock().unwrap_or_else(|e| e.into_inner()); // not held while it runs
    let Some(Observer(f, context)) = observer else { return Ok(()) };
    // SAFETY: the observer, as its setter promised; the declaration alive for the call.
    let refused = unsafe { f(context, declared.map_or(std::ptr::null(), |d| d)) };
    // SAFETY: the observer's NUL-terminated reason, valid until its next call.
    if refused.is_null() { Ok(()) } else { Err(unsafe { CStr::from_ptr(refused) }.to_string_lossy().into_owned()) }
}

/// # Safety
/// As set_hardware.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_set_hardware(table: *const oscairn_clap_hardware) {
    // SAFETY: the caller's.
    unsafe { set_hardware(table) }
}
/// # Safety
/// As observe_activations.
#[no_mangle]
pub unsafe extern "C" fn rust_clap_observe_activations(activating: Option<Activating>, context: *mut c_void) {
    // SAFETY: the caller's.
    unsafe { observe_activations(activating, context) }
}
