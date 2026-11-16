// SPDX-License-Identifier: AGPL-3.0-or-later
//! The engine of the real-engine harnesses (ui/REAL-ENGINE.md): a harness never connects to an engine somebody else started
//! (that one may have the owner's real data home and every save, delete and import would land there). It starts its own as a
//! child, in a process group of its own, with an environment built from nothing in which every folder the engine uses lies in
//! the root of the run; it asks the engine where it works before the first request that writes; a request that names a path
//! goes through the guard; and the child and its workers are killed when the harness ends, however it ends.

use super::guard::Root;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The environment of the engine: nothing inherited. Every variable the engine and its workers read to find a data, config,
/// cache, runtime or temporary folder (`OSCAIRN_DATA_HOME`, `OSCAIRN_CONFIG_HOME`, `HOME`, `XDG_*`, `TMPDIR`) points inside
/// the root; `PATH` is the system's. `CLAP_PATH` and `LV2_PATH` are left out (plugins come from `--plugins`).
pub fn child_env(root: &Root) -> Vec<(OsString, OsString)> {
    let run = root.dir().join("engine-run");
    let at = |p: &Path| p.as_os_str().to_owned();
    vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("HOME".into(), at(root.home())),
        ("OSCAIRN_DATA_HOME".into(), at(root.data_home())),
        ("OSCAIRN_CONFIG_HOME".into(), at(root.config_home())),
        ("XDG_DATA_HOME".into(), at(&run.join("xdg-data"))),
        ("XDG_CONFIG_HOME".into(), at(&run.join("xdg-config"))),
        ("XDG_CACHE_HOME".into(), at(&run.join("xdg-cache"))),
        ("XDG_STATE_HOME".into(), at(&run.join("xdg-state"))),
        ("XDG_RUNTIME_DIR".into(), at(&run.join("runtime"))),
        ("TMPDIR".into(), at(&run.join("tmp"))),
    ]
}

/// A socket path from the environment (`OSCAIRN_REAL_SOCKET`) is a stranger's engine: refused. (The harness starts its own.)
pub fn refuse_supplied_socket(supplied: Option<OsString>) -> Result<(), String> {
    match supplied.filter(|v| !v.is_empty()) {
        None => Ok(()),
        Some(v) => Err(format!("OSCAIRN_REAL_SOCKET is set ({v:?}): a harness talks only to the engine it starts itself, in its own root; unset it and give OSCAIRN_REAL_ENGINE (the engine binary)")),
    }
}

/// Every string a request names as a path, by the keys the engine's own table of path arguments uses.
fn path_arguments(method: &str, params: &Value) -> Vec<String> {
    let key = match method {
        "performance.save" | "performance.load" | "performance.prepare" | "budget.estimate" | "performance.contents" | "performance.import" | "state.save" => "directory",
        "performance.export" | "library.export" | "library.exportLibrary" | "library.contents" | "library.install" | "assets.import" => "file",
        "library.addFolder" | "assets.register" => "folder",
        "plugins.addBuild" | "graph.add" | "graph.replace" => "path",
        "sounds.prepare" => "state",
        "packages.list" | "packages.install" => "source",
        "packages.addRepository" => "address",
        _ => return vec![],
    };
    let Some(s) = params.get(key).and_then(Value::as_str) else { return vec![] };
    // A web address is not a path of this computer (the engine's own check takes the same view).
    if (key == "source" || key == "address") && (s.starts_with("http://") || s.starts_with("https://")) {
        return vec![];
    }
    // A plugin's path (graph.add) is a plugin the engine loads, not a file it writes: read from the plugin folder.
    if matches!(method, "graph.add" | "graph.replace" | "plugins.addBuild") {
        return vec![];
    }
    vec![s.to_string()]
}

