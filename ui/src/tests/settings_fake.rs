// SPDX-License-Identifier: AGPL-3.0-or-later
//! A fake engine for the Settings app that answers as the real handlers of core/control/src/json_rpc.cpp do (the
//! request shapes and what they answer are in ui/SETTINGS-APP.md "Verified shapes"): the parameters a handler takes and
//! no others (`fields` refuses an unknown one with -32602), the answers' field names and types, the errors worded as the
//! real engine words them where a real engine was asked (the null audio device, a key's name, a repository), and what a
//! request leaves behind. Every request it was asked is kept, in order, so a test can say what the screen sent.

use crate::client::Error;
use crate::settings_repos::key_id;
use serde_json::{json, Value};

const BUSY: i64 = -32009;

fn refused(code: i64, message: &str) -> Error {
    Error { code, message: message.into() }
}

/// An audio output or input device as the engine lists it.
#[derive(Clone, Debug)]
pub struct Dev {
    pub name: String,
    pub rates: Vec<u32>,
    pub sizes: Vec<u32>,
}

pub fn dev(name: &str, rates: &[u32], sizes: &[u32]) -> Dev {
    Dev { name: name.into(), rates: rates.to_vec(), sizes: sizes.to_vec() }
}

pub struct Audio {
    pub outputs: Vec<Dev>,
    pub inputs: Vec<Dev>,
    pub output: String,
    pub input: String,
    pub rate: u32,
    pub size: u32,
    pub source: String,
    pub notice: String,
    pub fixed: bool,
    /// What the engine says refuses every choice of a device (the null device's own words).
    pub fixes: String,
    /// A device that cannot be opened: why. The last one plays again.
    pub cannot_open: Vec<(String, String)>,
    /// A new rate the device has not restarted at yet: the answers still have the old one, as the real engine's do (it applies the
    /// rate some time after answering), and the status after it has the new one.
    pub pending_rate: Option<u32>,
}

/// A MIDI input as the engine keeps it.
#[derive(Clone, Debug)]
pub struct Port {
    pub name: String,
    pub profiles: Vec<(String, String)>,
    pub automatic: bool,
    pub chosen: Vec<String>,
    pub ignored: bool,
    /// "mackie" or "duplicate" for the profile that plays it, else "controls".
    pub kind: String,
}

pub fn port(name: &str, profiles: &[(&str, &str)]) -> Port {
    Port { name: name.into(), profiles: profiles.iter().map(|p| (p.0.to_string(), p.1.to_string())).collect(), automatic: true, chosen: vec![], ignored: false, kind: "controls".into() }
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub address: String,
    pub name: String,
    pub key: String,
    pub origin: String,
    pub enabled: bool,
}

/// What a repository's listing comes to.
#[derive(Clone, Debug)]
pub enum Reads {
    /// It reads: the name its root claims and its root key.
    Done(String, String),
    /// It cannot be read: why.
    Fails(String),
    /// Its root key is not trusted: the name it claims and the key.
    Untrusted(String, String),
}

/// A package job in the engine: a listing, or an install, and how far `packages.job` has polled it (`u32::MAX`: ended;
/// one job runs at a time, another is refused with reason `busy`).
#[derive(Clone)]
enum JobKind {
    /// A listing of `source`.
    List { source: String, polls: u32 },
    /// An install of `id@version`: what its done job answers, and the `packages.installed` record added on success.
    Install { id: String, version: String, polls: u32, installed: Value, record: Value },
}

