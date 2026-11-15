// SPDX-License-Identifier: AGPL-3.0-or-later
//! StateFiles.h (core/plugin-host/src): a state bundle made in a private folder beside its destination and renamed
//! into place whole, and size-limited reads; failures worded as the C++ throws them.
use crate::host::Failure;
use std::ffi::{c_char, c_int, CString};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

extern "C" {
    fn mkdtemp(template: *mut c_char) -> *mut c_char;
    fn strerror(n: c_int) -> *const c_char;
}

/// std::filesystem's filesystem_error text for a call on paths (libc++'s on macOS, libstdc++'s elsewhere).
pub fn fs_error(call: &str, e: &std::io::Error, paths: &[&Path]) -> Failure {
    let what = match e.raw_os_error() {
        // SAFETY: strerror's string, copied at once.
        Some(n) => unsafe { std::ffi::CStr::from_ptr(strerror(n)) }.to_string_lossy().into_owned(),
        None => e.to_string(),
    };
    let message = if cfg!(target_os = "macos") {
        let quoted: String = paths.iter().map(|p| format!(" [\"{}\"]", p.display())).collect();
        format!("filesystem error: in {call}: {what}{quoted}")
    } else {
        // shortcut: libstdc++'s wording, unchecked on Linux yet.
        let bracketed: String = paths.iter().map(|p| format!(" [{}]", p.display())).collect();
        format!("filesystem error: {call}: {what}{bracketed}")
    };
    Failure::runtime(message)
}

/// std::filesystem::canonical.
pub fn canonical(path: &Path) -> Result<PathBuf, Failure> {
    std::fs::canonicalize(path).map_err(|e| fs_error("canonical", &e, &[path, Path::new("")]))
}

/// fs::absolute(path).lexically_normal().
fn normal(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() { path.to_path_buf() } else { std::env::current_dir().unwrap_or_default().join(path) };
    let mut out = PathBuf::new();
    for c in absolute.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// A state bundle: written into `temporary`, published to its destination whole.
pub struct Bundle {
    destination: PathBuf,
    pub temporary: PathBuf,
}
impl Bundle {
    pub fn new(path: &str) -> Result<Bundle, Failure> {
        if path.is_empty() {
            return Err(Failure::invalid("Invalid state directory"));
        }
        let destination = normal(Path::new(path));
        if destination.exists() || destination.is_symlink() {
            return Err(Failure::runtime(format!("State destination already exists: {}", destination.display())));
        }
        let parent = destination.parent().unwrap_or(Path::new("/")).to_path_buf();
        std::fs::create_dir_all(&parent).map_err(|e| fs_error("create_directories", &e, &[&parent]))?;
        let template = CString::new(parent.join(".oscairn-state-XXXXXX").into_os_string().into_encoded_bytes()).unwrap_or_default();
        let raw = template.into_raw();
        // SAFETY: a NUL-terminated template mkdtemp fills in place; taken back at once.
        let made = !unsafe { mkdtemp(raw) }.is_null();
        // SAFETY: into_raw's.
        let pattern = unsafe { CString::from_raw(raw) };
        if !made {
            return Err(Failure::runtime("Cannot create state working directory"));
        }
        let pattern = PathBuf::from(pattern.to_string_lossy().into_owned());
        let temporary = canonical(&pattern)?;
        Ok(Bundle { destination, temporary })
    }
    pub fn publish(mut self) -> Result<(), Failure> {
        if self.destination.exists() || self.destination.is_symlink() {
            return Err(Failure::runtime("State destination already exists"));
        }
        std::fs::rename(&self.temporary, &self.destination).map_err(|e| fs_error("rename", &e, &[&self.temporary, &self.destination]))?;
        self.temporary = PathBuf::new();
        Ok(())
    }
}
impl Drop for Bundle {
    fn drop(&mut self) {
        if !self.temporary.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.temporary);
        }
    }
}

pub fn write(path: &Path, data: &[u8]) -> Result<(), Failure> {
    let written = std::fs::File::create(path).and_then(|mut f| f.write_all(data));
    written.map_err(|_| Failure::runtime(format!("Cannot write state file: {}", path.display())))
}
pub fn read(path: &Path, limit: u64) -> Result<Vec<u8>, Failure> {
    let size = std::fs::metadata(path).map_err(|e| fs_error("file_size", &e, &[path]))?.len();
    if size > limit {
        return Err(Failure::runtime("State file exceeds size limit"));
    }
    let data = std::fs::read(path).map_err(|_| Failure::runtime(format!("Cannot read state file: {}", path.display())))?;
    if data.len() as u64 != size {
        return Err(Failure::runtime(format!("Cannot read state file: {}", path.display())));
    }
    Ok(data)
}
