// SPDX-License-Identifier: AGPL-3.0-or-later
//! The guard of the real-engine harnesses (ui/REAL-ENGINE.md, "How to run"): they refuse to start unless `HOME`,
//! `OSCAIRN_DATA_HOME` and `OSCAIRN_CONFIG_HOME` all lie inside one directory given for the run (`OSCAIRN_TEST_ROOT`), which
//! is not the user's real home (read from the password database, not from `$HOME`) nor a parent of it, and every delete
//! the harnesses make goes through `Root`, which refuses a path outside the root and one the harness did not make.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

/// The file a root that is not empty must carry: the harness wrote it in an empty directory.
pub const MARKER: &str = ".oscairn-test-root";

/// The user's home folder from the password database (`$HOME` may be anything), canonical.
pub fn real_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getpwuid returns null or a record that stays valid until the next call; its directory is read at once.
    let dir = unsafe {
        let record = libc::getpwuid(libc::getuid());
        if record.is_null() || (*record).pw_dir.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr((*record).pw_dir).to_bytes().to_vec()
    };
    std::fs::canonicalize(Path::new(std::ffi::OsStr::from_bytes(&dir))).ok()
}

/// `path` as it really is: absolute, without `..`, the longest part that exists made canonical (symlinks followed), the rest
/// appended. A path that is relative or has a `..` is refused.
pub fn resolve(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(format!("{} has a `..`", path.display()));
    }
    let mut rest = Vec::new();
    let mut at = path.to_path_buf();
    loop {
        match std::fs::canonicalize(&at) {
            Ok(real) => {
                return Ok(rest.iter().rev().fold(real, |p, part: &OsString| p.join(part)));
            }
            Err(_) => {
                let Some(name) = at.file_name().map(|n| n.to_os_string()) else { return Err(format!("{} cannot be resolved", path.display())) };
                rest.push(name);
                if !at.pop() {
                    return Err(format!("{} cannot be resolved", path.display()));
                }
            }
        }
    }
}

/// The directory a run is confined to, and what the harness made in it.
pub struct Root {
    dir: PathBuf,
    made: Mutex<Vec<PathBuf>>,
}

impl Root {
    /// From the environment of the run: `OSCAIRN_TEST_ROOT`, `HOME`, `OSCAIRN_DATA_HOME`, `OSCAIRN_CONFIG_HOME`.
    pub fn from_env() -> Result<Root, String> {
        let home = real_home().ok_or("the real home folder cannot be read from the password database")?;
        Root::check(|name| std::env::var_os(name), &home)
    }

    /// The same with the variables and the real home given.
    pub fn check(var: impl Fn(&str) -> Option<OsString>, real_home: &Path) -> Result<Root, String> {
        let root = var("OSCAIRN_TEST_ROOT").filter(|v| !v.is_empty()).ok_or("OSCAIRN_TEST_ROOT is not set: it names the one directory the run may use")?;
        let root = std::fs::canonicalize(&root).map_err(|e| format!("OSCAIRN_TEST_ROOT {root:?} does not exist: {e}"))?;
        if !root.is_dir() {
            return Err(format!("OSCAIRN_TEST_ROOT {} is not a directory", root.display()));
        }
        if real_home.starts_with(&root) {
            return Err(format!("OSCAIRN_TEST_ROOT {} is the real home folder {} or a parent of it", root.display(), real_home.display()));
        }
        // A checkout or a scratch folder under the home is fine; the user's own folders are not a place for a test's files.
        for own in ["Documents", "Desktop", "Downloads", "Music", "Pictures", "Movies", "Library", "Public"] {
            if root.starts_with(real_home.join(own)) {
                return Err(format!("OSCAIRN_TEST_ROOT {} is inside {}", root.display(), real_home.join(own).display()));
            }
        }
        let empty = std::fs::read_dir(&root).map_err(|e| e.to_string())?.next().is_none();
        if empty {
            std::fs::write(root.join(MARKER), b"made by the real-engine harness\n").map_err(|e| e.to_string())?;
        } else if !root.join(MARKER).is_file() {
            return Err(format!("OSCAIRN_TEST_ROOT {} is not empty and has no {MARKER}: it is not a folder of the harness", root.display()));
        }
        let root = Root { dir: root, made: Mutex::new(Vec::new()) };
        for name in ["HOME", "OSCAIRN_DATA_HOME", "OSCAIRN_CONFIG_HOME"] {
            let value = var(name).filter(|v| !v.is_empty()).ok_or_else(|| format!("{name} is not set: the run's folders must be inside OSCAIRN_TEST_ROOT"))?;
            let inside = root.inside(Path::new(&value)).map_err(|e| format!("{name}: {e}"))?;
            if real_home.starts_with(&inside) {
                return Err(format!("{name} {} is the real home folder or a part of it", inside.display()));
            }
        }
        Ok(root)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `path` resolved, if it is strictly inside the root.
    pub fn inside(&self, path: &Path) -> Result<PathBuf, String> {
        let real = resolve(path)?;
        if real == self.dir || !real.starts_with(&self.dir) {
            return Err(format!("{} is not inside {}", path.display(), self.dir.display()));
        }
        Ok(real)
    }

    fn note(&self, real: PathBuf) {
        self.made.lock().unwrap().push(real);
    }

    /// Makes a directory (and its parents) the harness will own: inside the root only.
    pub fn create_dir_all(&self, path: &Path) -> Result<(), String> {
        let real = self.inside(path)?;
        std::fs::create_dir_all(&real).map_err(|e| e.to_string())?;
        self.note(real);
        Ok(())
    }

    /// Writes a file the harness will own: inside the root only.
    pub fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        let real = self.inside(path)?;
        std::fs::write(&real, bytes).map_err(|e| e.to_string())?;
        self.note(real);
        Ok(())
    }

