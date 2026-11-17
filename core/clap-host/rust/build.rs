// SPDX-License-Identifier: AGPL-3.0-or-later
//
// OSCAIRN_TSAN=1 (CMake sets it in a ThreadSanitizer build): the host tells ThreadSanitizer of its gate's handovers
// (host.rs, mod tsan), which its uninstrumented atomics do not show it.
fn main() {
    println!("cargo::rerun-if-env-changed=OSCAIRN_TSAN");
    println!("cargo::rustc-check-cfg=cfg(oscairn_tsan)");
    if std::env::var("OSCAIRN_TSAN").as_deref() == Ok("1") {
        println!("cargo::rustc-cfg=oscairn_tsan");
    }
}
