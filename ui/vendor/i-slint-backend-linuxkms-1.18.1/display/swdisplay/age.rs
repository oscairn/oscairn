// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Oscairn's: how old the display's two buffers are, in frames. No dependency, so the UI's tests
//! (ui/src/tests.rs) take this file as it is and render by it on any computer.

/// After a present: `drawn`, the buffer just sent to the display, holds the newest frame (1);
/// `next`, the one that was shown and is drawn into next, is a frame older (0: never drawn).
/// A buffer of age 2 is the software renderer's `SwappedBuffers`: it repaints what this frame
/// and the one before changed. Upstream's three buffers came back at age 3, which the renderer
/// has no memory for, so every frame was painted whole.
pub fn presented(drawn: &mut u8, next: &mut u8) {
    *drawn = 1;
    if *next != 0 {
        *next = next.saturating_add(1);
    }
}
