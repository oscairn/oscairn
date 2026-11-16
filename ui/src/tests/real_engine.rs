// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Performance app against a real engine (ui/REAL-ENGINE.md): ignored unless asked for. The harness starts the engine
//! itself, as a child with the null audio device, an environment built from nothing and every folder in the root of the run
//! (`engine.rs`); it never connects to an engine somebody else started.
//!
//! `OSCAIRN_REAL_ENGINE=/build/.../oscairn-engine OSCAIRN_REAL_PLUGINS=/the/engine's/plugins
//! OSCAIRN_REAL_SCANNER=/build/core/oscairn-plugin-scan cargo test real_engine -- --ignored --nocapture`
//! with the root of the run set as ui/REAL-ENGINE.md says (`OSCAIRN_REAL_ONLY=part,chain` for some sections only).
//!
//! The shell talks to the engine through a recording proxy, so every request it sends, every answer
//! and every notification is in the trace; the engine's state is read again on a connection of the
//! harness's own (never through the UI's picture). Pictures go to target/real-engine/.

use super::*;
use std::io::{BufRead, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// One line through the proxy.
#[derive(Clone, Debug)]
enum Line {
    Request { id: u64, method: String, params: Value },
    Reply { id: u64, result: Option<Value>, error: Option<Value> },
    Note { method: String, params: Value },
}

type Trace = Arc<Mutex<Vec<Line>>>;

/// Requests the guard refused to send (a path outside the root): the run stops at its next step.
static REFUSED: Mutex<Vec<String>> = Mutex::new(Vec::new());
use serde_json::Value;

/// Listens at `path` and passes each connection's lines to the engine at `engine` and back, noting them.
fn proxy(path: &std::path::Path, engine: std::path::PathBuf, trace: Trace, root: Arc<guard::Root>) {
    // (A path of this process's own: nothing is deleted; one that is there is a stranger's and stops the run.)
    assert!(!path.exists(), "{} exists already", path.display());
    let listener = UnixListener::bind(path).unwrap();
    std::thread::spawn(move || {
        for ui in listener.incoming() {
            let Ok(ui) = ui else { continue };
            let Ok(up) = UnixStream::connect(&engine) else { continue };
            let (ui_r, up_w, up_r, ui_w) = (ui.try_clone().unwrap(), up.try_clone().unwrap(), up, ui);
            let (t, r) = (trace.clone(), Some(root.clone()));
            std::thread::spawn(move || copy(ui_r, up_w, t, r));
            let t = trace.clone();
            std::thread::spawn(move || copy(up_r, ui_w, t, None));
        }
    });
}

/// `root` is given for the lines going up to the engine: a request that names a path outside it is not sent.
fn copy(from: UnixStream, mut to: UnixStream, trace: Trace, root: Option<Arc<guard::Root>>) {
    let upward = root.is_some();
    for line in std::io::BufReader::new(from).lines() {
        let Ok(line) = line else { break };
        if let Some(Err(e)) = root.as_ref().map(|r| engine::check_request(r, &line)) {
            eprintln!("REFUSED {e}");
            REFUSED.lock().unwrap().push(e);
            break;
        }
        if let Ok(doc) = serde_json::from_str::<Value>(&line) {
            let id = doc["id"].as_u64().unwrap_or(u64::MAX);
            let entry = if upward {
                Line::Request { id, method: doc["method"].as_str().unwrap_or("").into(), params: doc["params"].clone() }
            } else if let Some(m) = doc["method"].as_str() {
                Line::Note { method: m.into(), params: doc["params"].clone() }
            } else {
                Line::Reply { id, result: doc.get("result").cloned(), error: doc.get("error").cloned() }
            };
            trace.lock().unwrap().push(entry);
        }
        if writeln!(to, "{line}").is_err() {
            break;
        }
    }
    let _ = to.shutdown(std::net::Shutdown::Both);
}

/// A connection of the harness's own to the engine.
struct Direct {
    reader: std::io::BufReader<UnixStream>,
    writer: UnixStream,
    next: u64,
}

impl Direct {
    fn new(path: &std::path::Path) -> Direct {
        let s = UnixStream::connect(path).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        Direct { reader: std::io::BufReader::new(s.try_clone().unwrap()), writer: s, next: 1 }
    }

    /// The whole answer: `result` or `error`.
    fn raw(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let line = json!({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params}).to_string();
        if let Err(e) = engine::check_request(&guard::root_or_stop(), &line) {
            panic!("the harness would send a path outside its root: {e}");
        }
        writeln!(self.writer, "{line}").unwrap();
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let doc: Value = serde_json::from_str(&line).unwrap();
            if doc["id"].as_u64() == Some(self.next) {
                return doc;
            }
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let doc = self.raw(method, params.clone());
        assert!(doc.get("error").is_none(), "{method} {params}: {doc}");
        doc["result"].clone()
    }

    fn graph(&mut self) -> Value {
        self.call("graph.get", json!({}))
    }
}

/// Before anything else: the root of the run, the engine started in it and checked to be using it, the proxy's socket and a
/// connection of the harness's own. A harness never reaches an engine it did not start.
fn start_engine(binary: &std::path::Path, plugins: Option<&str>, profile: Option<&str>) -> (std::path::PathBuf, Trace, Direct, engine::ChildEngine) {
    let root = Arc::new(guard::root_or_stop()); // before an engine is started or a file touched
    let child = engine::ChildEngine::start(&root, binary, plugins, profile);
    let mut direct = Direct::new(&child.socket);
    engine::verify(&root, &mut |m, p| direct.call(m, p)).unwrap_or_else(|e| panic!("the real-engine harness refuses to go on: {e}"));
    let trace: Trace = Arc::default();
    let path = std::env::temp_dir().join(format!("oscairn-ui-real-{}.sock", std::process::id()));
    proxy(&path, child.socket.clone(), trace.clone(), root);
    fit_to_window();
    (path, trace, direct, child)
}

/// The shell on a headless window, the proxy and a direct connection.
struct Real {
    screen: Screen,
    events: mpsc::Receiver<Event<Request>>,
    trace: Trace,
    engine: Direct,
    /// The engine this harness started (killed with its workers when this is dropped).
    _child: engine::ChildEngine,
    /// What went wrong or looked wrong, said in the report at the end.
    findings: Vec<String>,
    /// Where the trace was when the last step began.
    mark: usize,
    /// How many frames the window drew while the shell ran (a frame is drawn only when something changed).
    frames: usize,
    /// How many times the figures the top bar shows for the CPU and RAM changed (a frame for each is no waste).
    figures: usize,
    shown_figures: (i32, i32),
}

impl Real {
    fn start(seed: seed::Seed) -> Option<Real> {
        let binary = std::path::PathBuf::from(std::env::var_os("OSCAIRN_REAL_ENGINE")?);
        let (path, trace, engine, child) = start_engine(&binary, std::env::var("OSCAIRN_REAL_PLUGINS").ok().as_deref(), Some("fantom-07"));
        let mut engine = engine;
        // The seed of this harness, made in the engine of this run (seed.rs), from nothing.
        seed::make(&mut engine, seed, std::env::var("OSCAIRN_REAL_PLUGINS").ok().as_deref());
        // The Performance built for the check (ui/REAL-ENGINE.md), opened again so each run starts from it.
        let found = engine.call("library.query", json!({"text": "Real Engine Check", "kind": "performance"}));
        if let Some(id) = found["items"][0]["id"].as_str() {
            engine.call("library.load", json!({"id": id}));
        }
        let screen = Screen::new();
        let (tx, events) = mpsc::channel();
        // Where the scanner looks; the scanner itself must be beside the test binary (REAL-ENGINE.md).
        let plugins = std::env::var("OSCAIRN_REAL_PLUGINS").ok();
        shell::set_plugin_dir(plugins.clone());
        shell::start(&screen.ui, path, &crate::profile::Profile::load(Some("fantom-07")).unwrap(), false, move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        // The plugin scanner as the shell runs it, its answer handed over here: the headless tests
        // have no event loop for the shell's own scanning thread to post it through.
        if let (Some(dir), Ok(helper)) = (plugins, std::env::var("OSCAIRN_REAL_SCANNER")) {
            shell::plugins_found(crate::scan::scan(std::path::Path::new(&dir), std::path::Path::new(&helper), crate::scan::TIMEOUT));
        }
        Some(Real { screen, events, trace, engine, _child: child, findings: vec![], mark: 0, frames: 0, figures: 0, shown_figures: (-1, -1) })
    }

    /// Hands the shell its events and runs its timers for `ms`.
    fn run(&mut self, ms: u64) {
        assert!(REFUSED.lock().unwrap().is_empty(), "the guard refused a request: {:?}", REFUSED.lock().unwrap());
        let until = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < until {
            // Everything that has arrived, then one frame (a window draws once per frame, however many events came).
            if let Ok(e) = self.events.recv_timeout(Duration::from_millis(10)) {
                shell::dispatch(e);
                while let Ok(e) = self.events.try_recv() {
                    shell::dispatch(e);
                }
            }
            let bar = self.screen.ui.global::<Bar>();
            let figures = (bar.get_cpu(), bar.get_ram());
            if figures != self.shown_figures {
                self.shown_figures = figures;
                self.figures += 1;
            }
            if self.screen.frame().is_some() {
                self.frames += 1;
            }
        }
    }

    /// Runs until `done` or 10 s; false at the time limit.
    fn until(&mut self, mut done: impl FnMut(&Shell) -> bool) -> bool {
        let until = Instant::now() + Duration::from_secs(10);
        while Instant::now() < until {
            if done(&self.screen.ui) {
                return true;
            }
            self.run(20);
        }
        false
    }

    /// Settles: runs until no request has been out for a while (the meters' polls go on).
    fn settle(&mut self) {
        self.run(400);
    }

    fn act(&mut self, a: Action) {
        shell::act(a);
        self.settle();
    }

    fn png(&mut self, name: &str) {
        self.screen.frame();
        png_in("target/real-engine", &self.screen, name);
    }

    fn lines(&self) -> Vec<Line> {
        self.trace.lock().unwrap().clone()
    }

    /// The requests since the mark, with their answers.
    fn since(&self, mark: usize) -> Vec<(String, Value, Option<Value>, Option<Value>)> {
        let lines = self.lines();
        let mut out = vec![];
        for (i, l) in lines.iter().enumerate().skip(mark) {
            if let Line::Request { id, method, params } = l {
                let answer = lines[i..].iter().find_map(|r| match r {
                    Line::Reply { id: rid, result, error } if rid == id => Some((result.clone(), error.clone())),
                    _ => None,
                });
                let (result, error) = answer.unwrap_or((None, None));
                out.push((method.clone(), params.clone(), result, error));
            }
        }
        out
    }

    /// Begins a step: what follows is checked from here.
    fn step(&mut self, what: &str) {
        eprintln!("== {what}");
        self.mark = self.lines().len();
    }

    /// The requests of the step that are not polls, each with what came back; refusals noted as findings.
    fn sent(&mut self, what: &str) -> Vec<(String, Value, Option<Value>, Option<Value>)> {
        let polls = ["parts.meters", "wires.meters", "record.takes", "record.get", "memory.state", "transport.get", "controllers.view", "edit.history", "performance.info", "graph.get", "session.status"];
        let all = self.since(self.mark);
        for (m, p, _, e) in &all {
            if let Some(e) = e {
                let f = format!("{what}: {m} {p} refused: {e}");
                eprintln!("FINDING {f}");
                self.findings.push(f);
            }
        }
        let notes: Vec<String> = self.lines().iter().skip(self.mark).filter_map(|l| if let Line::Note { method, .. } = l { Some(method.clone()) } else { None }).filter(|m| m != "status" && m != "controllers").collect();
        if std::env::var_os("OSCAIRN_REAL_NOTES").is_some() {
            for l in self.lines().iter().skip(self.mark) {
                if let Line::Note { method, params } = l {
                    if method == "params.changed" {
                        eprintln!("   note params.changed {}", short(params));
                    }
                }
            }
        }
        let asked: Vec<&str> = all.iter().filter(|(m, ..)| m == "graph.get" || m == "performance.info").map(|x| x.0.as_str()).collect();
        let history: Vec<String> = all.iter().filter(|(m, ..)| m == "edit.history").map(|x| x.2.as_ref().map_or("-".into(), |v| format!("{}/{}", v["undo"], v["redo"]))).collect();
        if !history.is_empty() {
            eprintln!("   edit.history answers {history:?}");
        }
        eprintln!("   notifications {notes:?}; asked again {asked:?}");
        let edits: Vec<_> = all.into_iter().filter(|(m, ..)| !polls.contains(&m.as_str())).collect();
        for (m, p, _, e) in &edits {
            eprintln!("   {m} {p} {}", if e.is_some() { "ERROR" } else { "ok" });
        }
        edits
    }

    fn note(&mut self, f: String) {
        eprintln!("FINDING {f}");
        self.findings.push(f);
    }

    fn check(&mut self, ok: bool, f: impl FnOnce() -> String) {
        if !ok {
            let f = f();
            self.note(f);
        }
    }

    fn rows(&self) -> Vec<(String, String)> {
        self.screen.ui.global::<crate::PartState>().get_rows().iter().map(|r| (r.label.to_string(), r.value.to_string())).collect()
    }

    fn undo(&mut self, what: &str) {
        let before = self.engine.call("edit.history", json!({}));
        let r = self.engine.raw("edit.undo", json!({}));
        self.check(r.get("error").is_none(), || format!("{what}: undo refused: {r}"));
        eprintln!("   undo ({}) -> {}", before["undo"], r["result"]);
        self.settle();
    }

    /// The top bar's action named `starts`, tapped; false when it is not there or disabled.
    fn bar(&mut self, starts: &str) -> bool {
        let actions: Vec<(String, bool)> = self.screen.ui.global::<Bar>().get_actions().iter().map(|a| (a.label.to_string(), a.enabled)).collect();
        let Some(i) = actions.iter().position(|a| a.0.starts_with(starts) && a.1) else {
            self.note(format!("top bar: no enabled {starts:?} among {actions:?}"));
            return false;
        };
        self.screen.ui.global::<Bar>().invoke_action(i as i32);
        self.settle();
        true
    }

    /// Undo through the top bar, as the musician does, with what it named.
    fn undo_ui(&mut self, what: &str) {
        // What undo names comes with the next notification's edit.history, after the requests already out.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && !self.screen.ui.global::<Bar>().get_actions().iter().any(|a| a.label.starts_with("Undo") && a.enabled) {
            self.run(50);
        }
        let label = self.screen.ui.global::<Bar>().get_actions().iter().map(|a| a.label.to_string()).find(|l| l.starts_with("Undo")).unwrap_or_default();
        eprintln!("   top bar: {label:?}");
        self.step(&format!("{what}: Undo on the top bar"));
        if self.bar("Undo") {
            self.sent(what);
        }
    }

    /// The list's cursor (the dial) on the row labeled `label` (or starting so); false when it is not there.
    fn goto(&mut self, label: &str) -> bool {
        let at = |r: &Real| r.screen.ui.global::<crate::PartState>().get_rows().iter().find(|x| x.cursor).map(|x| x.label.to_string());
        if at(self).is_none() {
            self.act(Action::Move { pane: None, by: 1 });
        }
        self.act(Action::End(-1));
        let mut seen = vec![];
        for _ in 0..80 {
            match at(self) {
                Some(l) if l == label || l.starts_with(label) => return true,
                Some(l) => {
                    if seen.last() == Some(&l) && seen.len() > 1 && seen[seen.len() - 2] == l {
                        break;
                    }
                    seen.push(l)
                }
                None => {}
            }
            shell::act(Action::Move { pane: None, by: 1 });
            self.run(30);
        }
        self.note(format!("no row {label:?}; rows seen {seen:?}"));
        false
    }

    /// The row's press (Confirm) after the cursor is put on it.
    fn press(&mut self, label: &str) -> bool {
        if !self.goto(label) {
            return false;
        }
        self.act(Action::Confirm(None));
        true
    }

    fn rows_all(&mut self) -> Vec<(String, String)> {
        let mut all = vec![];
        if self.screen.ui.global::<crate::PartState>().get_rows().iter().all(|x| !x.cursor) {
            self.act(Action::Move { pane: None, by: 1 });
        }
        self.act(Action::End(-1));
        for _ in 0..80 {
            let now = self.screen.ui.global::<crate::PartState>().get_rows().iter().find(|x| x.cursor).map(|x| (x.label.to_string(), x.value.to_string()));
            match now {
                Some(n) if all.last() != Some(&n) => all.push(n),
                _ => break,
            }
            shell::act(Action::Move { pane: None, by: 1 });
            self.run(20);
        }
        self.act(Action::End(-1));
        all
    }

    fn screen_no(&self) -> i32 {
        self.screen.ui.get_screen()
    }

    fn on(&mut self, screen: i32, what: &str) {
        let now = self.screen_no();
        self.check(now == screen, || format!("{what}: the screen is {now}, not {screen}"));
    }

    /// The engine's Performance as far as an edit and its undo should leave it: parts, scenes,
    /// controls, the master and the nodes, without what changes on every commit.
    fn state(&mut self) -> Value {
        let mut g = self.engine.graph();
        let mut nodes = g["nodes"].take();
        for n in nodes.as_array_mut().unwrap() {
            n.as_object_mut().unwrap().remove("parameterRevision");
            n.as_object_mut().unwrap().remove("ringing");
        }
        // User wires by what they join (their IDs change as wires are rebuilt).
        let wires: Vec<Value> = g["wires"].as_array().unwrap().iter().filter(|w| w["owner"] == "user").map(|w| json!([w["kind"], w["from"], w["to"], w["fromPort"], w["toPort"]])).collect();
        json!({"parts": g["parts"], "scenes": g["scenes"], "scene": g["scene"], "controls": g["controls"], "master": g["master"], "nodes": nodes, "userWires": wires})
    }

    /// One edit through the UI: `run` does it as the musician would; the engine's state must change
    /// (when `changes`), the edit must be named for undo, and Undo on the top bar must return it.
    fn edit(&mut self, what: &str, changes: bool, run: impl FnOnce(&mut Real)) -> Value {
        self.step(what);
        let before = self.state();
        let rows = self.rows();
        run(self);
        self.settle();
        let sent = self.sent(what);
        let after = self.state();
        if changes {
            self.check(after != before, || format!("{what}: the engine's state did not change (sent {:?})", sent.iter().map(|s| &s.0).collect::<Vec<_>>()));
        }
        let shown = self.rows();
        if self.screen_no() == 4 || self.screen_no() == 5 {
            eprintln!("   rows changed: {:?}", shown.iter().filter(|r| !rows.contains(r)).collect::<Vec<_>>());
        }
        if changes && after != before {
            diff(&before, &after, "", &mut |d| eprintln!("   engine: {d}"));
            self.undo_ui(what);
            self.run(300);
            let back = self.state();
            if back != before {
                let mut ds = vec![];
                diff(&before, &back, "", &mut |d| ds.push(d));
                self.note(format!("{what}: Undo did not return the state: {}", ds.join("; ")));
            }
            let rows_back = self.rows();
            // The rows shown before that are shown again (the view may have moved) must read as before.
            let stale: Vec<_> = rows_back.iter().filter(|b| rows.iter().any(|a| a.0 == b.0 && a.1 != b.1)).collect();
            if (self.screen_no() == 4 || self.screen_no() == 5) && !stale.is_empty() {
                self.note(format!("{what}: after Undo the rows show {stale:?}, before {:?}", rows.iter().filter(|a| stale.iter().any(|b| b.0 == a.0)).collect::<Vec<_>>()));
            }
        }
        after
    }
}

/// A value row changed with the dial: a press, `by` steps, a press to end.
fn dial(r: &mut Real, label: &str, by: i32) {
    if r.press(label) {
        shell::act(Action::Move { pane: None, by });
        r.run(100);
        r.act(Action::Confirm(None));
    }
}

fn part(r: &mut Real) {
    r.step("Part: open Lead from Play (a tap on its name)");
    r.screen.ui.global::<crate::PlayState>().invoke_part_tapped(0);
    r.settle();
    r.on(4, "Part");
    r.png("10-part-lead");
    r.sent("Part open");
    let rows = r.rows_all();
    eprintln!("   rows {rows:?}");
    let info: Vec<String> = r.screen.ui.global::<crate::PartState>().get_info().iter().map(|l| l.text.to_string()).collect();
    eprintln!("   info {info:?}");
    for (label, by) in [("Level", -3), ("Pan", 2), ("Lowest key", 5), ("Highest key", -5), ("Transpose", 2), ("Input channel", 1), ("Lowest velocity", 10), ("Highest velocity", -10), ("Notes at once", 4), ("Priority", 2), ("Sustain pedal", 1), ("Sostenuto pedal", 1), ("Soft pedal", 2), ("Plays to", 1), ("Send to Bus A", -2), ("Key", 2)] {
        r.edit(&format!("Part: {label} by {by}"), true, |r| dial(r, label, by));
    }
    r.step("Part: MIDI out (a list)");
    if r.press("MIDI out") {
        let list = r.rows_all();
        eprintln!("   MIDI out list {list:?}");
        r.png("13-part-midi-out");
        r.act(Action::Cancel(None)); // EXIT leaves the list in one step
    }
    r.on(4, "Part: after MIDI out");
    r.edit("Part: Mute", true, |r| {
        r.press("Mute");
    });
    r.edit("Part: Solo", true, |r| {
        r.press("Solo");
    });
    r.edit("Part: Plays from: Built in", true, |r| {
        if r.press("Plays from") {
            r.png("11-part-devices");
            let list = r.rows_all();
            eprintln!("   devices list {list:?}");
            r.press("Built in");
            r.act(Action::Cancel(None));
        }
    });
    r.on(4, "Part: after the devices list");
    r.png("12-part-after");
}

/// The first row (of all, the cursor walking them) whose label holds `part`.
fn row_with(r: &mut Real, part: &str) -> Option<String> {
    r.rows_all().into_iter().map(|x| x.0).find(|l| l.contains(part))
}

fn chain(r: &mut Real) {
    r.step("Chain: Lead's Insert effects");
    open_chain(r, 0, "Chain");
    r.png("20-chain-lead");
    r.sent("Chain open");
    let rows = r.rows_all();
    eprintln!("   chain rows {rows:?}");
    let delay = row_with(r, "delay").unwrap_or_default();
    r.step("Chain: delay's actions");
    r.press(&delay);
    r.png("21-chain-actions");
    let actions = r.rows_all();
    eprintln!("   actions {actions:?}");
    r.sent("Chain actions");
    r.edit("Chain: delay Turn off", true, |r| {
        r.press("Turn off");
    });
    eprintln!("   rows now {:?}", r.rows_all());
    r.edit("Chain: delay Move earlier", true, |r| {
        r.press("Move earlier");
    });
    eprintln!("   rows now {:?}", r.rows_all());
    r.edit("Chain: delay Remove", true, |r| {
        r.press("Remove");
    });
    eprintln!("   rows now {:?}", r.rows_all());
    let n = r.state()["parts"][0]["chain"].as_array().map_or(0, |c| c.len());
    r.check(n == 2, || "Chain: after Remove and Undo Lead's chain is not two plugins".into());

    r.step("Chain: delay's Presets...");
    let delay = row_with(r, "delay").unwrap_or_default();
    r.press(&delay);
    r.press("Presets...");
    r.run(1500);
    let mut presets = r.rows_all();
    for _ in 0..8 {
        if !presets.iter().any(|p| p.0 == "Ask again") {
            break;
        }
        r.run(1000);
        r.press("Ask again");
        r.run(500);
        presets = r.rows_all();
    }
    eprintln!("   presets {presets:?}");
    r.png("22-chain-presets");
    r.sent("Chain presets");
    if let Some(first) = presets.iter().map(|p| p.0.clone()).find(|l| !l.is_empty() && l != "No presets listed" && l != "Ask again") {
        r.edit(&format!("Chain: preset {first}"), true, |r| {
            r.press(&first);
            r.run(1500);
        });
    }
    back_until(r, "+ Insert plugin here");
    eprintln!("   screen {} rows {:?}", r.screen_no(), r.rows_all());

    r.step("Chain: insert a plugin after the last");
    let rows = r.rows_all();
    let last = rows.len().saturating_sub(1);
    r.act(Action::End(1));
    let _ = last;
    r.act(Action::Confirm(None));
    r.until(|ui| ui.global::<crate::PartState>().get_rows().iter().all(|x| x.label != "Finding plugins..."));
    r.settle();
    let plugins = r.rows_all();
    eprintln!("   plugins {plugins:?}");
    r.png("23-chain-insert");
    r.sent("Chain insert list");
    if let Some(name) = plugins.iter().map(|p| p.0.clone()).find(|l| l.contains("Transpose") || l.contains("Delay")) {
        r.edit(&format!("Chain: insert {name}"), true, |r| {
            r.press(&name);
            r.run(800);
        });
    } else {
        r.note(format!("Chain insert: no Delay or Transpose among {plugins:?}"));
    }
    eprintln!("   rows now {:?}", r.rows_all());

    r.step("Chain: synth's Parameters");
    let synth = row_with(r, "synth").unwrap_or_default();
    r.press(&synth);
    r.press("Parameters");
    let node = r.state()["parts"][0]["chain"][0].as_u64().unwrap_or(0);
    params_page(r, node, "Basic Synth");
    r.act(Action::Cancel(None)); // the parameter page's search or list first, if open
    if r.screen_no() == 7 {
        r.act(Action::Cancel(None));
    }
    r.on(5, "back from the parameters");
    back_until(r, "+ Insert plugin here");
    eprintln!("   screen {} rows {:?}", r.screen_no(), r.rows_all());
}

/// EXIT, one step each, until the first rows shown are the ones starting `label` (at most five steps).
fn back_until(r: &mut Real, label: &str) {
    for _ in 0..5 {
        if r.rows().iter().any(|x| x.0.starts_with(label)) {
            return;
        }
        r.act(Action::Cancel(None));
    }
    r.note(format!("back: never reached {label:?}; rows {:?}", r.rows()));
}

/// From Play, the Part screen of the strip `strip`, then its Insert effects.
fn open_chain(r: &mut Real, strip: i32, what: &str) {
    for _ in 0..4 {
        if r.screen_no() == 3 {
            break;
        }
        r.act(Action::Cancel(None));
    }
    r.screen.ui.global::<crate::PlayState>().invoke_part_tapped(strip);
    r.settle();
    r.on(4, what);
    r.press("Insert effects");
    r.on(5, what);
}

fn splits(r: &mut Real) {
    r.step("Split: the Split part's chain");
    open_chain(r, 2, "Split's chain");
    let rows = r.rows_all();
    eprintln!("   rows {rows:?}");
    r.png("40-split-chain");
    let split = rows.iter().map(|x| x.0.clone()).find(|l| l.contains("Split") || l.contains("Layers")).unwrap_or_default();
    r.press(&split);
    let lanes = r.rows_all();
    eprintln!("   split rows {lanes:?}");
    r.png("41-split-lanes");
    r.sent("Split open");
    r.edit("Split: Add lane", true, |r| {
        r.press("Add lane");
    });
    let lane = r.rows_all().into_iter().map(|x| x.0).find(|l| l.starts_with("Lane") || l.contains("Low")).unwrap_or_default();
    r.step("Split: the first lane");
    r.press(&lane);
    let lane_rows = r.rows_all();
    eprintln!("   lane rows {lane_rows:?}");
    r.png("42-lane");
    r.sent("Lane open");
    for (label, by) in [("Lowest key", 2), ("Highest key", -2), ("Lane level", -3), ("Lane pan", 2)] {
        r.edit(&format!("Lane: {label} by {by}"), true, |r| dial(r, label, by));
    }
    r.edit("Lane: Mute", true, |r| {
        r.press("Mute");
    });
    r.edit("Lane: Solo", true, |r| {
        r.press("Solo");
    });
    let plugin = r.rows_all().into_iter().map(|x| x.0).find(|l| l.starts_with("1.")).unwrap_or_default();
    r.step("Lane: its plugin's actions");
    r.press(&plugin);
    eprintln!("   actions {:?}", r.rows_all());
    r.edit("Lane: plugin Turn off", true, |r| {
        r.press("Turn off");
    });
    r.act(Action::Cancel(None)); // the plugin's actions: back to the lane
    eprintln!("   back to {:?}", r.rows_all().first());
    r.edit("Lane: insert Delay after its plugin", true, |r| {
        r.act(Action::End(-1));
        let rows = r.rows_all();
        if let Some(k) = rows.iter().position(|x| x.0.starts_with("1.")) {
            r.act(Action::End(-1));
            r.act(Action::Move { pane: None, by: k as i32 + 1 });
            r.act(Action::Confirm(None));
            r.until(|ui| ui.global::<crate::PartState>().get_rows().iter().any(|x| x.label == "Delay"));
            r.press("Delay");
            r.run(800);
        }
    });
    r.edit("Lane: Remove lane", true, |r| {
        r.press("Remove lane");
    });
    eprintln!("   now {} {:?}", r.screen_no(), r.rows_all());
    r.edit("Split: Remove split", true, |r| {
        if !r.rows_all().iter().any(|x| x.0 == "Remove split") {
            r.act(Action::Cancel(None)); // the lane's list: back to the split's
        }
        r.press("Remove split");
    });
    eprintln!("   now {} {:?}", r.screen_no(), r.rows_all());

    r.step("Split: a new split on the Empty part");
    open_chain(r, 3, "Empty's chain");
    eprintln!("   rows {:?}", r.rows_all());
    for kind in ["Split here: layers", "Split here: left / right", "Split here: mid / side", "Split here: frequency bands"] {
        r.edit(&format!("Empty: {kind}"), true, |r| {
            r.press("+ Insert plugin here");
            r.until(|ui| ui.global::<crate::PartState>().get_rows().iter().all(|x| x.label != "Finding plugins..."));
            r.press(kind);
            r.run(1500);
            eprintln!("   rows {:?}", r.rows_all());
            if kind.ends_with("bands") {
                r.png("43-bands");
                let split = r.rows_all().into_iter().map(|x| x.0).find(|l| l.contains("Split") || l.contains("band")).unwrap_or_default();
                r.press(&split);
                r.run(500);
                let rows = r.rows_all();
                eprintln!("   band split rows {rows:?}");
                r.png("44-bands-split");
            }
        });
        for _ in 0..3 {
            if r.rows_all().iter().any(|x| x.0 == "+ Insert plugin here") && r.screen_no() == 5 {
                break;
            }
            r.act(Action::Cancel(None));
        }
    }
}

/// Back to Play from wherever the last section ended.
fn to_play(r: &mut Real) {
    for _ in 0..8 {
        if r.screen_no() == 3 {
            return;
        }
        if r.screen.ui.global::<crate::KeyboardState>().get_open() {
            r.screen.ui.global::<crate::KeyboardState>().invoke_cancel();
        } else if r.screen.ui.global::<crate::MenuState>().get_open() {
            r.screen.ui.global::<crate::MenuState>().invoke_close();
        } else if r.screen_no() == 0 {
            r.screen.ui.global::<HomeState>().invoke_tapped(0);
        } else {
            shell::act(Action::Cancel(None));
        }
        r.settle();
    }
    r.on(3, "back to Play");
}

/// Types `text` on the on-screen keyboard open now (its keys by label; a space by the space key), then Done.
fn type_text(r: &mut Real, text: &str) {
    let ui = r.screen.ui.clone_strong();
    let k = ui.global::<crate::KeyboardState>();
    if !k.get_open() {
        r.note(format!("type {text:?}: no keyboard open"));
        return;
    }
    let keys = || k.get_keys().iter().map(|x| (x.label.to_string(), x.index)).collect::<Vec<_>>();
    // Clear what is there first.
    if let Some(c) = keys().iter().find(|x| x.0 == "Clear") {
        k.invoke_pressed(c.1);
        k.invoke_released();
    }
    for ch in text.chars() {
        let want = if ch == ' ' { "space".to_string() } else { ch.to_lowercase().to_string() };
        let found = keys().into_iter().find(|x| x.0.to_lowercase() == want || (ch == ' ' && x.0.trim().is_empty()));
        match found {
            Some(key) => {
                if ch.is_uppercase() {
                    if let Some(s) = keys().iter().find(|x| x.0 == "Shift" || x.0 == "⇧") {
                        k.invoke_pressed(s.1);
                        k.invoke_released();
                    }
                }
                k.invoke_pressed(key.1);
                k.invoke_released();
            }
            None => r.note(format!("type {text:?}: no key for {ch:?} among {:?}", keys().iter().map(|x| x.0.clone()).collect::<Vec<_>>())),
        }
    }
    eprintln!("   typed: {:?}", format!("{}{}", k.get_before(), k.get_after()));
    match keys().into_iter().find(|x| x.0 == "Done") {
        Some(d) => {
            k.invoke_pressed(d.1);
            k.invoke_released();
        }
        None => r.note("type: no Done key".into()),
    }
    r.settle();
}

fn menu_rows(r: &Real) -> Vec<(String, i32, bool)> {
    r.screen.ui.global::<crate::MenuState>().get_rows().iter().map(|m| (m.label.to_string(), m.tick, m.enabled)).collect()
}

/// A menu row by its label (in the view shown), tapped; false when not there.
fn menu_tap(r: &mut Real, label: &str) -> bool {
    let rows = menu_rows(r);
    match rows.iter().position(|m| m.0 == label || m.0.starts_with(label)) {
        Some(i) => {
            r.screen.ui.global::<crate::MenuState>().invoke_tapped(i as i32);
            r.settle();
            true
        }
        None => {
            r.note(format!("menu: no {label:?} among {rows:?}"));
            false
        }
    }
}

/// The parameter pages of the plugin at chain row `row` of the strip `strip`'s chain (a lane's when `lane` names it).
fn plugin_page(r: &mut Real, strip: i32, path: &[&str], node_name: &str) {
    to_play(r);
    open_chain(r, strip, node_name);
    for p in path {
        let label = r.rows_all().into_iter().map(|x| x.0).find(|l| l.contains(p)).unwrap_or_default();
        r.press(&label);
    }
    r.press("Parameters");
    let g = r.engine.graph();
    let node = g["nodes"].as_array().unwrap().iter().find(|n| n["name"] == node_name).and_then(|n| n["id"].as_u64()).unwrap_or(0);
    params_page(r, node, node_name);
}

fn plugin_pages(r: &mut Real) {
    plugin_page(r, 0, &["delay"], "delay");
    plugin_page(r, 1, &["sequencer"], "sequencer");
    plugin_page(r, 2, &["Split", "Lane 2", "Transpose"], "Transpose");
    plugin_page(r, 4, &["Bus Echo"], "Bus Echo");
}

fn sounds(r: &mut Real) {
    r.step("Sounds: Lead's Sound row");
    r.screen.ui.global::<crate::PlayState>().invoke_part_tapped(0);
    r.settle();
    r.press("Sound");
    r.on(6, "Sounds");
    r.run(500);
    let rows = r.rows_all();
    eprintln!("   rows {rows:?}");
    r.png("50-sounds");
    r.sent("Sounds open");
    // With no Sound in the library the list offers saving the part as the first one.
    r.check(rows.iter().any(|x| x.0 == "Save this part as the first Sound"), || format!("Sounds: an empty library offers no way to save the part as a Sound from this screen: {rows:?}"));
    r.edit("Sounds: Save this part as the first Sound", false, |r| {
        r.press("Save this part as the first Sound");
        type_text(r, "Check Lead");
        r.run(1500);
    });
    let found = r.engine.call("library.query", json!({"kind": "sound"}));
    r.check(found["items"].as_array().is_some_and(|i| i.iter().any(|x| x["name"] == "Check Lead")), || format!("Sounds: the first Sound is not in the library: {}", found["total"]));
    r.act(Action::Cancel(None)); // EXIT: back to the Part screen
    r.press("Sound");
    r.on(6, "Sounds again");
    r.run(500);
    let rows = r.rows_all();
    eprintln!("   rows {rows:?}");
    r.png("51-sounds-saved");
    if let Some(sound) = rows.iter().map(|x| x.0.clone()).find(|l| l.contains("Check Lead")) {
        r.step("Sounds: its actions");
        sounds_list(r);
        r.press(&sound);
        let actions = r.rows_all();
        eprintln!("   actions {actions:?}");
        r.png("52-sound-actions");
        r.step("Sounds: audition");
        r.press("Audition in this part");
        r.run(800);
        r.sent("audition");
        let a = r.engine.call("audition.get", json!({}));
        eprintln!("   audition.get {a}");
        r.press("Stop auditioning");
        r.run(800);
        r.sent("audition stop");
        r.step("Sounds: save the part over it, then as a new Sound");
        sounds_list(r);
        r.press(&sound);
        let save = r.rows_all().into_iter().map(|x| x.0).find(|l| l.starts_with("Save this part over") || l == "Save this part as a Sound").unwrap_or_default();
        r.press(&save);
        r.run(1000);
        r.sent("save over");
        sounds_list(r);
        r.press(&sound);
        r.press("Save this part as a new Sound");
        type_text(r, "Check Two");
        r.run(1000);
        r.sent("save new");
        let found = r.engine.call("library.query", json!({"kind": "sound"}));
        let names: Vec<Value> = found["items"].as_array().unwrap().iter().map(|i| i["name"].clone()).collect();
        eprintln!("   the library's Sounds {names:?}");
        r.check(names.iter().any(|n| n == "Check Two"), || format!("Sounds save new: not in the library: {names:?}"));
        r.step("Sounds: favorite");
        sounds_list(r);
        r.press(&sound);
        r.press("Add to favorites");
        r.sent("favorite");
        let fav = r.engine.call("library.query", json!({"kind": "sound", "favorites": true}));
        r.check(fav["items"].as_array().is_some_and(|i| i.iter().any(|x| x["name"] == "Check Lead")), || format!("Sounds favorite: the library says {}", fav["total"]));
        r.edit("Sounds: Load into this part", true, |r| {
            sounds_list(r);
        r.press(&sound);
            r.press("Load into this part");
            r.run(1500);
        });
        r.edit("Sounds: New part from it", true, |r| {
            sounds_list(r);
        r.press(&sound);
            r.press("New part from it");
            r.run(1500);
        });
    }
    r.step("Sounds: filters");
    sounds_list(r);
    if r.press("Filters") {
        let f = r.rows_all();
        eprintln!("   filters {f:?}");
        r.press("Favorites");
        r.run(500);
        r.sent("filter favorites");
        r.png("53-sounds-filters");
        r.press("Favorites");
        r.act(Action::Cancel(None)); // the filters' list: back to the Sounds
    }
    r.step("Sounds: search");
    if r.press("Search") {
        type_text(r, "che");
        r.run(500);
        r.sent("search");
        eprintln!("   rows {:?}", r.rows_all());
    }
}

/// The sound chooser's list (Search, Filters, the Sounds), from its actions or filters.
fn sounds_list(r: &mut Real) {
    for _ in 0..3 {
        if r.rows().iter().any(|x| x.0 == "Search") {
            return;
        }
        r.act(Action::Cancel(None));
    }
}

impl Real {
    /// The list's cursor dropped, so a press starts from the first row.
    fn rows_top(&mut self) {
        self.act(Action::End(-1));
    }
}

fn scene(r: &mut Real) {
    // After the undone + Scene the engine's `scene` is 0 (none): the Scene action is off then, and a tap on a scene turns it on.
    let none = r.engine.graph()["scene"] == 0;
    let enabled = |r: &Real| r.screen.ui.global::<Bar>().get_actions().iter().any(|a| a.label == "Scene" && a.enabled);
    eprintln!("   engine scene none: {none}, Scene action enabled: {}", enabled(r));
    let on = enabled(r);
    r.check(none != on, || format!("Scene: the engine's scene is {} and the action is {}", if none { "none" } else { "there" }, if on { "on" } else { "off" }));
    r.screen.ui.global::<crate::PlayState>().invoke_scene_tapped(0);
    r.settle();
    r.step("Scene: the top bar's Scene");
    r.bar("Scene");
    r.on(8, "Scene");
    let rows = r.rows_all();
    eprintln!("   rows {rows:?}");
    r.png("60-scene");
    r.sent("Scene open");
    // A change to store first: the Split part's level, live, before the step (it is an undo step of its own).
    r.engine.call("parts.level", json!({"part": 3, "level": 0.3, "mute": false}));
    r.run(300);
    r.edit("Scene: Store", true, |r| {
        r.press("Store");
    });
    r.engine.call("parts.level", json!({"part": 3, "level": 1.0, "mute": false}));
    r.edit("Scene: Rename", true, |r| {
        r.press("Rename");
        type_text(r, "Intro");
    });
    for row in ["Plugins and wires on or off", "Mutes and solos", "Parameters and controls", "Control mappings", "Part note zones"] {
        r.edit(&format!("Scene: scope {row}"), true, |r| {
            r.press(row);
        });
    }
    r.edit("Scene: Glide time", true, |r| dial(r, "Glide time", 3));
    // A glide kept (the curve needs one), set through the screen, then the curve and the launch.
    dial(r, "Glide time", 2);
    r.edit("Scene: Curve", true, |r| dial(r, "Curve", 1));
    r.edit("Scene: While playing", true, |r| dial(r, "While playing", 1));
    r.undo_ui("Scene: the glide kept");
}

fn parts_list(r: &mut Real) {
    r.step("Parts list: Edit on Play's top bar");
    r.bar("Edit");
    r.on(9, "Parts list");
    r.run(400);
    let ui = r.screen.ui.clone_strong();
    let p = ui.global::<crate::PartsState>();
    eprintln!("   rows {:?}\n   tools {:?}\n   buses {:?}", ui.global::<crate::PartsState>().get_rows().iter().map(|x| format!("{} | {} | {} | {}", x.name, x.sound, x.input, x.chain_text)).collect::<Vec<_>>(), p.get_tools().iter().map(|t| (t.label.to_string(), t.enabled)).collect::<Vec<_>>(), p.get_bus_names().iter().map(|s| s.to_string()).collect::<Vec<_>>());
    r.png("70-parts");
    r.sent("Parts open");
    r.edit("Parts list: Lead's level slider", true, |_| {
        p.invoke_held(0, 1, true);
        p.invoke_level(0, 0.4);
        p.invoke_held(0, 1, false);
    });
    r.edit("Parts list: Split's M", true, |_| p.invoke_mute(2));
    r.edit("Parts list: Lead's send to Bus A", true, |_| {
        p.invoke_held(0, 3, true);
        p.invoke_send(0, 0, 0.2);
        p.invoke_held(0, 3, false);
    });
    r.edit("Parts list: Split's send to Bus A (a new send)", true, |_| {
        p.invoke_held(2, 3, true);
        p.invoke_send(2, 0, 0.6);
        p.invoke_held(2, 3, false);
    });
    let tools: Vec<String> = p.get_tools().iter().map(|t| t.label.to_string()).collect();
    let add_part = tools.iter().position(|t| t.contains("Part")).unwrap_or(3) as i32;
    let add_bus = tools.iter().position(|t| t.contains("Bus")).unwrap_or(4) as i32;
    r.edit("Parts list: + Part", true, |_| p.invoke_tool(add_part));
    r.edit("Parts list: + Bus", true, |_| p.invoke_tool(add_bus));
    r.step("Parts list: a tap on Groovebox's words opens its Part screen");
    p.invoke_open(1);
    r.settle();
    r.on(4, "Parts list open");
    let name = r.screen.ui.global::<crate::PartState>().get_name();
    r.check(name == "Groovebox", || format!("Parts list open: the Part screen is {name:?}"));
    r.act(Action::Cancel(None)); // no cursor: back to where the Part screen was opened from
    r.on(9, "back to the Parts list");
    // Name and Remove part on the Empty part.
    p.invoke_open(3);
    r.settle();
    r.edit("Part: Name", true, |r| {
        r.press("Name");
        type_text(r, "Pad");
    });
    r.edit("Part: Remove part (two presses)", true, |r| {
        r.press("Remove part");
        r.act(Action::Confirm(None));
    });
    r.on(9, "after Remove part");
}

fn mixer(r: &mut Real) {
    r.bar("Edit");
    r.step("Mixer: from the Parts list");
    let tools: Vec<String> = r.screen.ui.global::<crate::PartsState>().get_tools().iter().map(|t| t.label.to_string()).collect();
    let i = tools.iter().position(|t| t.contains("Mixer")).unwrap_or(0) as i32;
    r.screen.ui.global::<crate::PartsState>().invoke_tool(i);
    r.settle();
    r.on(10, "Mixer");
    let ui = r.screen.ui.clone_strong();
    let m = ui.global::<crate::MixerState>();
    eprintln!("   strips {:?} sends {:?}", m.get_strips().iter().map(|s| format!("{} {} pan {} sends {:?}", s.name, s.db, s.pan, s.sends.iter().collect::<Vec<_>>())).collect::<Vec<_>>(), m.get_send_names().iter().map(|s| s.to_string()).collect::<Vec<_>>());
    r.png("80-mixer");
    r.sent("Mixer open");
    r.edit("Mixer: Lead's fader", true, |_| {
        m.invoke_held(0, 0, true);
        m.invoke_level(0, 0.6);
        m.invoke_held(0, 0, false);
    });
    r.edit("Mixer: Lead's pan", true, |_| {
        m.invoke_held(0, 3, true);
        m.invoke_pan(0, 0.8);
        m.invoke_held(0, 3, false);
    });
    r.edit("Mixer: Groovebox M", true, |_| m.invoke_mute(1));
    r.edit("Mixer: Split S", true, |_| m.invoke_solo(2));
    r.edit("Mixer: Lead's send", true, |_| {
        m.invoke_held(0, 4, true);
        m.invoke_send(0, 0, 0.9);
        m.invoke_held(0, 4, false);
    });
    r.edit("Mixer: the master's fader", true, |_| {
        m.invoke_held(-1, 0, true);
        m.invoke_level(-1, 0.3);
        m.invoke_held(-1, 0, false);
    });
    r.edit("Mixer: the dial on a fader", true, |r| {
        r.act(Action::Move { pane: None, by: 1 });
        r.act(Action::Confirm(None));
        r.act(Action::Move { pane: None, by: -4 });
        r.act(Action::Confirm(None));
    });
    r.act(Action::Cancel(None));
}

fn controls(r: &mut Real) {
    r.bar("Edit");
    r.step("Controls: from the Parts list");
    let tools: Vec<String> = r.screen.ui.global::<crate::PartsState>().get_tools().iter().map(|t| t.label.to_string()).collect();
    let i = tools.iter().position(|t| t.contains("Controls")).unwrap_or(1) as i32;
    r.screen.ui.global::<crate::PartsState>().invoke_tool(i);
    r.settle();
    r.on(11, "Controls");
    let ui = r.screen.ui.clone_strong();
    let c = ui.global::<crate::ControlsState>();
    let rows = || ui.global::<crate::ControlsState>().get_rows().iter().map(|x| format!("{} {} {} {}..{} {} rel {}", x.kind, x.name, x.value, x.start, x.end, x.curve, x.relative)).collect::<Vec<_>>();
    eprintln!("   rows {:?}\n   tools {:?}", rows(), c.get_tools().iter().map(|t| t.label.to_string()).collect::<Vec<_>>());
    r.png("90-controls");
    r.sent("Controls open");
    r.step("Controls: the Macro's fader");
    c.invoke_held(0, 1, true);
    c.invoke_level(0, 0.6);
    c.invoke_held(0, 1, false);
    r.settle();
    r.sent("Controls fader");
    let g = r.engine.graph();
    r.check((g["controls"][0]["value"].as_f64().unwrap_or(-1.) - 0.6).abs() < 0.02, || format!("Controls fader: the engine's control is {}", g["controls"][0]["value"]));
    r.edit("Controls: the first target's end", true, |_| {
        c.invoke_held(1, 1, true);
        c.invoke_range(1, true, 0.5);
        c.invoke_held(1, 1, false);
    });
    r.edit("Controls: the first target's Relative", true, |_| {
        c.invoke_press(1, 3);
    });
    r.edit("Controls: the first target's curve", true, |r| {
        r.screen.ui.global::<crate::ControlsState>().invoke_press(1, 2);
        r.settle();
        eprintln!("   menu {:?}", menu_rows(r));
        menu_tap(r, "Exponential");
    });
    r.edit("Controls: + Target, a part's level", true, |r| {
        r.screen.ui.global::<crate::ControlsState>().invoke_press(0, 2);
        r.settle();
        let m = menu_rows(r);
        eprintln!("   menu {m:?}");
        r.png("91-controls-target-menu");
        if let Some(first) = m.iter().find(|x| x.2 && x.0.contains("level")).map(|x| x.0.clone()) {
            menu_tap(r, &first);
            let m = menu_rows(r);
            eprintln!("   submenu {m:?}");
            if let Some(next) = m.iter().find(|x| x.2).map(|x| x.0.clone()) {
                menu_tap(r, &next);
            }
        }
    });
    r.rows_top();
    r.edit("Controls: + Control", true, |r| {
        let tools: Vec<String> = r.screen.ui.global::<crate::ControlsState>().get_tools().iter().map(|t| t.label.to_string()).collect();
        let i = tools.iter().position(|t| t.contains("Control")).unwrap_or(1) as i32;
        r.screen.ui.global::<crate::ControlsState>().invoke_tool(i);
        r.settle();
        let names: Vec<String> = r.screen.ui.global::<crate::ControlsState>().get_rows().iter().filter(|x| x.kind == 0).map(|x| x.name.to_string()).collect();
        eprintln!("   controls shown {names:?}");
        r.check(names.len() == 2, || format!("Controls + Control: the page shows {names:?}"));
    });
    r.edit("Controls: rename the Macro", true, |r| {
        r.screen.ui.global::<crate::ControlsState>().invoke_press(0, 0);
        r.settle();
        type_text(r, "Swell");
        let first = r.screen.ui.global::<crate::ControlsState>().get_rows().iter().next().map(|x| x.name.to_string()).unwrap_or_default();
        r.check(first == "Swell", || format!("Controls rename: the page shows {first:?}"));
    });
    r.step("Controls: By scene");
    let tools: Vec<String> = c.get_tools().iter().map(|t| t.label.to_string()).collect();
    if let Some(i) = tools.iter().position(|t| t.contains("scene")) {
        c.invoke_tool(i as i32);
        r.settle();
        r.png("92-controls-by-scene");
        eprintln!("   table {:?}", c.get_table_cells().iter().map(|x| x.on).collect::<Vec<_>>());
        r.edit("Controls: a mapping off in Chorus", true, |r| r.screen.ui.global::<crate::ControlsState>().invoke_table_press(0, 1));
        c.invoke_tool(i as i32);
        r.settle();
    }
    r.edit("Controls: Remove the first target", true, |r| {
        r.screen.ui.global::<crate::ControlsState>().invoke_press(1, 6);
    });
}

fn routing(r: &mut Real) {
    r.bar("Edit");
    r.step("Routing: from the Parts list");
    let tools: Vec<String> = r.screen.ui.global::<crate::PartsState>().get_tools().iter().map(|t| t.label.to_string()).collect();
    let i = tools.iter().position(|t| t.contains("Routing")).unwrap_or(2) as i32;
    r.screen.ui.global::<crate::PartsState>().invoke_tool(i);
    r.settle();
    r.on(12, "Routing");
    let ui = r.screen.ui.clone_strong();
    let rt = ui.global::<crate::RoutingState>();
    let boxes = || ui.global::<crate::RoutingState>().get_boxes().iter().map(|b| b.name.to_string()).collect::<Vec<_>>();
    eprintln!("   frames {:?}\n   boxes {:?}\n   wires {}\n   tools {:?}", rt.get_frames().iter().map(|b| b.name.to_string()).collect::<Vec<_>>(), boxes(), rt.get_wires().iter().count(), ui.global::<crate::Bar>().get_actions().iter().map(|t| t.label.to_string()).collect::<Vec<_>>());
    r.png("A0-routing");
    r.sent("Routing open");
    let panel = |ui: &Shell| ui.global::<crate::RoutingState>().get_panel().iter().map(|p| format!("{}:{} [{}]", p.kind, p.text, p.cells.iter().map(|c| c.label.to_string()).collect::<Vec<_>>().join("|"))).collect::<Vec<_>>();
    // Expand the parts, so their plugins are boxes.
    let tools: Vec<String> = ui.global::<crate::Bar>().get_actions().iter().map(|t| t.label.to_string()).collect();
    // The choice is kept (routing-view.txt): switched only when the parts are collapsed now.
    if let Some(i) = tools.iter().position(|t| t.contains("collapsed")).filter(|_| !boxes().iter().any(|b| b == "delay")) {
        ui.global::<crate::Bar>().invoke_action(i as i32); // the top bar's
        r.settle();
        eprintln!("   boxes after expanding {:?}", boxes());
    }
    if let Some(i) = boxes().iter().position(|b| b == "delay") {
        r.step("Routing: choose delay");
        rt.invoke_tapped(1, i as i32);
        r.settle();
        let p = panel(&ui);
        eprintln!("   panel {p:?}");
        r.png("A1-routing-delay");
        r.sent("Routing choose");
        // A cell by its label: (row, cell).
        let find = |ui: &Shell, label: &str| {
            ui.global::<crate::RoutingState>().get_panel().iter().enumerate().find_map(|(row, p)| p.cells.iter().position(|c| c.label.starts_with(label)).map(|c| (row as i32, c as i32)))
        };
        if let Some((row, cell)) = find(&ui, "Turn off") {
            r.edit("Routing: delay Turn off", true, |r| r.screen.ui.global::<crate::RoutingState>().invoke_panel_press(row, cell));
        } else {
            r.note(format!("Routing: no Turn off in {p:?}"));
        }
        if let Some((row, cell)) = find(&ui, "Connect audio to") {
            r.edit("Routing: Connect audio to Bus Echo", true, |r| {
                r.screen.ui.global::<crate::RoutingState>().invoke_panel_press(row, cell);
                r.settle();
                let m = menu_rows(r);
                eprintln!("   menu {m:?}");
                if let Some(t) = m.iter().find(|x| x.0.contains("Bus Echo") && x.2).map(|x| x.0.clone()) {
                    menu_tap(r, &t);
                    let m = menu_rows(r);
                    if r.screen.ui.global::<crate::MenuState>().get_open() {
                        eprintln!("   submenu {m:?}");
                        if let Some(t) = m.iter().find(|x| x.2).map(|x| x.0.clone()) {
                            menu_tap(r, &t);
                        }
                    }
                }
            });
        }
    } else {
        r.note(format!("Routing: no delay box among {:?}", boxes()));
    }
    let tools: Vec<String> = ui.global::<crate::Bar>().get_actions().iter().map(|t| t.label.to_string()).collect();
    if let Some(i) = tools.iter().position(|t| t.contains("Meters")) {
        r.step("Routing: Meters on");
        ui.global::<crate::Bar>().invoke_action(i as i32); // the top bar's
        r.run(600);
        let sent = r.since(r.mark);
        r.check(sent.iter().any(|s| s.0 == "wires.meters"), || "Routing Meters: wires.meters never asked".into());
        r.sent("Meters");
        ui.global::<crate::Bar>().invoke_action(i as i32); // the top bar's
        r.settle();
    }
}

fn top_bar(r: &mut Real) {
    r.step("Top bar: an edit, Undo, Redo");
    r.screen.ui.global::<crate::PlayState>().invoke_mute_tapped(1);
    r.settle();
    r.sent("mute");
    r.undo_ui("Top bar");
    r.step("Top bar: Redo");
    let before = r.state();
    r.bar("Redo");
    r.sent("Redo");
    let after = r.state();
    r.check(after != before, || "Top bar Redo: the engine's state did not change".into());
    r.check(part_of(&after, "Groovebox")["mute"] == true, || "Top bar Redo: Groovebox is not muted again".into());
    r.undo_ui("Top bar again");
    r.step("Top bar: Save");
    let saved_before = r.engine.call("performance.info", json!({}));
    r.engine.call("parts.level", json!({"part": 3, "level": 0.9, "mute": false}));
    r.run(300);
    let unsaved = r.engine.call("performance.info", json!({}));
    r.bar("Save");
    r.run(800);
    r.sent("Save");
    let after = r.engine.call("performance.info", json!({}));
    eprintln!("   info before {saved_before}\n   edited {unsaved}\n   after {after}");
    r.check(after["unsaved"] == false && after["id"] == saved_before["id"], || format!("Top bar Save: {after}"));
    r.check(!r.screen.ui.global::<Bar>().get_notice().is_empty(), || "Top bar Save: nothing said".into());
    eprintln!("   notice {:?}", r.screen.ui.global::<Bar>().get_notice());
    r.engine.call("parts.level", json!({"part": 3, "level": 1.0, "mute": false}));
    r.step("Top bar: Save As New (the name's pill)");
    r.screen.ui.global::<Bar>().invoke_pill_tapped();
    r.settle();
    type_text(r, "Check Copy");
    r.run(800);
    r.sent("Save As New");
    let info = r.engine.call("performance.info", json!({}));
    r.check(info["name"] == "Check Copy" && info["id"] != saved_before["id"], || format!("Save As New: {info}"));
    let shown = r.screen.ui.global::<Bar>().get_performance();
    r.check(shown.contains("Check Copy"), || format!("Save As New: the bar says {shown:?}"));
    r.png("B0-bar-saved-as-new");
    // Back to the check's own Performance for what follows.
    let found = r.engine.call("library.query", json!({"text": "Real Engine Check", "kind": "performance"}));
    if let Some(id) = found["items"][0]["id"].as_str() {
        r.engine.call("library.load", json!({"id": id}));
    }
    r.run(800);
    let shown = r.screen.ui.global::<Bar>().get_performance();
    r.check(shown.contains("Real Engine Check"), || format!("after library.load by another client the bar says {shown:?}"));
    r.step("Top bar: Learn");
    if r.bar("Learn") {
        r.png("B1-learn");
        let ps = r.screen.ui.global::<crate::PlayState>();
        eprintln!("   learning {} text {:?}", ps.get_learning(), ps.get_learn_text());
        r.sent("Learn");
        r.bar("Learn");
    }
    r.step("Top bar: a take recorded (record.start by another client)");
    let take = r.engine.call("record.start", json!({"name": "check"}));
    r.until(|ui| ui.global::<Bar>().get_recording());
    r.run(1300);
    let bar = r.screen.ui.global::<Bar>();
    eprintln!("   recording {} time {:?} behind {}", bar.get_recording(), bar.get_recording_time(), bar.get_recording_behind());
    r.check(bar.get_recording(), || "record: the top bar shows no recording".into());
    r.png("B2-recording");
    r.engine.call("record.stop", json!({"take": take["take"]}));
    let gone = r.until(|ui| !ui.global::<Bar>().get_recording());
    r.check(gone, || "record: the mark stays after the take stopped".into());
    let takes = r.engine.call("record.takes", json!({}));
    eprintln!("   takes {}", takes["takes"].as_array().map(|t| t.iter().map(|x| format!("{} {} {}", x["take"], x["state"], x["folder"])).collect::<Vec<_>>().join("; ")).unwrap_or_default());
}

fn tiles(ui: &Shell) -> Vec<(String, String, i32, f32)> {
    ui.global::<crate::GridState>().get_tiles().iter().map(|t| (t.name.to_string(), t.value.to_string(), t.kind, t.norm)).collect()
}

/// The parameter values of `node`, by id.
fn values(r: &mut Real, node: u64) -> Vec<(u64, f64)> {
    let list = r.engine.call("params.list", json!({"node": node}));
    let ids: Vec<Value> = list["parameters"].as_array().unwrap().iter().map(|p| json!({"node": node, "param": p["id"]})).collect();
    let mut out = vec![];
    for chunk in ids.chunks(128) {
        // params.get takes at most 128 addresses
        let got = r.engine.call("params.get", json!({"values": chunk}));
        out.extend(got["values"].as_array().unwrap().iter().map(|v| (v["param"].as_u64().unwrap(), v["value"].as_f64().unwrap())));
    }
    out
}

/// One parameter edit on the page shown: the engine's value must move, the tile must show it, Undo must return it.
fn param_edit(r: &mut Real, node: u64, what: &str, run: impl FnOnce(&mut Real)) {
    r.step(what);
    let before = values(r, node);
    let shown = tiles(&r.screen.ui);
    run(r);
    r.settle();
    r.sent(what);
    let after = values(r, node);
    let moved: Vec<_> = before.iter().zip(&after).filter(|(a, b)| (a.1 - b.1).abs() > 1e-9).map(|(a, b)| (a.0, a.1, b.1)).collect();
    eprintln!("   engine moved {moved:?}");
    r.check(!moved.is_empty(), || format!("{what}: no parameter of node {node} moved"));
    let now = tiles(&r.screen.ui);
    let changed: Vec<_> = now.iter().filter(|t| !shown.contains(t)).map(|t| (t.0.clone(), t.1.clone())).collect();
    eprintln!("   tiles changed {changed:?}");
    r.check(moved.is_empty() || !changed.is_empty(), || format!("{what}: the engine moved {moved:?} but no tile changed"));
    if !moved.is_empty() {
        r.undo_ui(what);
        r.run(300);
        let back = values(r, node);
        r.check(back == before, || format!("{what}: Undo did not return the values: {:?}", before.iter().zip(&back).filter(|(a, b)| a != b).collect::<Vec<_>>()));
        let shown_back = tiles(&r.screen.ui);
        let stale: Vec<_> = shown_back.iter().filter(|t| shown.iter().any(|s| s.0 == t.0 && s.1 != t.1)).map(|t| (t.0.clone(), t.1.clone())).collect();
        r.check(stale.is_empty(), || format!("{what}: after Undo the tiles show {stale:?}"));
    }
}

/// The parameter page shown: every page and tab opened, one edit of each kind of tile, a view dragged.
fn params_page(r: &mut Real, node: u64, name: &str) {
    r.on(7, &format!("Parameters of {name}"));
    r.run(500);
    let ui = r.screen.ui.clone_strong();
    let grid = ui.global::<crate::GridState>();
    let pages: Vec<String> = grid.get_pages().iter().map(|p| p.to_string()).collect();
    let tabs: Vec<String> = grid.get_tabs().iter().map(|p| p.to_string()).collect();
    let panels = grid.get_panels().iter().count();
    eprintln!("   {name}: pages {pages:?} tabs {tabs:?} views {panels} search {} message {:?}\n   tiles {:?}", grid.get_search_shown(), grid.get_message(), tiles(&ui));
    let file = format!("30-params-{}", name.to_lowercase().replace(' ', "-"));
    r.png(&file);
    r.sent(&format!("Parameters of {name}"));
    // The engine's names against the tiles': every parameter shown somewhere across the pages.
    let list = r.engine.call("params.list", json!({"node": node}));
    let names: Vec<String> = list["parameters"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap_or("").to_string()).collect();
    let mut seen: Vec<String> = tiles(&ui).into_iter().map(|t| t.0).collect();
    for k in 1..pages.len() {
        grid.invoke_show_page(k as i32);
        r.settle();
        seen.extend(tiles(&ui).into_iter().map(|t| t.0));
        r.png(&format!("{file}-page{k}"));
        let t: Vec<String> = grid.get_tabs().iter().map(|p| p.to_string()).collect();
        for j in 1..t.len() {
            grid.invoke_show_tab(j as i32);
            r.settle();
            seen.extend(tiles(&ui).into_iter().map(|t| t.0));
        }
    }
    if pages.len() > 1 {
        grid.invoke_show_page(0);
        r.settle();
    }
    let missing: Vec<&String> = names.iter().filter(|n| !seen.iter().any(|s| s == *n)).collect();
    eprintln!("   parameters not on a tile by their name: {missing:?}");
    // One edit per kind of tile on the first page.
    let mut done = vec![];
    for (i, t) in tiles(&ui).into_iter().enumerate() {
        if done.contains(&t.2) || t.1.is_empty() && t.2 != 3 {
            continue;
        }
        done.push(t.2);
        let label = format!("Parameters of {name}: {} (kind {})", t.0, t.2);
        let i = i as i32;
        match t.2 {
            0 | 1 | 7 | 8 => param_edit(r, node, &label, |r| {
                let g = r.screen.ui.global::<crate::GridState>();
                g.invoke_grab(i, 0);
                g.invoke_drag(i, 0, if t.3 > 0.5 { 0.2 } else { 0.8 });
                g.invoke_release(i);
            }),
            3 => param_edit(r, node, &label, |r| r.screen.ui.global::<crate::GridState>().invoke_tap(i, -1)),
            2 => param_edit(r, node, &label, |r| r.screen.ui.global::<crate::GridState>().invoke_tap(i, 1)),
            4 => param_edit(r, node, &label, |r| {
                r.screen.ui.global::<crate::GridState>().invoke_tap(i, -1);
                r.settle();
                let list: Vec<String> = r.screen.ui.global::<crate::GridState>().get_list().iter().map(|s| s.to_string()).collect();
                eprintln!("   list {list:?}");
                r.png(&format!("{file}-list"));
                r.screen.ui.global::<crate::GridState>().invoke_choose(if list.len() > 1 { 1 } else { 0 });
            }),
            5 => param_edit(r, node, &label, |r| {
                let g = r.screen.ui.global::<crate::GridState>();
                g.invoke_grab(i, 1);
                g.invoke_drag(i, 1, 0.5);
                g.invoke_release(i);
            }),
            _ => {}
        }
    }
    // The dial: onto the first tile, a press, a step, a press.
    param_edit(r, node, &format!("Parameters of {name}: the dial on a tile"), |r| {
        for _ in 0..4 {
            if r.screen.ui.global::<crate::GridState>().get_tiles().iter().any(|t| t.cursor) {
                break;
            }
            r.act(Action::Move { pane: None, by: 1 });
        }
        r.act(Action::Confirm(None));
        r.act(Action::Move { pane: None, by: 3 });
        r.act(Action::Confirm(None));
    });
    // Out of the value taken for the dial, if it still is one (a Cancel past it would leave the page, and the view's drag
    // would be on a screen that is not shown: the top bar's Undo would be another screen's).
    if r.screen.ui.global::<crate::GridState>().get_tiles().iter().any(|t| t.editing) {
        r.act(Action::Cancel(None));
    }
    r.on(7, &format!("Parameters of {name}: after the dial"));
    // A view (an envelope's, an operator's), on the first page that has one: a handle dragged.
    let _ = panels;
    let with_view = (0..pages.len().max(1)).find(|&k| {
        if pages.len() > 1 {
            grid.invoke_show_page(k as i32);
            r.settle();
        }
        grid.get_panels().iter().count() > 0
    });
    if let Some(k) = with_view {
        let p = grid.get_panels().row_data(0).unwrap();
        eprintln!("   view {:?} on page {k}: {} points", p.title, p.points.iter().count());
        param_edit(r, node, &format!("Parameters of {name}: the first view dragged"), |r| {
            let g = r.screen.ui.global::<crate::GridState>();
            // A handle, in the panel's own pixels (the points it draws); else its middle.
            let (x, y) = p.points.iter().nth(1).or(p.points.iter().next()).map_or((p.w * 0.5, p.h * 0.5), |q| (q.x, q.y));
            if g.invoke_view_down(0, x, y) {
                g.invoke_view_move(0, x + p.w * 0.1, y - p.h * 0.2);
                g.invoke_view_up(0, x + p.w * 0.1, y - p.h * 0.2);
            } else {
                r.note(format!("Parameters of {name}: the view took no touch at ({x}, {y})"));
            }
        });
        r.png(&format!("{file}-view"));
        if pages.len() > 1 {
            grid.invoke_show_page(0);
            r.settle();
        }
    }
    // Search, where the page has one: the keyboard opens, typed letters narrow the tiles.
    if grid.get_search_shown() {
        r.step(&format!("Parameters of {name}: search"));
        ui.global::<crate::Bar>().invoke_action(0); // Search, the top bar's first action
        r.settle();
        let keys: Vec<(String, i32)> = ui.global::<crate::KeyboardState>().get_keys().iter().map(|k| (k.label.to_string(), k.index)).collect();
        let word = names.iter().find(|n| n.len() > 3).map(|n| n[..3].to_lowercase()).unwrap_or("a".into());
        for c in word.chars() {
            if let Some(k) = keys.iter().find(|k| k.0 == c.to_string()) {
                ui.global::<crate::KeyboardState>().invoke_pressed(k.1);
                ui.global::<crate::KeyboardState>().invoke_released();
            }
        }
        if let Some(k) = keys.iter().find(|k| k.0 == "Done") {
            ui.global::<crate::KeyboardState>().invoke_pressed(k.1);
        }
        r.settle();
        eprintln!("   search {:?} -> tiles {:?}", grid.get_search(), tiles(&ui).iter().map(|t| t.0.clone()).collect::<Vec<_>>());
        r.png(&format!("{file}-search"));
        r.sent("search");
    }
}

/// The differences between two documents, each "path: was -> now".
fn diff(a: &Value, b: &Value, path: &str, out: &mut impl FnMut(String)) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                diff(x.get(k).unwrap_or(&Value::Null), y.get(k).unwrap_or(&Value::Null), &format!("{path}.{k}"), out);
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, &format!("{path}[{i}]"), out);
            }
        }
        _ if a != b => out(format!("{path}: {} -> {}", short(a), short(b))),
        _ => {}
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 { format!("{}...", &s[..160]) } else { s }
}

