// SPDX-License-Identifier: AGPL-3.0-or-later
//! The engine's control socket (docs/control-protocol.md): one compact JSON-RPC document per
//! line over a Unix stream, as a trusted client of its path. Unlike the C++ LocalClient, a request
//! that takes long never closes the connection: it stays out, the UI shows the engine busy, and its
//! answer is taken whenever it comes. Only the socket closing ends a connection; it is then made
//! again each second.
//!
//! A reader thread of its own does all the waiting; the UI thread only writes (small requests,
//! with a write timeout) and is never blocked on the engine. `R` is what the caller wants to know
//! about each request when its answer comes.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const MAX_LINE: u64 = 2 << 20; // a setlist is at most 1 MiB; its reply, escaped, a little more

pub enum Event<R> {
    /// Connected and answered `session.hello`: ask for what to show, subscribe.
    Connected,
    Lost(String),
    Reply { request: R, result: Result<Value, Error> },
    /// A subscription's notification (`status`, `graph.changed`, ...).
    Notification { method: String, params: Value },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub code: i64,
    pub message: String,
}

/// The engine is busy with a long request and refused this one (docs/control-protocol.md, "Long requests").
pub const BUSY: i64 = -32009;

struct Pending<R> {
    request: R,
    sent: Instant,
}

pub struct Client<R> {
    writer: Mutex<Option<UnixStream>>,
    pending: Mutex<HashMap<u64, Pending<R>>>,
    next: AtomicU64,
}

