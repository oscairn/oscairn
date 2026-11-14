// SPDX-License-Identifier: AGPL-3.0-or-later
//! The screens (slint/), and the JUCE app's built-in profiles and themes, built in as text: the
//! UI reads the same files the JUCE app does, not copies of them.

use std::fmt::Write;
use std::path::Path;

fn main() {
    slint_build::compile("slint/shell.slint").expect("Slint build failed");
    let mut code = String::new();
    // Each built-in by its id: a profile's file name, a theme's folder name.
    for (name, dir, file) in [("PROFILES", "../hosts/standalone/profiles", ""), ("THEMES", "../hosts/standalone/themes", "theme.json")] {
        println!("cargo:rerun-if-changed={dir}");
        let mut found: Vec<(String, String)> = std::fs::read_dir(dir)
            .expect("the JUCE app's profiles and themes")
            .filter_map(|e| {
                let path = e.ok()?.path();
                let id = path.file_stem()?.to_str()?.to_string();
                let path = if file.is_empty() { path } else { path.join(file) };
                let path = std::fs::canonicalize(path).ok().filter(|p| p.extension().is_some_and(|x| x == "json"))?;
                Some((id, path.display().to_string()))
            })
            .collect();
        found.sort();
        writeln!(code, "pub const {name}: &[(&str, &str)] = &[").unwrap();
        for (id, path) in found {
            println!("cargo:rerun-if-changed={path}");
            writeln!(code, "    ({id:?}, include_str!({path:?})),").unwrap();
        }
        writeln!(code, "];").unwrap();
    }
    // The theme a device shows when neither the user nor its profile chose one.
    let default = std::fs::read_to_string("../hosts/standalone/themes/default").expect("themes/default");
    writeln!(code, "pub const DEFAULT_THEME: &str = {:?};", default.trim()).unwrap();
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("built_in.rs"), code).unwrap();
}