pub struct Settings {
    pub audio: Audio,
    pub ports: Vec<Port>,
    /// Keys: name, public key.
    pub keys: Vec<(String, String)>,
    pub repos: Vec<Repo>,
    /// The packages installed here, `packages.installed`'s records (each id, version, kind, repository, signers, ...).
    pub installed_packages: Vec<Value>,
    /// The installed libraries, `library.libraries`: id, name, items.
    pub libraries: Vec<(String, String, u64)>,
    /// What `packages.removal` answers for a package, by its id: its `dependents` and its `unused`, as the handler
    /// words them (`{kind,id,name,version,items}` with `examples` for `own`; `{id,version,kind,name,bytes}`).
    pub removal: Vec<(String, Vec<Value>, Vec<Value>)>,
    /// How many reads of `packages.installed` still list what `packages.uninstall` removed.
    pub uninstall_lag: u32,
    removing: Option<(Vec<String>, u32)>,
    /// The trusted root keys.
    pub trusted: Vec<String>,
    /// What each address reads as, until it is added.
    pub sources: Vec<(String, Reads)>,
    /// The packages a repository's listing offers, by source address: what `packages.job` returns as its `packages`
    /// when the listing is done.
    pub offers: Vec<(String, Vec<Value>)>,
    /// How many times a listing job is asked about before it ends.
    pub job_polls: u32,
    jobs: Vec<(u64, JobKind)>,
    /// What `packages.install` brings, by `source|id|version`: the done job's `installed` object and the new
    /// `packages.installed` record. Empty: with nothing to install, the job ends failed asking a dependency.
    pub installs: Vec<(String, String, String, Value, Value)>,
    /// A `packages.install` refused at once because another package job runs (reason `busy`).
    pub install_busy: bool,
    pub release: String,
    /// The next this many requests that are not session ones are refused as busy.
    pub busy: u32,
    pub calls: Vec<(String, Value)>,
    /// What the engine pushes after a request, taken by the caller.
    pub notifications: Vec<(String, Value)>,
    /// A method that fails: its message.
    pub fail: Vec<(String, String)>,
    pub list: u64,
    pub midi_list: u64,
    next_key: u32,
}