mod engine;
mod guard;
mod library;
mod seed;

// The Library app against the same engine, with a small library of its own (ui/LIBRARY-APP.md): the shell reaches
// it with the switch the tests use. The engine the harness starts has the run's OSCAIRN_DATA_HOME (the state file of
// "Add to setlist" is written there).
#[test]
#[ignore = "needs an engine binary: OSCAIRN_REAL_ENGINE"]
fn real_library() {
    let Some(mut r) = Real::start(seed::Seed::Library) else {
        eprintln!("real_library: OSCAIRN_REAL_ENGINE is not set");
        return;
    };
    shell::library_in_rust(true);
    library::library(&mut r);
    library::library_second(&mut r);
    eprintln!("\nFINDINGS ({}):\n{}", r.findings.len(), r.findings.join("\n"));
}

mod settings;

// The Settings app against the same engine (ui/SETTINGS-APP.md): `OSCAIRN_REAL_PROFILE` is the shell's profile (a built-in's id,
// or `desktop` for a computer's, the default), run once for each kind against an engine of its own with `--audio null`.
#[test]
#[ignore = "needs an engine binary: OSCAIRN_REAL_ENGINE"]
fn real_settings() {
    let profile = std::env::var("OSCAIRN_REAL_PROFILE").ok().filter(|p| p != "desktop");
    let Some((mut r, config)) = settings::start_on(profile.as_deref()) else {
        eprintln!("real_settings: OSCAIRN_REAL_ENGINE is not set");
        return;
    };
    settings::settings(&mut r, &config, profile.as_deref());
    eprintln!("\nFINDINGS ({}):\n{}", r.findings.len(), r.findings.join("\n"));
}