/// A request line of the protocol (one request, or a batch): `Err` when it names a path outside the root, a relative one, a
/// `..` or a symlink out.
pub fn check_request(root: &Root, line: &str) -> Result<(), String> {
    fn one(root: &Root, doc: &Value) -> Result<(), String> {
        if let Some(batch) = doc.as_array() {
            return batch.iter().try_for_each(|d| one(root, d));
        }
        let method = doc["method"].as_str().unwrap_or("");
        for path in path_arguments(method, &doc["params"]) {
            root.inside(Path::new(&path)).map_err(|e| format!("{method}: {e}"))?;
        }
        Ok(())
    }
    match serde_json::from_str::<Value>(line) {
        Ok(doc) => one(root, &doc),
        Err(_) => Ok(()),
    }
}

/// Every `path` in an answer (at any depth).
fn reported_paths(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                match (k.as_str(), x.as_str()) {
                    ("path", Some(s)) if !s.is_empty() => out.push(s.to_string()),
                    _ => reported_paths(x, out),
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| reported_paths(x, out)),
        _ => {}
    }
}

/// Before the first request that writes: the engine is using this root. The engine does not report its data folder, so it is
/// shown two ways: it has written into the root's data home (it does at start), and the paths of everything it lists (library
/// items, installed libraries, asset libraries) lie inside the root. `ask` sends a read-only request and returns its `result`.
pub fn verify(root: &Root, ask: &mut dyn FnMut(&str, Value) -> Value) -> Result<(), String> {
    let data = root.data_home();
    let written = std::fs::read_dir(data).map(|mut d| d.next().is_some()).unwrap_or(false);
    if !written {
        return Err(format!("the engine has written nothing into {} (OSCAIRN_DATA_HOME of the run): it is not using the root's data home", data.display()));
    }
    let mut paths = vec![];
    for (method, params) in [("library.query", json!({})), ("library.libraries", json!({})), ("assets.libraries", json!({}))] {
        reported_paths(&ask(method, params), &mut paths);
    }
    for p in paths {
        root.inside(Path::new(&p)).map_err(|e| format!("the engine reports {p}: {e}"))?;
    }
    Ok(())
}

/// The engine the harness started: a child in a process group of its own, killed with its workers on drop.
pub struct ChildEngine {
    child: Child,
    pub socket: PathBuf,
}

impl ChildEngine {
    /// Starts `binary` (`OSCAIRN_REAL_ENGINE`) with `--audio null`, its socket in a private folder of the root, the environment
    /// of `child_env`, and `--plugins` / `--device-profile` when given. Returns when it listens.
    pub fn start(root: &Root, binary: &Path, plugins: Option<&str>, profile: Option<&str>) -> ChildEngine {
        refuse_supplied_socket(std::env::var_os("OSCAIRN_REAL_SOCKET")).unwrap_or_else(|e| panic!("the real-engine harness refuses to run: {e}"));
        let run = root.dir().join("engine-run");
        for d in ["", "xdg-data", "xdg-config", "xdg-cache", "xdg-state", "runtime", "tmp", "sock"] {
            root.create_dir_all(&run.join(d)).unwrap();
        }
        for d in [root.home(), root.data_home(), root.config_home()] {
            root.create_dir_all(d).unwrap();
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(run.join("sock"), std::fs::Permissions::from_mode(0o700)).unwrap();
        let socket = run.join("sock/engine.sock");
        assert!(socket.as_os_str().len() < 100, "{} is too long for a socket path: use a shorter OSCAIRN_TEST_ROOT", socket.display());
        let log = std::fs::File::create(run.join("engine.log")).unwrap();
        let mut cmd = Command::new(binary);
        cmd.env_clear().envs(child_env(root)).args(["--socket"]).arg(&socket).args(["--audio", "null", "--buffer-frames", "128"]);
        if let Some(p) = plugins {
            cmd.args(["--plugins", p]);
        }
        if let Some(p) = profile {
            cmd.args(["--device-profile", p]);
        }
        use std::os::unix::process::CommandExt;
        let child = cmd.process_group(0).stdin(Stdio::null()).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap_or_else(|e| panic!("cannot start the engine {}: {e}", binary.display()));
        let engine = ChildEngine { child, socket };
        let until = Instant::now() + Duration::from_secs(30);
        while !std::fs::read_to_string(run.join("engine.log")).unwrap_or_default().contains("listening on") {
            assert!(Instant::now() < until, "the engine did not listen within 30 s: {}", std::fs::read_to_string(run.join("engine.log")).unwrap_or_default());
            std::thread::sleep(Duration::from_millis(50));
        }
        engine
    }
}

impl Drop for ChildEngine {
    fn drop(&mut self) {
        // The child is its group's leader and its workers are in the group: ask, wait a moment, then end them all.
        let group = self.child.id() as libc::pid_t;
        // SAFETY: killpg on the group of the child this struct started.
        unsafe { libc::killpg(group, libc::SIGTERM) };
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until && matches!(self.child.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(50));
        }
        unsafe { libc::killpg(group, libc::SIGKILL) };
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oscairn-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::canonicalize(&dir).unwrap()
    }

