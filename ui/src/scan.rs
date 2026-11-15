// SPDX-License-Identifier: AGPL-3.0-or-later
//! The plugins a part can add: found as the JUCE app finds them (hosts/standalone/PartsModel.cpp's
//! `bundledPlugins`, core/clap-host's `kick::clap::scan`), by the isolated scanner `oscairn-plugin-scan`
//! beside the program, run once per plugin file (`--worker FILE`, 5 s, its output at most 4 MiB, killed
//! at either) over the plugin folder (`--plugins DIR`, else `plugins` beside the program, else
//! `../lib/clap`), CLAP files then LV2 bundles. Its output is the helper's wire format (`KICKCLAP1`,
//! little-endian numbers and length-prefixed strings), read here with every length checked. The engine
//! is not involved: this is a helper process, as in the JUCE app, so there is no control request.
//! A plugin that fails to scan is left out.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MAGIC: &[u8] = b"KICKCLAP1\n";
const MAX_WIRE: usize = 4 << 20;
pub const TIMEOUT: Duration = Duration::from_millis(5000);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plugin {
    pub path: String,
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub features: Vec<String>,
}

impl Plugin {
    /// "Instrument" or "Effect", as the JUCE chain colors them: a plugin that makes sound from notes.
    pub fn is_instrument(&self) -> bool {
        self.features.iter().any(|f| f == "instrument")
    }
}

/// What the part's menu shows: the name without its vendor ("Oscairn Basic Synth" from Oscairn is
/// "Basic Synth"), by name.
pub fn choices(mut plugins: Vec<Plugin>) -> Vec<Plugin> {
    for p in &mut plugins {
        if !p.vendor.is_empty() && p.name.len() > p.vendor.len() + 1 && p.name.starts_with(&format!("{} ", p.vendor)) {
            p.name = p.name[p.vendor.len() + 1..].to_string();
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

/// The plugin folder as the JUCE app has it.
pub fn plugin_dir(given: Option<&str>) -> PathBuf {
    if let Some(d) = given.filter(|d| !d.is_empty()) {
        return d.into();
    }
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)).unwrap_or_default();
    let bundled = exe.join("plugins");
    let installed = exe.join("../lib/clap");
    if bundled.is_dir() {
        bundled
    } else {
        installed
    }
}

/// The scanner beside the program.
pub fn scanner() -> PathBuf {
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("oscairn-plugin-scan"))).unwrap_or_default()
}

/// Files with `extension` under `root`, as `kick::clap::findBinaries`: at most eight levels down,
/// a match that is a folder (a bundle) not entered, 8192 at most, each once.
fn find_binaries(root: &Path, extension: &str) -> BTreeSet<PathBuf> {
    fn walk(dir: &Path, depth: usize, extension: &str, found: &mut BTreeSet<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            if found.len() >= 8192 {
                return;
            }
            let path = e.path();
            if path.extension().is_some_and(|x| x.to_string_lossy() == extension.trim_start_matches('.')) {
                if let Ok(p) = std::fs::canonicalize(&path) {
                    found.insert(p);
                }
            } else if depth < 8 && path.is_dir() {
                walk(&path, depth + 1, extension, found);
            }
        }
    }
    let mut found = BTreeSet::new();
    if root.extension().is_some_and(|x| x.to_string_lossy() == extension.trim_start_matches('.')) {
        if let Ok(p) = std::fs::canonicalize(root) {
            found.insert(p);
        }
    } else if root.is_dir() {
        walk(root, 0, extension, &mut found);
    }
    found
}

