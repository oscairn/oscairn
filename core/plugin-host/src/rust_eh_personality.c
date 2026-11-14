/* SPDX-License-Identifier: AGPL-3.0-or-later */
/* samplepack's static library carries Rust's compiler_builtins, whose objects name
 * rust_eh_personality although the library is built panic=abort and never unwinds. A program
 * that takes a builtin from it (the JUCE engine's ___udivti3) then fails to link. Weak, so a
 * samplepack that defines the symbol itself wins.
 * shortcut: a stub on the host's side; remove once samplepack's C library defines it. */
__attribute__((weak)) void rust_eh_personality(void) {}