// A poisoned lock only means another thread panicked holding it; the map and stream are still whole.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl<R: Send + 'static> Client<R> {
    /// Starts the reader thread; `on_event` is called on it.
    pub fn start(path: PathBuf, on_event: impl Fn(Event<R>) + Send + 'static) -> std::io::Result<Arc<Client<R>>> {
        let client = Arc::new(Client { writer: Mutex::new(None), pending: Mutex::new(HashMap::new()), next: AtomicU64::new(1) });
        let me = client.clone();
        std::thread::Builder::new().name("engine-reader".into()).spawn(move || me.run(path, on_event))?;
        Ok(client)
    }

    fn run(&self, path: PathBuf, on_event: impl Fn(Event<R>)) {
        loop {
            match UnixStream::connect(&path).and_then(|s| s.try_clone().map(|w| (s, w))) {
                Ok((read, write)) => {
                    let _ = write.set_write_timeout(Some(Duration::from_millis(200)));
                    *lock(&self.writer) = Some(write);
                    let hello = self.write(0, "session.hello", json!({}));
                    let why = if hello { self.read(read, &on_event) } else { "session.hello not sent".into() };
                    *lock(&self.writer) = None;
                    // What was out gets no answer now: each is heard as failed.
                    let lost: Vec<Pending<R>> = lock(&self.pending).drain().map(|(_, p)| p).collect();
                    for p in lost {
                        on_event(Event::Reply { request: p.request, result: Err(Error { code: 0, message: "the engine's connection closed".into() }) });
                    }
                    on_event(Event::Lost(why));
                }
                Err(e) => on_event(Event::Lost(format!("cannot connect to {}: {e}", path.display()))),
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    fn read(&self, stream: UnixStream, on_event: &impl Fn(Event<R>)) -> String {
        let mut lines = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            // Bounded, so a runaway peer cannot grow it without end: a longer line is not the engine's.
            match (&mut lines).take(MAX_LINE + 1).read_until(b'\n', &mut line) {
                Ok(0) => return "the engine closed the connection".into(),
                Err(e) => return format!("read: {e}"),
                Ok(n) if n as u64 > MAX_LINE => return "a reply longer than 2 MiB".into(),
                Ok(_) => {}
            }
            // Not JSON (garbage, or a line cut off by a close): passed over.
            let Ok(mut doc) = serde_json::from_slice::<Value>(&line) else { continue };
            if let Some(method) = doc.get("method").and_then(Value::as_str) {
                let method = method.to_string();
                on_event(Event::Notification { method, params: doc["params"].take() });
                continue;
            }
            let Some(id) = doc.get("id").and_then(Value::as_u64) else { continue };
            if id == 0 {
                on_event(Event::Connected); // session.hello's answer
                continue;
            }
            let Some(p) = lock(&self.pending).remove(&id) else { continue };
            let result = match doc.get("error") {
                Some(e) => Err(Error { code: e["code"].as_i64().unwrap_or(0), message: e["message"].as_str().unwrap_or("error").into() }),
                None => Ok(doc["result"].take()),
            };
            on_event(Event::Reply { request: p.request, result });
        }
    }

    /// Sends a request without waiting; false when there is no connection or the engine has
    /// stopped reading (its socket full for 200 ms), and then `request` is dropped.
    pub fn send(&self, request: R, method: &str, params: Value) -> bool {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        lock(&self.pending).insert(id, Pending { request, sent: Instant::now() });
        let sent = self.write(id, method, params);
        if !sent {
            lock(&self.pending).remove(&id);
        }
        sent
    }

    fn write(&self, id: u64, method: &str, params: Value) -> bool {
        let doc = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string() + "\n";
        let mut writer = lock(&self.writer);
        let Some(w) = writer.as_mut() else { return false };
        if w.write_all(doc.as_bytes()).is_err() {
            // shortcut: a write timed out part way leaves a broken frame, so the connection is
            // dropped and made again; fine for small requests, revisit for large saves.
            let _ = w.shutdown(std::net::Shutdown::Both);
            *writer = None;
            return false;
        }
        true
    }

    /// How long the oldest request still out for which `counts` holds has been out.
    pub fn waiting(&self, counts: impl Fn(&R) -> bool) -> Option<Duration> {
        lock(&self.pending).values().filter(|p| counts(&p.request)).map(|p| p.sent.elapsed()).max()
    }

    pub fn connected(&self) -> bool {
        lock(&self.writer).is_some()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc::{self, Receiver};

    pub const WAIT: Duration = Duration::from_secs(3);

    /// The client's events as text, in order.
    fn events() -> (impl Fn(Event<&'static str>) + Send + 'static, Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        let on = move |e| {
            let _ = tx.send(match e {
                Event::Connected => "connected".to_string(),
                Event::Lost(_) => "lost".into(),
                Event::Reply { request, result } => format!("{request}:{}", result.map(|_| "ok".to_string()).unwrap_or_else(|e| e.code.to_string())),
                Event::Notification { method, .. } => method,
            });
        };
        (on, rx)
    }

    /// The engine's end of a connection: reads requests, writes whatever a test wants.
    pub struct FakeEngine {
        reader: BufReader<UnixStream>,
        pub writer: UnixStream,
    }

    impl FakeEngine {
        pub fn new(stream: UnixStream) -> FakeEngine {
            FakeEngine { reader: BufReader::new(stream.try_clone().unwrap()), writer: stream }
        }
        pub fn request(&mut self) -> Value {
            let mut l = String::new();
            self.reader.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        }
        pub fn answer(&mut self) {
            let id = self.request()["id"].clone();
            self.send(&format!("{}\n", json!({"jsonrpc":"2.0","id":id,"result":{}})));
        }
        pub fn send(&mut self, text: &str) {
            self.writer.write_all(text.as_bytes()).unwrap();
        }
    }

    /// A socket path of the test's own, and a client of it with the engine answering hello.
    fn connected(name: &str) -> (Arc<Client<&'static str>>, FakeEngine, Receiver<String>, UnixListener) {
        let path = std::env::temp_dir().join(format!("oscairn-ui-{name}-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        let (on, rx) = events();
        let client = Client::start(path, on).unwrap();
        let mut engine = FakeEngine::new(listener.accept().unwrap().0);
        engine.answer(); // session.hello
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "connected");
        (client, engine, rx, listener)
    }

    // A request the engine is slow to answer keeps the connection: the late answer arrives and
    // later requests still work. Then the engine closing it is reported as Lost.
    #[test]
    fn slow_answer_keeps_the_connection() {
        let (client, mut engine, rx, _l) = connected("slow");
        assert!(client.send("slow", "library.load", json!({"id":"x"})));
        std::thread::sleep(Duration::from_millis(1500)); // past LocalClient's 1 s deadline
        assert!(client.waiting(|_| true).unwrap() >= Duration::from_millis(1400) && client.connected());
        engine.answer();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "slow:ok");
        assert!(client.send("next", "library.query", json!({})));
        engine.answer();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "next:ok");
        assert!(client.waiting(|_| true).is_none());
        drop(engine);
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "lost");
    }

    // Garbage, half-formed and stray documents are passed over; errors keep their code;
    // notifications come through.
    #[test]
    fn garbage_is_passed_over() {
        let (client, mut engine, rx, _l) = connected("garbage");
        assert!(client.send("real", "setlists.get", json!({"id":"s"})));
        let id = engine.request()["id"].clone();
        engine.writer.write_all(b"\xff\xfe not utf-8\n").unwrap();
        engine.send("{not json\n");
        engine.send("[1,2,3]\n");
        engine.send("{\"jsonrpc\":\"2.0\",\"result\":{}}\n"); // no id
        engine.send("{\"jsonrpc\":\"2.0\",\"id\":999999,\"result\":{}}\n"); // nobody asked
        engine.send("{\"jsonrpc\":\"2.0\",\"method\":\"status\",\"params\":7}\n");
        engine.send(&format!("{}\n", json!({"jsonrpc":"2.0","id":id,"error":{"code":-32009}}))); // busy, no message
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "status");
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "real:-32009");
        assert!(client.send("after", "library.query", json!({})));
        engine.answer();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "after:ok");
    }

    // The engine gone in the middle of a reply: the request out fails, then the connection is
    // lost, and it is made again once the engine is back.
    #[test]
    fn dropped_mid_reply_then_reconnected() {
        let (client, mut engine, rx, listener) = connected("cut");
        assert!(client.send("cut", "library.load", json!({"id":"x"})));
        let id = engine.request()["id"].clone();
        engine.send(&format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"resu"));
        drop(engine);
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "cut:0");
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "lost");
        assert!(!client.send("none", "library.load", json!({})));
        let mut engine = FakeEngine::new(listener.accept().unwrap().0);
        engine.answer();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "connected");
    }

    // A line past the bound drops the connection rather than growing without end.
    #[test]
    fn endless_line_is_cut() {
        let (_client, mut engine, rx, _l) = connected("endless");
        let chunk = "x".repeat(64 << 10);
        std::thread::spawn(move || while engine.writer.write_all(chunk.as_bytes()).is_ok() {});
        assert_eq!(rx.recv_timeout(Duration::from_secs(10)).unwrap(), "lost");
    }

    // No engine yet: said, and tried again each second until it is there.
    #[test]
    fn waits_for_the_engine() {
        let path = std::env::temp_dir().join(format!("oscairn-ui-absent-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (on, rx) = events();
        let client = Client::start(path.clone(), on).unwrap();
        assert_eq!(rx.recv_timeout(WAIT).unwrap(), "lost");
        assert!(!client.send("early", "library.query", json!({})));
        let listener = UnixListener::bind(&path).unwrap();
        let mut engine = FakeEngine::new(listener.accept().unwrap().0);
        engine.answer();
        while rx.recv_timeout(WAIT).unwrap() != "connected" {}
        let _ = std::fs::remove_file(&path);
    }
}