#[test]
#[ignore = "needs an engine binary: OSCAIRN_REAL_ENGINE"]
fn real_engine() {
    let Some(mut r) = Real::start(seed::Seed::Check) else {
        eprintln!("real_engine: OSCAIRN_REAL_ENGINE is not set");
        return;
    };
    r.step("connect");
    let connected = r.until(|ui| !ui.global::<Bar>().get_performance().is_empty() && ui.global::<Bar>().get_cpu_known());
    r.check(connected, || "connect: the top bar never filled in".into());
    r.settle();
    r.png("01-home");
    r.sent("connect");
    idle(&mut r, "Home", 20);
    let home: Vec<String> = r.screen.ui.global::<HomeState>().get_tiles().iter().map(|t| format!("{}: {}", t.name, t.line)).collect();
    eprintln!("   home tiles {home:?}");
    // OSCAIRN_REAL_ONLY=part,chain runs only those sections (Play's opening always runs).
    let only = std::env::var("OSCAIRN_REAL_ONLY").unwrap_or_default();
    let want = |name: &str| only.is_empty() || only.split(',').any(|o| o == name);
    type Section = (&'static str, fn(&mut Real));
    let sections: [Section; 12] = [
        ("play", play),
        ("part", part),
        ("chain", chain),
        ("splits", splits),
        ("plugins", plugin_pages),
        ("sounds", sounds),
        ("scene", scene),
        ("parts", parts_list),
        ("mixer", mixer),
        ("controls", controls),
        ("routing", routing),
        ("bar", top_bar),
    ];
    for (name, section) in sections {
        if name == "play" || want(name) {
            if name != "play" {
                to_play(&mut r);
            }
            section(&mut r);
        }
    }
    eprintln!("\nFINDINGS ({}):\n{}", r.findings.len(), r.findings.join("\n"));
}

/// The shell at rest on the screen shown: for `seconds`, nothing touched, how many requests it sent and frames it drew
/// (every notification the engine sent counted too). At rest the answer must be none of either: the engine's status
/// notifications that say what the last said, and the meters' figures that did not change, cost nothing.
fn idle(r: &mut Real, what: &str, seconds: u64) {
    r.step(&format!("Idle on {what} for {seconds} s"));
    r.settle();
    r.run(4000); // what a load or a connect leaves to ask, asked
    let (mark, frames, figures) = (r.lines().len(), r.frames, r.figures);
    r.run(seconds * 1000);
    let lines = r.lines();
    let mut asked: Vec<String> = vec![];
    let (mut notes, mut statuses) = (0, 0);
    for l in lines.iter().skip(mark) {
        match l {
            Line::Request { method, .. } => asked.push(method.clone()),
            Line::Note { method, .. } if method == "status" => statuses += 1,
            Line::Note { .. } => notes += 1,
            _ => {}
        }
    }
    let (drawn, changed) = (r.frames - frames, r.figures - figures);
    let mut by_method: Vec<(String, usize)> = vec![];
    for m in &asked {
        match by_method.iter_mut().find(|x| &x.0 == m) {
            Some(x) => x.1 += 1,
            None => by_method.push((m.clone(), 1)),
        }
    }
    eprintln!("   IDLE {what}: {} requests in {seconds} s ({:.2}/s) {by_method:?}; {drawn} frames ({:.2}/s), {changed} changes of the CPU or RAM figure; {statuses} status and {notes} other notifications", asked.len(), asked.len() as f64 / seconds as f64, drawn as f64 / seconds as f64);
    r.check(asked.is_empty(), || format!("idle on {what}: {} requests in {seconds} s at rest: {by_method:?}", asked.len()));
    r.check(drawn <= changed, || format!("idle on {what}: {drawn} frames drawn in {seconds} s at rest, {changed} figures changed"));
}

fn part_of<'a>(g: &'a Value, name: &str) -> &'a Value {
    g["parts"].as_array().unwrap().iter().find(|p| p["name"] == name).unwrap_or(&Value::Null)
}