    fn root_in(dir: &Path, home: &Path) -> Root {
        let map: HashMap<&str, OsString> = [
            ("OSCAIRN_TEST_ROOT", dir.as_os_str().to_owned()),
            ("HOME", dir.join("home").into_os_string()),
            ("OSCAIRN_DATA_HOME", dir.join("data").into_os_string()),
            ("OSCAIRN_CONFIG_HOME", dir.join("config").into_os_string()),
        ]
        .into_iter()
        .collect();
        Root::check(|n| map.get(n).cloned(), home).unwrap()
    }

    #[test]
    fn the_engines_environment_is_built_from_nothing_and_every_folder_is_in_the_root() {
        let (dir, home) = (scratch("env"), scratch("env-home"));
        let root = root_in(&dir, &home);
        let env = child_env(&root);
        let names: Vec<String> = env.iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        for needed in ["HOME", "OSCAIRN_DATA_HOME", "OSCAIRN_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR", "TMPDIR"] {
            assert!(names.contains(&needed.to_string()), "{needed} missing");
        }
        // Only these: no other XDG_*/OSCAIRN_* variable and nothing from this process.
        for n in &names {
            assert!(n == "PATH" || n == "HOME" || n == "TMPDIR" || n.starts_with("XDG_") || n.starts_with("OSCAIRN_"), "unexpected {n}");
        }
        for (k, v) in &env {
            if k == "PATH" {
                assert_eq!(v, "/usr/bin:/bin");
            } else {
                assert!(Path::new(v).starts_with(&dir), "{k:?} = {v:?} is outside the root");
            }
        }
        // A spawn with it, in an inherited environment that has all of those set: the child sees only the root's.
        let out = Command::new("/usr/bin/env").env("HOME", "/Users/someone").env("XDG_DATA_HOME", "/elsewhere").env("OSCAIRN_X", "y").env_clear().envs(child_env(&root)).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(!text.contains("/Users/someone") && !text.contains("/elsewhere") && !text.contains("OSCAIRN_X"), "{text}");
        assert!(text.contains(&format!("HOME={}", root.home().display())), "{text}");
    }

    #[test]
    fn a_supplied_socket_is_refused() {
        assert!(refuse_supplied_socket(None).is_ok());
        assert!(refuse_supplied_socket(Some(OsString::new())).is_ok());
        assert!(refuse_supplied_socket(Some("/tmp/other/engine.sock".into())).is_err(), "outside");
        let dir = scratch("sock");
        assert!(refuse_supplied_socket(Some(dir.join("engine.sock").into_os_string())).is_err(), "even one inside: the harness starts its own");
    }