/// The helper on one file: its stdout, or why not (it ended badly, took more than `timeout`, or said more than 4 MiB).
fn run_helper(helper: &Path, binary: &Path, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut child = Command::new(helper).arg("--worker").arg(binary).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| format!("Cannot launch the scanner helper: {e}"))?;
    // The write end of its stdin stays here: the helper ends itself if this process dies.
    let _life = child.stdin.take();
    let mut out = child.stdout.take().ok_or("no pipe")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut out).take(MAX_WIRE as u64 + 1).read_to_end(&mut bytes);
        bytes
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Ok(s),
            Ok(None) if start.elapsed() > timeout => {
                // shortcut: the helper is killed, not its process group as the C++ scanner does; a
                // grandchild holding the pipe is left to end by itself (the reader is not waited for).
                let _ = child.kill();
                let _ = child.wait();
                return Err("Plugin scan timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => break Err(e),
        }
    };
    let bytes = reader.join().unwrap_or_default();
    match status {
        Ok(s) if s.success() && bytes.len() <= MAX_WIRE => Ok(bytes),
        Ok(s) if s.success() => Err("Plugin scanner output exceeded size limit".into()),
        _ => Err("Plugin crashed or failed during scan".into()),
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn number(&mut self) -> Option<u32> {
        let b = self.bytes.get(self.at..self.at + 4)?;
        self.at += 4;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn field(&mut self) -> Option<String> {
        let n = self.number()? as usize;
        if n > MAX_WIRE {
            return None;
        }
        let b = self.bytes.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(String::from_utf8_lossy(b).into_owned())
    }
}

/// The helper's output read as `decodeScan` does, every count and length bounded; None for anything it would refuse.
pub fn decode(bytes: &[u8]) -> Option<Vec<Plugin>> {
    if bytes.len() > MAX_WIRE {
        return None;
    }
    let start = bytes.windows(MAGIC.len()).rposition(|w| w == MAGIC)?;
    let mut r = Reader { bytes, at: start + MAGIC.len() };
    let count = r.number()?;
    if count > 4096 {
        return None;
    }
    let mut plugins = Vec::new();
    for _ in 0..count {
        let mut p = Plugin { path: r.field()?, id: r.field()?, name: r.field()?, vendor: r.field()?, ..Default::default() };
        let _version = r.field()?;
        let _description = r.field()?;
        let features = r.number()?;
        if features > 128 {
            return None;
        }
        for _ in 0..features {
            p.features.push(r.field()?);
        }
        let (discovery, presets) = (r.number()?, r.number()?);
        if discovery > 1 || presets > 4096 {
            return None;
        }
        for _ in 0..presets {
            let _ = (r.field()?, r.number()?, r.field()?, r.field()?);
        }
        if p.path.is_empty() || p.id.is_empty() || p.name.is_empty() {
            return None;
        }
        plugins.push(p);
    }
    let failures = r.number()?;
    if failures > 8192 {
        return None;
    }
    for _ in 0..failures {
        let _ = (r.field()?, r.field()?);
    }
    (r.at == bytes.len()).then_some(plugins)
}

/// Every plugin under `folder` the helper describes, CLAP files then LV2 bundles; blocks, so off the UI thread.
pub fn scan(folder: &Path, helper: &Path, timeout: Duration) -> Vec<Plugin> {
    let mut plugins = Vec::new();
    for extension in [".clap", ".lv2"] {
        for binary in find_binaries(folder, extension) {
            if let Some(found) = run_helper(helper, &binary, timeout).ok().and_then(|b| decode(&b)) {
                plugins.extend(found);
            }
        }
    }
    plugins
}

#[cfg(test)]
pub mod tests {
    use super::*;

    fn number(out: &mut Vec<u8>, n: u32) {
        out.extend(n.to_le_bytes());
    }
    fn field(out: &mut Vec<u8>, s: &str) {
        number(out, s.len() as u32);
        out.extend(s.as_bytes());
    }

    /// What the helper writes for `plugins` as (path, id, name, vendor, features), as `encodeScan` does.
    pub fn encode(plugins: &[(&str, &str, &str, &str, &[&str])]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        number(&mut out, plugins.len() as u32);
        for (path, id, name, vendor, features) in plugins {
            for s in [path, id, name, vendor, &"1.0", &""] {
                field(&mut out, s);
            }
            number(&mut out, features.len() as u32);
            for f in *features {
                field(&mut out, f);
            }
            number(&mut out, 0);
            number(&mut out, 0);
        }
        number(&mut out, 0);
        out
    }

    /// A folder inside the build's own target directory (no user folder is touched).
    pub fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/scratch").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A helper script that prints `output` (a file beside it), whatever it is asked.
    pub fn fake_helper(dir: &Path, output: &[u8], tail: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(dir.join("out.bin"), output).unwrap();
        let helper = dir.join("helper.sh");
        std::fs::write(&helper, format!("#!/bin/sh\n{tail}\ncat \"$(dirname \"$0\")/out.bin\"\n")).unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        helper
    }

    #[test]
    fn reads_the_helpers_words_and_nothing_odd() {
        let blob = encode(&[("/p/a.clap", "org.a", "Oscairn Basic Synth", "Oscairn", &["instrument"]), ("/p/a.clap", "org.b", "Chorus", "", &["audio-effect"])]);
        let got = decode(&blob).unwrap();
        assert_eq!((got.len(), got[0].id.as_str(), got[0].is_instrument(), got[1].is_instrument()), (2, "org.a", true, false));
        let names: Vec<_> = choices(got).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["Basic Synth", "Chorus"], "the vendor's prefix is left off, by name");
        // Anything the C++ reader refuses: no magic, truncated, trailing bytes, a length past the end, an empty id.
        assert!(decode(b"nothing").is_none());
        assert!(decode(&blob[..blob.len() - 1]).is_none());
        let mut more = blob.clone();
        more.push(0);
        assert!(decode(&more).is_none());
        let mut huge = MAGIC.to_vec();
        number(&mut huge, 1);
        number(&mut huge, 0xFFFF_FFF0);
        assert!(decode(&huge).is_none());
        assert!(decode(&encode(&[("/p", "", "N", "", &[])])).is_none());
        // Noise before the magic is skipped, as the helper's output may carry a plugin's own prints.
        let mut noisy = b"hello\n".to_vec();
        noisy.extend(&blob);
        assert_eq!(decode(&noisy).unwrap().len(), 2);
    }

    #[test]
    fn scans_a_folder_with_the_helper_and_survives_a_bad_one() {
        let dir = scratch("scan");
        std::fs::create_dir_all(dir.join("plugins/deep/x.clap")).unwrap();
        std::fs::write(dir.join("plugins/a.clap"), "").unwrap();
        std::fs::write(dir.join("plugins/deep/x.clap/inner.clap"), "").unwrap();
        std::fs::write(dir.join("plugins/b.lv2"), "").unwrap();
        let blob = encode(&[("/p/a.clap", "org.a", "Synth", "", &["instrument"])]);
        let helper = fake_helper(&dir, &blob, "");
        let found = scan(&dir.join("plugins"), &helper, Duration::from_secs(5));
        assert_eq!(found.len(), 3, "a.clap, the bundle x.clap (not entered), b.lv2");
        // A helper that fails, one that says nonsense and one that hangs: nothing found, no panic.
        let bad = fake_helper(&dir, &blob, "exit 1");
        assert!(scan(&dir.join("plugins"), &bad, Duration::from_secs(5)).is_empty());
        let nonsense = fake_helper(&dir, b"junk", "");
        assert!(scan(&dir.join("plugins"), &nonsense, Duration::from_secs(5)).is_empty());
        let slow = fake_helper(&dir, &blob, "sleep 5");
        let start = Instant::now();
        assert!(scan(&dir.join("plugins/a.clap"), &slow, Duration::from_millis(100)).is_empty());
        assert!(start.elapsed() < Duration::from_secs(3), "killed at the timeout");
        assert!(scan(&dir.join("plugins"), Path::new("/no/such/helper"), TIMEOUT).is_empty());
        assert!(scan(&dir.join("none"), &helper, TIMEOUT).is_empty());
    }
}