fn strips(ui: &Shell) -> Vec<(String, String, bool, bool, f32)> {
    ui.global::<crate::PlayState>().get_strips().iter().map(|s| (s.name.to_string(), s.detail.to_string(), s.mute, s.solo, s.fraction)).collect()
}

fn knobs(ui: &Shell) -> Vec<(String, String)> {
    ui.global::<crate::PlayState>().get_knobs().iter().map(|t| (t.name.to_string(), t.value.to_string())).collect()
}

fn play(r: &mut Real) {
    r.step("Play: open from Home's Performance tile");
    r.screen.ui.global::<HomeState>().invoke_tapped(0);
    r.settle();
    r.run(600);
    let ui = r.screen.ui.clone_strong();
    r.check(ui.get_screen() == 3, || format!("Play: the screen is {}", ui.get_screen()));
    r.png("02-play");
    r.sent("Play open");
    idle(r, "Play", 20);
    let scenes: Vec<String> = ui.global::<crate::PlayState>().get_scenes().iter().map(|s| format!("{}{}", s.name, if s.on { "*" } else { "" })).collect();
    eprintln!("   scenes {scenes:?}\n   strips {:?}\n   knobs {:?}\n   buttons {:?} bank {:?}", strips(&ui), knobs(&ui), ui.global::<crate::PlayState>().get_knob_buttons().iter().map(|b| b.text.to_string()).collect::<Vec<_>>(), ui.global::<crate::PlayState>().get_knob_bank());
    eprintln!("   bar: {:?} {:?} actions {:?}", ui.global::<Bar>().get_performance(), ui.global::<Bar>().get_scene(), ui.global::<Bar>().get_actions().iter().map(|a| (a.label.to_string(), a.enabled)).collect::<Vec<_>>());
    let g = r.engine.graph();
    let names: Vec<String> = g["parts"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_string()).collect();
    let shown: Vec<String> = strips(&ui).into_iter().map(|s| s.0).collect();
    r.check(names == shown, || format!("Play: strips {shown:?}, the engine's parts {names:?}"));

    r.step("Play: tap scene 2 (Chorus)");
    ui.global::<crate::PlayState>().invoke_scene_tapped(1);
    r.settle();
    r.sent("Play scene tap");
    let g = r.engine.graph();
    r.check(g["scene"] == 2, || format!("Play scene tap: the engine's scene is {}", g["scene"]));
    r.check(ui.global::<Bar>().get_scene() == "Chorus", || format!("Play scene tap: the bar says {:?}", ui.global::<Bar>().get_scene()));
    r.check(part_of(&g, "Split")["level"].as_f64().is_some_and(|l| (l - 0.5).abs() < 0.01), || format!("Play scene tap: Chorus's Split level not applied: {}", part_of(&g, "Split")["level"]));

    r.step("Play: the left knob to scene 1, a press");
    r.act(Action::Move { pane: Some(0), by: 1 }); // shows the cursor
    let at = ui.global::<crate::PlayState>().get_scenes().iter().position(|s| s.cursor).unwrap_or(0);
    if at > 0 {
        r.act(Action::Move { pane: Some(0), by: -(at as i32) });
    }
    r.act(Action::Confirm(Some(0)));
    r.sent("Play scene by knob");
    let g = r.engine.graph();
    r.check(g["scene"] == 1, || format!("Play scene by knob: the engine's scene is {}", g["scene"]));
    r.act(Action::Cancel(None));

    r.step("Play: M on the first strip");
    ui.global::<crate::PlayState>().invoke_mute_tapped(0);
    r.settle();
    r.sent("Play mute");
    let g = r.engine.graph();
    r.check(part_of(&g, "Lead")["mute"] == true, || "Play mute: the engine's Lead is not muted".into());
    r.check(strips(&ui)[0].2, || "Play mute: the strip does not show it".into());
    r.undo("Play mute");
    let g = r.engine.graph();
    r.check(part_of(&g, "Lead")["mute"] == false, || "Play mute: undo left Lead muted".into());
    r.check(!strips(&ui)[0].2, || "Play mute: after undo the strip still shows M".into());

    r.step("Play: S on the third strip");
    ui.global::<crate::PlayState>().invoke_solo_tapped(2);
    r.settle();
    r.sent("Play solo");
    let g = r.engine.graph();
    r.check(part_of(&g, "Split")["solo"] == true, || "Play solo: the engine's Split is not soloed".into());
    r.check(strips(&ui)[2].3, || "Play solo: the strip does not show it".into());
    r.png("03-play-solo");
    r.undo("Play solo");
    let g = r.engine.graph();
    r.check(part_of(&g, "Split")["solo"] == false, || "Play solo: undo left Split soloed".into());
    r.check(!strips(&ui)[2].3, || "Play solo: after undo the strip still shows S".into());

    r.step("Play: the first fader to half its travel");
    ui.global::<crate::PlayState>().invoke_held(0, true);
    ui.global::<crate::PlayState>().invoke_level(0, 0.5);
    ui.global::<crate::PlayState>().invoke_held(0, false);
    r.settle();
    r.sent("Play fader");
    let g = r.engine.graph();
    let level = part_of(&g, "Lead")["level"].as_f64().unwrap_or(-1.);
    r.check(level < 0.99 && level > 0.0, || format!("Play fader: the engine's Lead level is {level}"));
    eprintln!("   Lead level {level}, strip {:?}", strips(&ui)[0]);
    r.undo("Play fader");
    let g = r.engine.graph();
    let level = part_of(&g, "Lead")["level"].as_f64().unwrap_or(-1.);
    r.check((level - 1.).abs() < 0.001, || format!("Play fader: undo left Lead's level at {level}"));
    r.check(strips(&ui)[0].4 > 0.99, || format!("Play fader: after undo the strip shows {:?}", strips(&ui)[0]));

    r.step("Play: the master fader");
    ui.global::<crate::PlayState>().invoke_level(-1, 0.25);
    r.settle();
    r.sent("Play master");
    let g = r.engine.graph();
    eprintln!("   master {}", g["master"]);
    r.check(g["master"]["level"].as_f64().is_some_and(|l| l < 0.99), || format!("Play master: the engine's master is {}", g["master"]));
    r.undo("Play master");
    let g = r.engine.graph();
    r.check(g["master"]["level"].as_f64().is_some_and(|l| (l - 1.).abs() < 0.001), || format!("Play master: undo left {}", g["master"]));

    r.step("Play: the first knob of the row dragged");
    let before = knobs(&ui);
    ui.global::<crate::PlayState>().invoke_knob_grab(0, 0);
    ui.global::<crate::PlayState>().invoke_knob_drag(0, 0, 0.8);
    ui.global::<crate::PlayState>().invoke_knob_release(0);
    r.settle();
    let sent = r.sent("Play knob");
    r.check(sent.iter().any(|s| s.0 == "params.set" || s.0 == "controls.set" || s.0 == "parts.level"), || format!("Play knob: nothing set; the row was {before:?}"));
    eprintln!("   knobs after {:?}", knobs(&ui));
    for (m, p, _, _) in sent.iter().filter(|s| s.0 == "params.set") {
        let v = &p["values"][0];
        let now = r.engine.call("params.get", json!({"values": [{"node": v["node"], "param": v["param"]}]}));
        r.check((now["values"][0]["value"].as_f64().unwrap_or(-1.) - v["value"].as_f64().unwrap_or(-2.)).abs() < 1e-6, || format!("Play knob: {m} {p}, the engine reads {now}"));
    }
    r.undo("Play knob");

    r.step("Play: the bank button");
    let buttons: Vec<(String, bool)> = ui.global::<crate::PlayState>().get_knob_buttons().iter().map(|b| (b.text.to_string(), b.bank)).collect();
    if let Some(i) = buttons.iter().position(|b| b.1) {
        let was = ui.global::<crate::PlayState>().get_knob_bank();
        ui.global::<crate::PlayState>().invoke_knob_button(i as i32);
        r.settle();
        r.sent("Play bank");
        eprintln!("   bank {was:?} -> {:?}; knobs {:?}", ui.global::<crate::PlayState>().get_knob_bank(), knobs(&ui));
        r.png("04-play-bank");
    } else {
        // One bank (one control on a six-knob group): the row shows no bank button.
        eprintln!("   no bank button among {buttons:?} (one bank)");
    }

    r.step("Play: + Scene");
    let add = ui.global::<crate::PlayState>().get_scenes().iter().position(|s| s.add);
    if let Some(i) = add {
        ui.global::<crate::PlayState>().invoke_scene_tapped(i as i32);
        r.settle();
        r.sent("Play + Scene");
        let g = r.engine.graph();
        r.check(g["scenes"].as_array().unwrap().len() == 3, || format!("Play + Scene: the engine has {} scenes", g["scenes"].as_array().unwrap().len()));
        r.check(ui.global::<crate::PlayState>().get_scenes().iter().filter(|s| s.filled).count() == 3, || "Play + Scene: Play does not show the third".into());
        r.undo("Play + Scene");
        let g = r.engine.graph();
        r.check(g["scenes"].as_array().unwrap().len() == 2, || format!("Play + Scene: undo left {} scenes", g["scenes"].as_array().unwrap().len()));
        r.check(ui.global::<crate::PlayState>().get_scenes().iter().filter(|s| s.filled).count() == 2, || "Play + Scene: after undo Play still shows three".into());
    } else {
        r.note("Play + Scene: no + Scene tile".into());
    }
    r.png("05-play-end");
}