    #[test]
    fn verify_fails_for_an_engine_that_reports_a_folder_outside_the_root() {
        let (dir, home, outside) = (scratch("verify"), scratch("verify-home"), scratch("verify-outside"));
        let root = root_in(&dir, &home);
        // The engine has not written into the root's data home: refused whatever it answers.
        std::fs::create_dir_all(root.data_home()).unwrap();
        assert!(verify(&root, &mut |_, _| json!({"items": []})).is_err(), "nothing written into the data home");
        std::fs::write(root.data_home().join("parameter-revision"), b"1").unwrap();
        // An honest engine: its items are in the root.
        let inside = root.data_home().join("Performances/a.oscairn");
        assert!(verify(&root, &mut |m, _| if m == "library.query" { json!({"items": [{"id": "1", "path": inside.to_str().unwrap()}]}) } else { json!({"libraries": []}) }).is_ok());
        // A fake engine on the owner's data home: an item, an installed library or an asset library outside the root.
        let real = outside.join("Library/Application Support/Oscairn/Performances/mine.oscairn");
        for lie in ["library.query", "library.libraries", "assets.libraries"] {
            let e = verify(&root, &mut |m, _| if m == lie { json!({"items": [{"path": real.to_str().unwrap()}], "libraries": [{"path": real.to_str().unwrap()}]}) } else { json!({}) }).err().unwrap_or_else(|| panic!("{lie} outside was accepted"));
            assert!(e.contains("not inside"), "{e}");
        }
        // A relative path is as bad.
        assert!(verify(&root, &mut |_, _| json!({"items": [{"path": "Performances/x"}]})).is_err());
    }

    #[test]
    fn a_request_with_a_path_outside_the_root_a_dotdot_or_a_link_out_is_refused() {
        let (dir, home, outside) = (scratch("req"), scratch("req-home"), scratch("req-outside"));
        let root = root_in(&dir, &home);
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("docs/link")).unwrap();
        let line = |method: &str, key: &str, path: &Path| json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": {key: path.to_str().unwrap()}}).to_string();
        // Inside: sent.
        assert!(check_request(&root, &line("library.export", "file", &dir.join("docs/a.olib"))).is_ok());
        assert!(check_request(&root, &line("library.addFolder", "folder", &dir.join("docs"))).is_ok());
        assert!(check_request(&root, &line("library.query", "text", Path::new("/anywhere"))).is_ok(), "not a path argument");
        // Outside, `..`, a link out, relative, the home: refused, for each method that takes a path.
        let home_docs = real_home_documents();
        for (method, key) in [("library.export", "file"), ("library.exportLibrary", "file"), ("performance.export", "file"), ("library.install", "file"), ("library.contents", "file"), ("assets.import", "file"), ("library.addFolder", "folder"), ("assets.register", "folder"), ("performance.import", "directory"), ("performance.save", "directory"), ("state.save", "directory"), ("packages.install", "source")] {
            for bad in [outside.join("x.olib"), dir.join("docs/../../x.olib"), dir.join("docs/link/x.olib"), PathBuf::from("x.olib"), home_docs.join("x.olib")] {
                assert!(check_request(&root, &line(method, key, &bad)).is_err(), "{method} {}", bad.display());
            }
        }
        // A web address is not a path; a batch is checked in each of its requests.
        assert!(check_request(&root, &line("packages.install", "source", Path::new("https://example.org/a.olib"))).is_ok());
        let batch = format!("[{},{}]", line("library.export", "file", &dir.join("a.olib")), line("library.export", "file", &outside.join("a.olib")));
        assert!(check_request(&root, &batch).is_err());
    }

    fn real_home_documents() -> PathBuf {
        super::super::guard::real_home().unwrap().join("Documents")
    }

    #[test]
    fn the_child_and_its_workers_are_killed_with_the_harness() {
        // A stand-in for the engine: a shell that starts a worker of its own and prints what the engine prints.
        // (A short path: the socket of the child must fit a sun_path.)
        let dir = PathBuf::from(format!("/tmp/ohg-kill-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        let home = scratch("kill-home");
        let root = root_in(&dir, &home);
        let script = dir.join("engine.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 300 &\necho $! > \"$OSCAIRN_DATA_HOME/worker.pid\"\necho 'oscairn-engine: listening on x'\nwait\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let engine = ChildEngine::start(&root, &script, None, None);
        let worker: libc::pid_t = std::fs::read_to_string(root.data_home().join("worker.pid")).unwrap().trim().parse().unwrap();
        // SAFETY: signal 0 only asks whether the process exists.
        let alive = |p: libc::pid_t| unsafe { libc::kill(p, 0) == 0 };
        assert!(alive(worker));
        drop(engine);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!alive(worker), "the worker of the engine is still running");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