impl Settings {
    /// A desktop's engine: a built-in output and a USB one, an input, a few rates and buffer sizes.
    pub fn new() -> Settings {
        Settings {
            audio: Audio {
                outputs: vec![dev("Built-in Output", &[44100, 48000, 96000], &[64, 128, 256, 512]), dev("Studio Interface", &[48000, 96000], &[32, 64, 128])],
                inputs: vec![dev("Built-in Microphone", &[48000], &[128]), dev("Studio Interface", &[48000, 96000], &[64, 128])],
                output: "Built-in Output".into(),
                input: "Built-in Microphone".into(),
                rate: 48000,
                size: 128,
                source: "default".into(),
                notice: String::new(),
                fixed: false,
                fixes: String::new(),
                cannot_open: vec![],
                pending_rate: None,
            },
            ports: vec![],
            keys: vec![],
            repos: vec![Repo { address: "https://packages.oscairn.org".into(), name: "Oscairn".into(), key: "6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into(), origin: "official".into(), enabled: true }],
            installed_packages: vec![],
            removal: vec![],
            libraries: vec![],
            uninstall_lag: 0,
            removing: None,
            trusted: vec!["6a7eceba0a34104fea44e99e8c904fc53f410b4ad7ee615bbb51bf6876630841".into()],
            sources: vec![],
            offers: vec![],
            job_polls: 1,
            jobs: vec![],
            installs: vec![],
            install_busy: false,
            release: "0.1.0-alpha.1".into(),
            busy: 0,
            calls: vec![],
            notifications: vec![],
            fail: vec![],
            list: 1,
            midi_list: 1,
            next_key: 1,
        }
    }

    /// The null device, as `--audio null` makes it: one output that is fixed, and rates and sizes to choose.
    pub fn null() -> Settings {
        let mut s = Settings::new();
        s.audio = Audio {
            outputs: vec![dev("null", &[48000, 44100, 88200, 96000], &[64, 128, 256, 512])],
            inputs: vec![],
            output: "null".into(),
            input: String::new(),
            rate: 48000,
            size: 128,
            source: "null".into(),
            notice: String::new(),
            fixed: true,
            fixes: "--audio null fixes the audio device: null".into(),
            cannot_open: vec![],
            pending_rate: None,
        };
        s
    }

    pub fn methods(&self) -> Vec<String> {
        self.calls.iter().map(|c| c.0.clone()).collect()
    }

    pub fn called(&self, method: &str) -> Vec<Value> {
        self.calls.iter().filter(|c| c.0 == method).map(|c| c.1.clone()).collect()
    }

    /// The handler's own check of a request's fields (json_rpc.cpp `fields`): none outside `allowed`.
    /// What a removal takes away: the packages' records and their libraries.
    fn forget(&mut self, ids: &[String]) {
        self.installed_packages.retain(|x| !ids.iter().any(|i| x["id"] == i.as_str()));
        self.libraries.retain(|l| !ids.contains(&l.0));
    }

    fn fields(&self, p: &Value, allowed: &[&str]) -> Result<(), Error> {
        let Some(o) = p.as_object() else { return Err(refused(-32602, "Invalid method parameters")) };
        match o.keys().any(|k| !allowed.contains(&k.as_str())) {
            true => Err(refused(-32602, "Unknown or duplicate parameter field")),
            false => Ok(()),
        }
    }

    fn audio_json(&self, rate: u32) -> Value {
        let list = |d: &[Dev]| d.iter().map(|d| json!({"name": d.name, "sampleRates": d.rates, "bufferSizes": d.sizes})).collect::<Vec<_>>();
        let a = &self.audio;
        json!({"outputs": list(&a.outputs), "inputs": list(&a.inputs), "output": a.output, "input": a.input, "sampleRate": rate, "bufferSize": a.size, "source": a.source, "fixed": a.fixed, "notice": a.notice})
    }

    /// `audio.set` (json_rpc.cpp:1307-1334; Control::setAudio): a choice checked against what is offered (-32602), then
    /// applied and kept as the user's; a device that fails to open leaves the last one (-32005, the reason).
    fn audio_set(&mut self, p: &Value) -> Result<Value, Error> {
        self.fields(p, &["output", "input", "sampleRate", "bufferSize"])?;
        let a = &mut self.audio;
        if (p.get("output").is_some() || p.get("input").is_some()) && !a.fixes.is_empty() {
            return Err(refused(-32602, &a.fixes));
        }
        if let Some(o) = p.get("output").and_then(Value::as_str) {
            if !a.outputs.iter().any(|d| d.name == o) {
                return Err(refused(-32602, &format!("{o} is not an output device")));
            }
            if let Some((_, why)) = a.cannot_open.iter().find(|c| c.0 == o) {
                return Err(refused(-32005, why));
            }
            a.output = o.into();
            a.source = "user".into();
        }
        if let Some(i) = p.get("input").and_then(Value::as_str) {
            if !a.inputs.iter().any(|d| d.name == i) {
                return Err(refused(-32602, &format!("{i} is not an input device")));
            }
            a.input = i.into();
            a.source = "user".into();
        }
        let open = a.outputs.iter().find(|d| d.name == a.output).cloned().unwrap_or(dev("", &[], &[]));
        if let Some(r) = p.get("sampleRate") {
            // The handler reads an integer (`integer(v)`).
            let Some(r) = r.as_u64().map(|r| r as u32) else { return Err(refused(-32602, "Expected an integer")) };
            if !open.rates.contains(&r) {
                return Err(refused(-32602, &format!("{r} is not a sample rate {} offers ({})", open.name, open.rates.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))));
            }
            a.pending_rate = Some(r).filter(|r| *r != a.rate);
        }
        if let Some(s) = p.get("bufferSize") {
            let Some(s) = s.as_u64().map(|s| s as u32) else { return Err(refused(-32602, "Expected an integer")) };
            if !open.sizes.contains(&s) {
                return Err(refused(-32602, &format!("{s} is not a buffer size {} offers ({})", open.name, open.sizes.iter().map(u32::to_string).collect::<Vec<_>>().join(", "))));
            }
            a.size = s;
        }
        self.list += 1;
        Ok(self.audio_json(self.audio.rate))
    }

    fn port_json(&self, p: &Port) -> Value {
        let playing: Vec<String> = if p.ignored {
            vec![]
        } else if p.automatic {
            p.profiles.first().map(|f| vec![f.0.clone()]).unwrap_or_default()
        } else {
            p.chosen.clone()
        };
        let used = if playing.is_empty() { String::new() } else { p.kind.clone() };
        json!({"name": p.name, "profiles": p.profiles.iter().map(|f| json!({"id": f.0, "name": f.1})).collect::<Vec<_>>(), "automatic": p.automatic, "chosen": p.chosen, "playing": playing, "use": used, "ignored": p.ignored})
    }

    fn ports_json(&self) -> Value {
        json!({"inputs": self.ports.iter().map(|p| self.port_json(p)).collect::<Vec<_>>()})
    }

    /// `controllers.inputs`, `.choose` and `.ignore` (json_rpc.cpp:1910-1945).
    fn controllers(&mut self, method: &str, p: &Value) -> Result<Value, Error> {
        match method {
            "controllers.choose" => {
                self.fields(p, &["input", "profiles", "automatic"])?;
                let input = p.get("input").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?.to_string();
                let (list, automatic) = (p.get("profiles"), p.get("automatic"));
                if list.is_none() != automatic.is_some_and(|a| a == &json!(true)) {
                    return Err(refused(-32602, "Give the profiles, or automatic: true"));
                }
                let ids: Option<Vec<String>> = list.map(|l| l.as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect());
                let known: Vec<String> = self.ports.iter().flat_map(|p| p.profiles.iter().map(|f| f.0.clone())).collect();
                if let Some(bad) = ids.iter().flatten().find(|i| !known.contains(i)) {
                    return Err(refused(-32602, &format!("No controller profile is named {bad}")));
                }
                if let Some(port) = self.ports.iter_mut().find(|p| p.name == input) {
                    port.automatic = ids.is_none();
                    port.chosen = ids.unwrap_or_default();
                }
                self.midi_list += 1;
            }
            "controllers.ignore" => {
                self.fields(p, &["input", "ignored"])?;
                let input = p.get("input").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?;
                let on = p.get("ignored").and_then(Value::as_bool).ok_or_else(|| refused(-32602, "Expected a boolean"))?;
                if let Some(port) = self.ports.iter_mut().find(|p| p.name == input) {
                    port.ignored = on;
                }
                self.midi_list += 1;
            }
            _ => self.fields(p, &[])?,
        }
        Ok(self.ports_json())
    }

    /// `keys.list` and `keys.make` (json_rpc.cpp:1124-1135), the name's rule as the engine words it.
    fn keys(&mut self, method: &str, p: &Value) -> Result<Value, Error> {
        let entry = |k: &(String, String)| json!({"name": k.0, "id": key_id(&k.1), "key": k.1});
        if method == "keys.list" {
            self.fields(p, &[])?;
            return Ok(json!({"keys": self.keys.iter().map(entry).collect::<Vec<_>>()}));
        }
        self.fields(p, &["name"])?;
        let name = p.get("name").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?;
        let ok = !name.is_empty() && name.len() <= 64 && !name.starts_with('.') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
        if !ok {
            return Err(refused(-32602, "a key's name is letters, digits, '.', '-' and '_', at most 64, not starting with '.'"));
        }
        if self.keys.iter().any(|k| k.0 == name) {
            return Err(refused(-32602, &format!("there is a key named {name} already")));
        }
        self.next_key += 1;
        // A public key of 32 bytes, distinct for each (the engine's is random).
        let key: String = (0..32).map(|i| format!("{:02x}", (i as u32 * 7 + self.next_key * 13) % 256)).collect();
        self.keys.push((name.into(), key));
        Ok(entry(self.keys.last().unwrap()))
    }

    /// Whether a package job is running: a listing or an install is still being polled.
    fn job_running(&self) -> bool {
        self.jobs.iter().any(|(_, j)| match j {
            JobKind::List { polls, .. } | JobKind::Install { polls, .. } => *polls != u32::MAX,
        })
    }

    fn job_json(&self, id: u64, state: &str, error: &str, root: &str, claimed: &str) -> Value {
        json!({"job": id, "state": state, "error": error, "repository": "", "name": "", "claimedName": claimed, "rootKey": root, "serial": 0, "packages": [], "package": "", "version": "", "installed": {"added": 0, "skipped": 0, "notes": [], "code": [], "dependencies": []}})
    }

    /// The repositories' requests (json_rpc.cpp:1036-1122), the errors as the real engine words them.
    fn packages(&mut self, method: &str, p: &Value) -> Result<Value, Error> {
        match method {
            "packages.repositories" => {
                self.fields(p, &[])?;
                let list: Vec<Value> = self.repos.iter().map(|r| json!({"address": r.address, "name": r.name, "key": r.key, "id": key_id(&r.key), "origin": r.origin, "enabled": r.enabled})).collect();
                Ok(json!({"repositories": list}))
            }
            "packages.enableRepository" => {
                self.fields(p, &["address", "enabled"])?;
                let address = p.get("address").and_then(Value::as_str).unwrap_or_default();
                let on = p.get("enabled").and_then(Value::as_bool).ok_or_else(|| refused(-32602, "Expected a boolean"))?;
                match self.repos.iter_mut().find(|r| r.address == address) {
                    Some(r) => r.enabled = on,
                    None => return Err(refused(-32602, &format!("no repository {address} on this device"))),
                }
                Ok(json!({"accepted": true}))
            }
            "packages.removeRepository" => {
                self.fields(p, &["address"])?;
                let address = p.get("address").and_then(Value::as_str).unwrap_or_default();
                let Some(at) = self.repos.iter().position(|r| r.address == address) else { return Err(refused(-32602, &format!("no repository {address} on this device"))) };
                if self.repos[at].origin == "official" {
                    return Err(refused(-32602, "the official repository can be turned off but not removed"));
                }
                self.repos.remove(at);
                Ok(json!({"accepted": true}))
            }
            "packages.addRepository" => {
                self.fields(p, &["address", "key", "name"])?;
                let text = |k: &str| p.get(k).and_then(Value::as_str).map(String::from);
                let (address, key) = (text("address").ok_or_else(|| refused(-32602, "Expected a string"))?, text("key").ok_or_else(|| refused(-32602, "Expected a string"))?);
                if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err(refused(-32602, "a repository's root key is 64 lowercase hex digits"));
                }
                if !self.trusted.contains(&key) {
                    self.trusted.push(key.clone());
                }
                let name = text("name").unwrap_or_else(|| address.clone());
                match self.repos.iter_mut().find(|r| r.address == address) {
                    Some(r) => r.name = name,
                    None => self.repos.push(Repo { address, name, key, origin: "user".into(), enabled: true }),
                }
                Ok(json!({"accepted": true}))
            }
            "packages.list" => {
                self.fields(p, &["source"])?;
                let source = p.get("source").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?.to_string();
                if self.job_running() {
                    return Err(Error { code: -32007, message: "a package job is running; wait for it to finish".into() });
                }
                let id = self.jobs.len() as u64 + 1;
                self.jobs.push((id, JobKind::List { source, polls: 0 }));
                Ok(json!({"job": id}))
            }
            "packages.install" => {
                self.fields(p, &["source", "id", "version", "acceptUnsignedCode", "replace"])?;
                let source = p.get("source").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?.to_string();
                let id = p.get("id").and_then(Value::as_str).ok_or_else(|| refused(-32602, "Expected a string"))?.to_string();
                let version = p.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
                // One job runs at a time; another is refused with reason `busy`.
                if self.install_busy || self.job_running() {
                    return Err(refused(-32007, "a package job is running; wait for it to finish"));
                }
                let (installed, record) = self.installs.iter().find(|(a, i, v, _, _)| a == &source && i == &id && (version.is_empty() || v == &version)).map(|x| (x.3.clone(), x.4.clone())).unwrap_or_default();
                let job = self.jobs.len() as u64 + 1;
                self.jobs.push((job, JobKind::Install { id, version, polls: 0, installed, record }));
                Ok(json!({"job": job}))
            }
            "packages.job" => {
                self.fields(p, &["job"])?;
                let id = p.get("job").and_then(Value::as_u64).unwrap_or(0);
                let limit = self.job_polls;
                let Some(at) = self.jobs.iter().position(|j| j.0 == id) else { return Err(refused(-32602, &format!("no package job {id}"))) };
                match &mut self.jobs[at].1 {
                    JobKind::List { source, polls } => {
                        if *polls != u32::MAX && *polls < limit {
                            *polls += 1;
                            return Ok(self.job_json(id, "running", "", "", ""));
                        }
                        *polls = u32::MAX;
                        let source = source.clone();
                        let trusted = self.trusted.clone();
                        let mut answer = match self.sources.iter().find(|s| s.0 == source).map(|s| s.1.clone()) {
                            // A root signed whole is read when its key is trusted, else offered by its key (repository.cpp:365).
                            Some(Reads::Done(name, key)) | Some(Reads::Untrusted(name, key)) => {
                                if trusted.contains(&key) {
                                    self.job_json(id, "done", "", &key, &name)
                                } else {
                                    let why = format!("not a repository this device trusts: its root key is {}; trust that key for repositories to use it", key_id(&key));
                                    self.job_json(id, "failed", &why, &key, &name)
                                }
                            }
                            Some(Reads::Fails(why)) => self.job_json(id, "failed", &why, "", ""),
                            None => self.job_json(id, "failed", &format!("not a usable repository: cannot fetch repository.json: cannot reach {source}/repository.json: Could not connect to the server."), "", ""),
                        };
                        // This device's name for the repository at `source`, empty when it knows none.
                        if let Some(r) = self.repos.iter().find(|r| r.address == source) {
                            answer["name"] = json!(r.name);
                        }
                        if answer["state"] == "done" {
                            if let Some((_, pkgs)) = self.offers.iter().find(|s| s.0 == source) {
                                answer["packages"] = json!(pkgs);
                            }
                        }
                        Ok(answer)
                    }
                    JobKind::Install { id: pid, version, polls, installed, record, .. } => {
                        if *polls != u32::MAX && *polls < limit {
                            *polls += 1;
                            return Ok(self.job_json(id, "running", "", "", ""));
                        }
                        *polls = u32::MAX;
                        // Asking about it once it has unpacked moves it into place (docs).
                        let (pid, version, installed, record) = (pid.clone(), version.clone(), installed.clone(), record.clone());
                        let mut answer = self.job_json(id, "done", "", "", "");
                        if installed.is_null() {
                            answer["state"] = "failed".into();
                            answer["error"] = "the package needs an asset library this repository does not offer".into();
                        } else {
                            answer["package"] = json!(pid);
                            answer["version"] = json!(version);
                            answer["installed"] = installed.clone();
                            if record.is_object() && !self.installed_packages.iter().any(|x| x["id"] == pid.as_str() && x["version"] == version.as_str()) {
                                self.installed_packages.push(record.clone());
                            }
                        }
                        Ok(answer)
                    }
                }
            }
            "packages.installed" => {
                self.fields(p, &[])?;
                if let Some((ids, n)) = self.removing.take() {
                    if n == 0 {
                        self.forget(&ids);
                    } else {
                        self.removing = Some((ids, n - 1));
                    }
                }
                Ok(json!({"packages": self.installed_packages}))
            }
            "packages.removal" | "packages.uninstall" => {
                let uninstall = method == "packages.uninstall";
                self.fields(p, if uninstall { &["id", "version", "withUnused", "evenIfUsed"] } else { &["id", "version"] })?;
                let id = p["id"].as_str().unwrap_or_default().to_string();
                let part = |x: &Value, bytes: bool| {
                    let mut o = json!({"id": x["id"], "version": x["version"], "kind": x["kind"], "name": x["name"]});
                    if bytes {
                        o["bytes"] = x.get("bytes").cloned().unwrap_or(json!(0));
                    }
                    o
                };
                let Some(package) = self.installed_packages.iter().find(|x| x["id"] == id.as_str() && (p["version"].is_null() || x["version"] == p["version"])).cloned() else {
                    return Err(refused(-32005, &format!("Performance: no package {id} is installed")));
                };
                let (dependents, unused) = self.removal.iter().find(|r| r.0 == id).map(|r| (r.1.clone(), r.2.clone())).unwrap_or_default();
                if !uninstall {
                    return Ok(json!({"package": part(&package, true), "dependents": dependents, "unused": unused}));
                }
                if !dependents.is_empty() && p["evenIfUsed"] != json!(true) {
                    let names: Vec<&str> = dependents.iter().filter_map(|d| d["name"].as_str()).collect();
                    return Err(refused(-32005, &format!("Performance: {} is used by {}; nothing was removed", package["name"].as_str().unwrap_or_default(), names.join(", "))));
                }
                let mut removed = vec![part(&package, false)];
                removed.extend(unused.iter().filter(|u| p["withUnused"] == json!(true) || p["withUnused"].as_array().is_some_and(|w| w.contains(&u["id"]))).map(|u| part(u, false)));
                let ids: Vec<String> = removed.iter().filter_map(|r| r["id"].as_str().map(String::from)).collect();
                if self.uninstall_lag == 0 {
                    self.forget(&ids);
                } else {
                    self.removing = Some((ids, self.uninstall_lag));
                }
                Ok(json!({"removed": removed}))
            }
            "packages.preinstalled" => {
                self.fields(p, &[])?;
                Ok(json!({"state": "none", "release": self.release, "error": "", "job": 0, "packages": []}))
            }
            _ => Err(refused(-32601, "Method not found")),
        }
    }