    /// Whether the harness made `real` or a folder holding it.
    fn owned(&self, real: &Path) -> bool {
        self.made.lock().unwrap().iter().any(|m| real.starts_with(m))
    }

    /// The one way a harness deletes a file: inside the root, and made by the harness (itself or in a folder it made).
    pub fn remove_file(&self, path: &Path) -> Result<(), String> {
        let real = self.inside(path)?;
        if !self.owned(&real) {
            return Err(format!("{} was not made by the harness", path.display()));
        }
        std::fs::remove_file(&real).map_err(|e| e.to_string())
    }

    /// The same for a folder and what is in it.
    pub fn remove_dir_all(&self, path: &Path) -> Result<(), String> {
        let real = self.inside(path)?;
        if !self.owned(&real) {
            return Err(format!("{} was not made by the harness", path.display()));
        }
        std::fs::remove_dir_all(&real).map_err(|e| e.to_string())
    }
}

/// The root of this run, or the run stops with the reason, before an engine is asked anything or a file touched.
pub fn root_or_stop() -> Root {
    Root::from_env().unwrap_or_else(|e| panic!("the real-engine harness refuses to run: {e} (ui/REAL-ENGINE.md, \"How to run\")"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oscairn-guard-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn env(pairs: &[(&str, &Path)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = pairs.iter().map(|(k, v)| (k.to_string(), v.as_os_str().to_owned())).collect();
        move |name| map.get(name).cloned()
    }

    fn good(root: &Path) -> impl Fn(&str) -> Option<OsString> {
        let (h, d, c) = (root.join("home"), root.join("data"), root.join("config"));
        let map: HashMap<String, OsString> = [("OSCAIRN_TEST_ROOT", root.to_owned()), ("HOME", h), ("OSCAIRN_DATA_HOME", d), ("OSCAIRN_CONFIG_HOME", c)]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.into_os_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn the_run_starts_with_every_folder_inside_the_root() {
        let (root, home) = (scratch("ok"), scratch("ok-home"));
        let r = Root::check(good(&root), &home).expect("a good run");
        assert!(root.join(MARKER).is_file(), "an empty root gets its marker");
        assert_eq!(r.dir(), root);
        // A second run in the same root (it has the marker).
        assert!(Root::check(good(&root), &home).is_ok());
    }

    #[test]
    fn the_run_refuses_a_missing_variable_the_real_home_and_a_root_that_is_not_the_harness() {
        let (root, home) = (scratch("bad"), scratch("bad-home"));
        for missing in ["OSCAIRN_TEST_ROOT", "HOME", "OSCAIRN_DATA_HOME", "OSCAIRN_CONFIG_HOME"] {
            let all = good(&root);
            let e = Root::check(|n| if n == missing { None } else { all(n) }, &home).err().unwrap_or_else(|| panic!("{missing} missing was accepted"));
            assert!(e.contains(missing), "{e}");
        }
        // A folder that is the real home, and one outside the root.
        let all = good(&root);
        let e = Root::check(|n| if n == "HOME" { Some(home.clone().into_os_string()) } else { all(n) }, &home).err().expect("the real home as HOME");
        assert!(e.contains("HOME"), "{e}");
        let all = good(&root);
        let e = Root::check(|n| if n == "OSCAIRN_DATA_HOME" { Some(std::env::temp_dir().into_os_string()) } else { all(n) }, &home).err().expect("a folder outside the root");
        assert!(e.contains("OSCAIRN_DATA_HOME"), "{e}");
        // A root that is the real home, or a parent of it.
        let parent = scratch("bad-parent");
        let inner = parent.join("me");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(parent.join(MARKER), b"x").unwrap();
        assert!(Root::check(env(&[("OSCAIRN_TEST_ROOT", &parent), ("HOME", &parent.join("h")), ("OSCAIRN_DATA_HOME", &parent.join("d")), ("OSCAIRN_CONFIG_HOME", &parent.join("c"))]), &inner).is_err(), "a parent of the home");
        // A root inside the user's Documents.
        let docs = home.join("Documents");
        std::fs::create_dir_all(&docs).unwrap();
        assert!(Root::check(good(&docs), &home).is_err(), "inside Documents");
        // A folder that is not empty and not the harness's.
        let other = scratch("bad-other");
        std::fs::write(other.join("keep.txt"), b"x").unwrap();
        assert!(Root::check(good(&other), &home).is_err(), "somebody's folder");
        // A variable that does not exist leaves the run without a root at all.
        assert!(Root::check(|_| None, &home).is_err());
    }

    #[test]
    fn the_real_home_is_read_from_the_password_database() {
        let home = real_home().expect("a home");
        assert!(home.is_absolute() && home.is_dir(), "{home:?}");
    }

    #[test]
    fn a_delete_outside_the_root_the_home_a_dotdot_or_a_link_out_is_refused() {
        let (root, outside) = (scratch("del"), scratch("del-outside"));
        let r = Root::check(good(&root), &outside).unwrap();
        let victim = outside.join("mine.olib");
        std::fs::write(&victim, b"keep").unwrap();
        assert!(r.remove_file(&victim).is_err(), "outside the root");
        assert!(r.remove_dir_all(&outside).is_err(), "a folder outside the root");
        assert!(r.remove_dir_all(&root).is_err(), "the root itself");
        assert!(r.remove_file(&real_home().unwrap().join("Documents").join("x.olib")).is_err(), "the real home");
        // `..` out of the root.
        r.create_dir_all(&root.join("work")).unwrap();
        assert!(r.remove_file(&root.join("work").join("..").join("..").join(victim.file_name().unwrap())).is_err());
        assert!(r.remove_file(&root.join("work/../../x")).is_err());
        // A link inside the root that leads out.
        std::os::unix::fs::symlink(&outside, root.join("work/link")).unwrap();
        assert!(r.remove_file(&root.join("work/link/mine.olib")).is_err(), "through a symlink");
        assert!(r.remove_dir_all(&root.join("work/link")).is_err(), "a symlink to a folder outside");
        // A relative path.
        assert!(r.remove_file(Path::new("mine.olib")).is_err());
        assert_eq!(std::fs::read(&victim).unwrap(), b"keep", "nothing outside was touched");
    }

    #[test]
    fn only_what_the_harness_made_is_deleted() {
        let (root, home) = (scratch("own"), scratch("own-home"));
        let r = Root::check(good(&root), &home).unwrap();
        let old = root.join("old.olib");
        std::fs::write(&old, b"from before").unwrap();
        assert!(r.remove_file(&old).is_err(), "a file in the root that the harness did not make");
        r.create_dir_all(&root.join("docs")).unwrap();
        r.write(&root.join("docs/a.olib"), b"a").unwrap();
        // A file put there by the engine is in a folder the harness made: it goes with it.
        std::fs::write(root.join("docs/b.olib"), b"b").unwrap();
        r.remove_file(&root.join("docs/a.olib")).unwrap();
        r.remove_dir_all(&root.join("docs")).unwrap();
        assert!(old.exists() && !root.join("docs").exists());
    }
}