    /// One request, as the real engine answers it.
    pub fn handle(&mut self, method: &str, p: &Value) -> Result<Value, Error> {
        self.calls.push((method.to_string(), p.clone()));
        // During a long request every other request with an ID is refused at once (docs: "Long requests").
        if self.busy > 0 && !method.starts_with("session.") {
            self.busy -= 1;
            return Err(refused(BUSY, "The engine is busy"));
        }
        if let Some((_, why)) = self.fail.iter().find(|f| f.0 == method) {
            return Err(refused(-32005, why));
        }
        match method {
            "audio.devices" => {
                self.fields(p, &[])?;
                Ok(self.audio_json(self.audio.rate))
            }
            "audio.set" => self.audio_set(p),
            m if m.starts_with("controllers.") => {
                let r = self.controllers(m, p);
                if r.is_ok() && m != "controllers.inputs" {
                    let status = self.status();
                    self.notifications.push(("status".into(), status));
                }
                r
            }
            m if m.starts_with("keys.") => self.keys(m, p),
            m if m.starts_with("packages.") => self.packages(m, p),
            "library.libraries" => {
                self.fields(p, &[])?;
                let list: Vec<Value> = self.libraries.iter().map(|(id, name, items)| json!({"id": id, "name": name, "items": items})).collect();
                Ok(json!({"libraries": list}))
            }
            _ => Err(refused(-32601, "Method not found")),
        }
    }

    /// The status the engine pushes: what a page watches. A rate asked for has been applied by then.
    pub fn status(&mut self) -> Value {
        if let Some(r) = self.audio.pending_rate.take() {
            self.audio.rate = r;
        }
        json!({"audioReady": true, "audioDevice": self.audio.output, "sampleRate": self.audio.rate, "bufferSize": self.audio.size, "audioDeviceList": self.list, "midiPortList": self.midi_list, "cpuLoad": 0.1, "midiInputs": [], "midiOutputs": []})
    }
}
