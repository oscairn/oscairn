// SPDX-License-Identifier: AGPL-3.0-or-later
//! A fake engine for the Library app that answers as the real handlers of core/control/src/json_rpc.cpp do (the
//! request shapes and what they answer are in ui/LIBRARY-APP.md "Verified shapes"): the parameters a handler takes
//! and no others (`fields` refuses an unknown or a missing one with -32602), the answer's field names and types, and
//! what a request leaves behind (a favorite, an edit, a load making the item the most recent). Every request it was
//! asked is kept, in order, so a test can say what the screen sent.

use crate::client::Error;
use serde_json::{json, Value};

const BUSY: i64 = -32009;

fn refused(code: i64, message: &str) -> Error {
    Error { code, message: message.into() }
}

/// An item of the library as the fake keeps it.
#[derive(Clone, Debug)]
pub struct Stored {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub color: String,
    pub library: String,
    pub unfit: String,
    pub read_only: bool,
    pub in_place: bool,
    pub recent: u32,
}

pub fn stored(kind: &str, id: &str, name: &str) -> Stored {
    Stored { id: id.into(), kind: kind.into(), name: name.into(), description: String::new(), tags: vec![], favorite: false, color: String::new(), library: String::new(), unfit: String::new(), read_only: false, in_place: false, recent: 0 }
}

impl Stored {
    pub fn tags(mut self, tags: &[&str]) -> Stored {
        self.tags = tags.iter().map(|t| t.to_string()).collect();
        self
    }
    pub fn described(mut self, text: &str) -> Stored {
        self.description = text.into();
        self
    }
    pub fn starred(mut self) -> Stored {
        self.favorite = true;
        self
    }
    pub fn colored(mut self, hex: &str) -> Stored {
        self.color = hex.into();
        self
    }
    pub fn locked(mut self, library: &str) -> Stored {
        self.library = library.into();
        self
    }
    pub fn unfit(mut self, why: &str) -> Stored {
        self.unfit = why.into();
        self
    }
    fn json(&self) -> Value {
        let mut v = json!({"id": self.id, "kind": self.kind, "type": format!("oscairn.{}", self.kind), "name": self.name, "description": self.description,
            "path": format!("/library/{}s/{}", self.kind, self.id), "tags": self.tags, "favorite": self.favorite, "recent": self.recent, "color": self.color,
            "revision": format!("r-{}", self.id), "hash": format!("h-{}", self.id), "library": self.library});
        if !self.unfit.is_empty() {
            v["unfit"] = json!(self.unfit);
        }
        if self.read_only {
            v["readOnly"] = json!(true);
        }
        if self.in_place {
            v["inPlace"] = json!(true);
        }
        v
    }
}

pub struct Library {
    pub items: Vec<Stored>,
    /// Installed libraries: id, name.
    pub libraries: Vec<(String, String)>,
    pub tag_colors: Vec<(String, String)>,
    pub hidden: Vec<String>,
    pub plugin_packages: usize,
    /// The open Performance: id, name, whether it has unsaved changes.
    pub open: (String, String, bool),
    pub auditioning: Option<(String, String, u32)>,
    /// Whether a Performance opens beside the one playing (`budget.estimate`'s `seamless`).
    pub seamless: bool,
    /// The next this many requests that are not session or app ones are refused as busy, as during a load.
    pub busy: u32,
    pub preinstall: String,
    /// Every request, in order.
    pub calls: Vec<(String, Value)>,
    /// What a load (or any change of the Performance) makes the engine push, taken by the caller.
    pub notifications: Vec<(String, Value)>,
    /// A method that fails: its message.
    pub fail: Vec<(String, String)>,
    /// The setlists' documents, by their info's id.
    pub docs: Vec<Value>,
    /// Archives that can be read, by their file: what `library.contents` says of each.
    pub archives: Vec<(String, Value)>,
    /// The folders indexed in place.
    pub folders: Vec<String>,
    /// The plugin store (`plugins.list`).
    pub plugins: Vec<Value>,
    /// Signing keys: name, ID.
    pub keys: Vec<(String, String)>,
    /// The package repositories: address, name, enabled.
    pub repos: Vec<(String, String, bool)>,
    /// What each repository lists (`packages.job`'s `packages`), or why it cannot be read; a package may carry `_code`, the
    /// signers of its plugin code, which an install checks.
    pub repo_packages: Vec<(String, Result<Vec<Value>, String>)>,
    /// How many times a job is asked about before it ends.
    pub job_polls: u32,
    /// The jobs started: what each is, and how many questions it has had.
    jobs: Vec<(Value, u32)>,
    /// The packages installed from a repository: `packages.installed`'s records.
    pub installed_packages: Vec<Value>,
    /// The trust store.
    pub trusted: Vec<Value>,
    next: u32,
    recents: u32,
}

impl Library {
    pub fn new(items: Vec<Stored>) -> Library {
        Library {
            items,
            libraries: vec![],
            tag_colors: vec![],
            hidden: vec![],
            plugin_packages: 0,
            open: (String::new(), "Untitled".into(), false),
            auditioning: None,
            seamless: true,
            busy: 0,
            preinstall: "done".into(),
            calls: vec![],
            notifications: vec![],
            fail: vec![],
            docs: vec![],
            archives: vec![],
            folders: vec![],
            plugins: vec![],
            keys: vec![],
            repos: vec![],
            repo_packages: vec![],
            job_polls: 1,
            jobs: vec![],
            installed_packages: vec![],
            trusted: vec![],
            next: 100,
            recents: 0,
        }
    }

    pub fn methods(&self) -> Vec<String> {
        self.calls.iter().map(|c| c.0.clone()).collect()
    }

    pub fn called(&self, method: &str) -> Vec<Value> {
        self.calls.iter().filter(|c| c.0 == method).map(|c| c.1.clone()).collect()
    }

    /// The handler's own check of a request's fields (json_rpc.cpp `fields`): none outside `allowed`, and each of `required` present.
    fn fields(&self, p: &Value, allowed: &[&str], required: &[&str]) -> Result<(), Error> {
        if !p.is_object() {
            return Err(refused(-32602, "Invalid method parameters"));
        }
        for k in p.as_object().unwrap().keys() {
            if !allowed.contains(&k.as_str()) {
                return Err(refused(-32602, &format!("Unknown parameter {k}")));
            }
        }
        for k in required {
            if p.get(k).is_none() {
                return Err(refused(-32602, &format!("Missing {k}")));
            }
        }
        Ok(())
    }

    fn find(&mut self, id: &str) -> Result<&mut Stored, Error> {
        self.items.iter_mut().find(|i| i.id == id).ok_or_else(|| refused(-32005, "No such item"))
    }

    /// What `library.query` gives (json_rpc.cpp:925-955).
    fn query(&mut self, p: &Value) -> Result<Value, Error> {
        self.fields(p, &["text", "tags", "favorites", "recent", "limit", "kind", "type", "source", "id"], &[])?;
        if let Some(l) = p.get("limit") {
            let n = l.as_u64().unwrap_or(0);
            if n == 0 || n > 1000 {
                return Err(refused(-32602, "Invalid limit"));
            }
        }
        if let Some(k) = p.get("kind").and_then(Value::as_str) {
            if !["performance", "sound", "setlist"].contains(&k) {
                return Err(refused(-32602, "Unknown library kind"));
            }
        }
        let text = p.get("text").and_then(Value::as_str).unwrap_or_default().to_lowercase();
        let tags: Vec<String> = p.get("tags").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        let favorites = p.get("favorites").and_then(Value::as_bool).unwrap_or(false);
        let recent = p.get("recent").and_then(Value::as_bool).unwrap_or(false);
        let source = p.get("source").and_then(Value::as_str).unwrap_or_default();
        let id = p.get("id").and_then(Value::as_str).unwrap_or_default();
        let mut kind = p.get("kind").and_then(Value::as_str).unwrap_or_default();
        // `type`: item types by ID, one or a list (docs: RFC 0023); a setlist's is oscairn.setlist.
        let types: Vec<&str> = match p.get("type") {
            Some(Value::String(t)) => vec![t.as_str()],
            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        for t in &types {
            if !["oscairn.performance", "oscairn.sound", "oscairn.setlist"].contains(t) {
                return Err(refused(-32602, "Unknown item type"));
            }
        }
        if types.len() == 1 {
            kind = types[0].trim_start_matches("oscairn.");
        }
        let mut hits: Vec<&Stored> = self
            .items
            .iter()
            .filter(|i| (kind.is_empty() || i.kind == kind) && (id.is_empty() || i.id == id))
            .filter(|i| source.is_empty() || (source == "mine" && i.library.is_empty()) || i.library == source)
            .filter(|i| !favorites || i.favorite)
            .filter(|i| !recent || i.recent > 0)
            .filter(|i| tags.iter().all(|t| i.tags.contains(t)))
            .filter(|i| text.is_empty() || [i.name.to_lowercase(), i.description.to_lowercase(), i.tags.join(" ").to_lowercase()].iter().any(|f| f.contains(&text)))
            .collect();
        if recent {
            hits.sort_by_key(|i| i.recent);
        } else {
            hits.sort_by_key(|i| i.name.to_lowercase());
        }
        let total = hits.len();
        let mut counts: Vec<(String, u64)> = vec![];
        for i in &hits {
            for t in i.tags.iter().filter(|t| !self.hidden.contains(t)) {
                match counts.iter_mut().find(|c| c.0 == *t) {
                    Some(c) => c.1 += 1,
                    None => counts.push((t.clone(), 1)),
                }
            }
        }
        let limit = p.get("limit").and_then(Value::as_u64).unwrap_or(200) as usize;
        Ok(json!({
            "items": hits.iter().take(limit).map(|i| i.json()).collect::<Vec<_>>(),
            "tags": counts.iter().map(|(t, n)| json!({"tag": t, "count": n})).collect::<Vec<_>>(),
            "total": total,
            "tagColors": self.tag_colors.iter().map(|(t, c)| json!({"tag": t, "color": c})).collect::<Vec<_>>(),
            "hiddenTags": self.hidden,
        }))
    }

    fn changed(&mut self) {
        self.notifications.push(("graph.changed".into(), json!({})));
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
            "library.query" => self.query(p),
            "library.libraries" => {
                self.fields(p, &[], &[])?;
                let list: Vec<Value> = self.libraries.iter().map(|(id, name)| json!({"id": id, "name": name, "items": self.items.iter().filter(|i| i.library == *id).count()})).collect();
                Ok(json!({"libraries": list}))
            }
            "library.favorite" => {
                self.fields(p, &["id", "favorite"], &["id", "favorite"])?;
                let on = p["favorite"].as_bool().ok_or_else(|| refused(-32602, "favorite"))?;
                self.find(p["id"].as_str().unwrap_or_default())?.favorite = on;
                Ok(json!({"accepted": true}))
            }
            "library.tag" => {
                self.fields(p, &["tag", "hidden", "color"], &["tag"])?;
                let tag = p["tag"].as_str().unwrap_or_default().to_string();
                self.hidden.retain(|t| *t != tag);
                if p["hidden"] == true {
                    self.hidden.push(tag.clone());
                }
                self.tag_colors.retain(|t| t.0 != tag);
                if let Some(c) = p["color"].as_str().filter(|c| !c.is_empty()) {
                    self.tag_colors.push((tag, c.to_string()));
                }
                Ok(json!({"accepted": true}))
            }
            "library.edit" => {
                self.fields(p, &["id", "name", "description", "tags", "color"], &["id", "name", "description", "tags"])?;
                let item = self.find(p["id"].as_str().unwrap_or_default())?;
                if !item.library.is_empty() {
                    return Err(refused(-32005, "A locked library's items cannot be edited"));
                }
                item.name = p["name"].as_str().unwrap_or_default().into();
                item.description = p["description"].as_str().unwrap_or_default().into();
                item.tags = p["tags"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                item.color = p["color"].as_str().unwrap_or_default().into();
                Ok(json!({"accepted": true}))
            }
            "library.copy" => {
                self.fields(p, &["id"], &["id"])?;
                let mut copy = self.find(p["id"].as_str().unwrap_or_default())?.clone();
                self.next += 1;
                copy.id = format!("copy-{}", self.next);
                if copy.library.is_empty() {
                    copy.name = format!("{} copy", copy.name);
                }
                copy.library.clear();
                copy.favorite = false;
                let id = copy.id.clone();
                self.items.push(copy);
                Ok(json!({"id": id}))
            }
            "library.delete" => {
                self.fields(p, &["id"], &["id"])?;
                let id = p["id"].as_str().unwrap_or_default().to_string();
                self.find(&id)?;
                self.items.retain(|i| i.id != id);
                Ok(json!({"accepted": true}))
            }
            "library.save" => {
                self.fields(p, &["name", "new"], &[])?;
                let name = p.get("name").and_then(Value::as_str).map(String::from).unwrap_or_else(|| self.open.1.clone());
                let id = if self.open.0.is_empty() || p["new"] == true {
                    self.next += 1;
                    let id = format!("saved-{}", self.next);
                    self.items.push(stored("performance", &id, &name));
                    id
                } else {
                    self.open.0.clone()
                };
                self.open = (id.clone(), name, false);
                self.changed();
                Ok(json!({"id": id}))
            }
            "library.load" => {
                self.fields(p, &["id"], &["id"])?;
                let id = p["id"].as_str().unwrap_or_default().to_string();
                let item = self.find(&id)?.clone();
                if !item.unfit.is_empty() {
                    return Err(refused(-32005, &format!("{} does not fit this device: {}", item.name, item.unfit)));
                }
                self.recents += 1;
                let n = self.recents;
                self.find(&id)?.recent = n;
                self.open = (id, item.name, false);
                self.auditioning = None;
                self.changed();
                Ok(json!({"loaded": true}))
            }
            "packages.repositories" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"repositories": self.repos.iter().map(|(a, n, e)| json!({"address": a, "name": n, "key": "k", "id": "0123456789abcdef", "origin": "user", "enabled": e})).collect::<Vec<_>>()}))
            }
            "packages.list" | "packages.install" => {
                let install = method == "packages.install";
                if install {
                    self.fields(p, &["source", "id", "version", "acceptUnsignedCode", "replace"], &["source", "id"])?;
                } else {
                    self.fields(p, &["source"], &["source"])?;
                }
                self.jobs.push((json!({"method": method, "params": p}), 0));
                Ok(json!({"job": self.jobs.len()}))
            }
            "packages.job" => {
                self.fields(p, &["job"], &["job"])?;
                let at = p["job"].as_u64().unwrap_or(0) as usize;
                if at == 0 || at > self.jobs.len() {
                    return Err(refused(-32602, "No such job"));
                }
                self.jobs[at - 1].1 += 1;
                let (what, asked) = self.jobs[at - 1].clone();
                let id = at as u64;
                let mut doc = json!({"job": id, "state": "running", "error": "", "repository": "", "name": "", "claimedName": "", "rootKey": "", "serial": 1, "packages": [], "package": "", "version": "", "installed": {"added": 0, "skipped": 0, "notes": [], "code": []}});
                if asked <= self.job_polls {
                    return Ok(doc);
                }
                let params = what["params"].clone();
                let source = params["source"].as_str().unwrap_or_default().to_string();
                let listed = self.repo_packages.iter().find(|r| r.0 == source).map(|r| r.1.clone());
                if what["method"] == "packages.list" {
                    match listed {
                        Some(Ok(packages)) => {
                            doc["state"] = json!("done");
                            doc["name"] = json!(self.repos.iter().find(|r| r.0 == source).map(|r| r.1.clone()).unwrap_or_default());
                            doc["packages"] = json!(packages.iter().map(|x| { let mut x = x.clone(); if let Some(o) = x.as_object_mut() { o.remove("_code"); } x }).collect::<Vec<_>>());
                        }
                        Some(Err(why)) => {
                            doc["state"] = json!("failed");
                            doc["error"] = json!(why);
                        }
                        None => {
                            doc["state"] = json!("failed");
                            doc["error"] = json!("cannot reach the repository");
                        }
                    }
                    return Ok(doc);
                }
                // An install.
                let id_wanted = params["id"].as_str().unwrap_or_default().to_string();
                let package = listed.and_then(|l| l.ok()).and_then(|l| l.into_iter().find(|x| x["id"] == id_wanted));
                let Some(package) = package else {
                    doc["state"] = json!("failed");
                    doc["error"] = json!(format!("refused: no package {id_wanted}"));
                    return Ok(doc);
                };
                let code = package["_code"].clone();
                let accept = params["acceptUnsignedCode"] == true;
                // Code from a key not trusted, or unsigned, is refused unless the user said yes (RFC 0006, RFC 0031).
                let blocked = code.as_array().is_some_and(|c| c.iter().any(|s| s["status"] == "unsigned" || (s["status"] == "unknown" && !self.trusted.iter().any(|t| t["key"] == s["publicKey"]))));
                if blocked && !accept {
                    doc["state"] = json!("failed");
                    doc["error"] = json!("refused: the plugins in this package run code from an unknown source");
                    doc["name"] = json!(self.repos.iter().find(|r| r.0 == source).map(|r| r.1.clone()).unwrap_or_default());
                    doc["installed"]["code"] = code;
                    return Ok(doc);
                }
                let version = params["version"].as_str().map(String::from).unwrap_or_else(|| package["versions"][0]["version"].as_str().unwrap_or_default().to_string());
                self.installed_packages.retain(|x| x["id"] != id_wanted);
                self.installed_packages.push(json!({"id": id_wanted, "version": version, "kind": "plugin", "repository": source, "sha256": package["versions"][0]["sha256"], "signers": [], "otherBuild": false, "withdrawn": null, "dependency": false}));
                doc["state"] = json!("done");
                doc["package"] = json!(package["name"]);
                doc["version"] = json!(version);
                doc["installed"] = json!({"added": 1, "skipped": 0, "notes": ["Its builds are in the plugin store"], "code": []});
                Ok(doc)
            }
            "trust.list" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"keys": self.trusted}))
            }
            "trust.set" => {
                self.fields(p, &["key", "name", "scope", "revoked"], &["key", "name", "scope"])?;
                self.trusted.retain(|t| t["key"] != p["key"]);
                self.trusted.push(json!({"id": "x", "key": p["key"], "name": p["name"], "scope": p["scope"], "revoked": p["revoked"] == true}));
                Ok(json!({"accepted": true}))
            }
            "performance.contents" => {
                self.fields(p, &["directory"], &["directory"])?;
                Ok(json!({"parts": [{"id": 1, "name": "Lead", "bus": false}, {"id": 2, "name": "Bus A", "bus": true}], "scenes": [{"id": 1, "name": "Verse"}, {"id": 2, "name": "Chorus"}]}))
            }
            "performance.import" => {
                self.fields(p, &["directory", "parts", "scenes"], &["directory"])?;
                self.changed();
                Ok(json!({"parts": [3], "notes": []}))
            }
            "library.contents" => {
                self.fields(p, &["file"], &["file"])?;
                let file = p["file"].as_str().unwrap_or_default();
                self.archives.iter().find(|a| a.0 == file).map(|a| a.1.clone()).ok_or_else(|| refused(-32005, &format!("Cannot read {file}: no such archive")))
            }
            "library.install" => {
                self.fields(p, &["file", "asLibrary", "items", "acceptUnsignedCode"], &["file"])?;
                let file = p["file"].as_str().unwrap_or_default();
                let contents = self.archives.iter().find(|a| a.0 == file).map(|a| a.1.clone()).ok_or_else(|| refused(-32005, "no such archive"))?;
                let accept = p["acceptUnsignedCode"] == true;
                let code: Vec<Value> = contents["code"].as_array().cloned().unwrap_or_default();
                // Plugin code from a key not trusted, or unsigned, is refused unless the user saw it and said yes (RFC 0006).
                if !accept && code.iter().any(|c| c["status"] == "unknown" || c["status"] == "unsigned") {
                    return Err(refused(-32005, "The plugins in this archive run code from an unknown source"));
                }
                let wanted: Vec<String> = p["items"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
                let locked = p["asLibrary"] == true;
                let library_id = contents["id"].as_str().unwrap_or_default().to_string();
                let (mut added, mut skipped) = (0, 0);
                for i in contents["items"].as_array().into_iter().flatten() {
                    let id = i["id"].as_str().unwrap_or_default().to_string();
                    if !wanted.is_empty() && !wanted.contains(&id) {
                        continue;
                    }
                    if self.items.iter().any(|x| x.id == id) {
                        skipped += 1;
                        continue;
                    }
                    let mut item = stored(i["kind"].as_str().unwrap_or("performance"), &id, i["name"].as_str().unwrap_or_default());
                    if locked {
                        item.library = library_id.clone();
                        if !self.libraries.iter().any(|l| l.0 == library_id) {
                            self.libraries.push((library_id.clone(), contents["name"].as_str().unwrap_or_default().to_string()));
                        }
                    }
                    self.items.push(item);
                    added += 1;
                }
                Ok(json!({"added": added, "skipped": skipped, "notes": [], "code": code}))
            }
            "library.export" | "library.exportLibrary" => {
                let export = method == "library.export";
                if export {
                    self.fields(p, &["id", "file", "editedSounds", "sign"], &["id", "file"])?;
                    self.find(p["id"].as_str().unwrap_or_default())?;
                } else {
                    self.fields(p, &["file", "name", "setlists", "sign"], &["file"])?;
                }
                if let Some(s) = p.get("sign") {
                    self.fields(s, &["key", "items", "archive"], &["key"])?;
                    if !self.keys.iter().any(|k| s["key"] == k.0) {
                        return Err(refused(-32005, "No such signing key"));
                    }
                }
                let file = std::path::PathBuf::from(p["file"].as_str().unwrap_or_default());
                // It refuses a file that exists (docs: performance.export).
                if file.exists() {
                    return Err(refused(-32005, &format!("{} exists already", file.display())));
                }
                std::fs::write(&file, b"an archive").map_err(|e| refused(-32005, &e.to_string()))?;
                Ok(json!({"exported": true, "notes": []}))
            }
            "library.folders" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"folders": self.folders}))
            }
            "library.addFolder" | "library.removeFolder" => {
                self.fields(p, &["folder"], &["folder"])?;
                let folder = p["folder"].as_str().unwrap_or_default().to_string();
                if method == "library.addFolder" {
                    self.folders.push(folder);
                } else {
                    self.folders.retain(|f| *f != folder);
                }
                Ok(json!({"accepted": true}))
            }
            "plugins.list" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"plugins": self.plugins}))
            }
            "plugins.remove" | "plugins.addBuild" => {
                let add = method == "plugins.addBuild";
                if add {
                    self.fields(p, &["format", "id", "version", "platform", "path"], &["format", "id", "version", "path"])?;
                } else {
                    self.fields(p, &["format", "id", "version", "platform"], &["format", "id", "version"])?;
                    let (id, version) = (p["id"].as_str().unwrap_or_default().to_string(), p["version"].as_str().unwrap_or_default().to_string());
                    self.plugins.retain(|x| !(x["id"] == id && x["version"] == version));
                }
                Ok(json!({"accepted": true}))
            }
            "library.uninstall" => {
                self.fields(p, &["library"], &["library"])?;
                let id = p["library"].as_str().unwrap_or_default().to_string();
                self.libraries.retain(|l| l.0 != id);
                self.items.retain(|i| i.library != id);
                Ok(json!({"accepted": true}))
            }
            "keys.list" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"keys": self.keys.iter().map(|(n, id)| json!({"name": n, "id": id, "key": format!("pub-{id}")})).collect::<Vec<_>>()}))
            }
            "keys.make" => {
                self.fields(p, &["name"], &["name"])?;
                let name = p["name"].as_str().unwrap_or_default().to_string();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || ".-_".contains(c)) {
                    return Err(refused(-32602, "A key's name is letters, digits, '.', '-' and '_'"));
                }
                self.next += 1;
                let id = format!("{:016x}", u64::from(self.next).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                self.keys.push((name.clone(), id.clone()));
                Ok(json!({"name": name, "id": id, "key": format!("pub-{id}")}))
            }
            "setlists.get" => {
                self.fields(p, &["id"], &["id"])?;
                let id = p["id"].as_str().unwrap_or_default();
                self.docs.iter().find(|d| d["info"]["id"] == id).map(|d| json!({"setlist": d})).ok_or_else(|| refused(-32005, "No such setlist"))
            }
            "setlists.save" => {
                self.fields(p, &["setlist"], &["setlist"])?;
                let doc = p["setlist"].clone();
                // A reader refuses a member it does not know, and a missing one (docs/setlist-format.md).
                let info = doc["info"].as_object().ok_or_else(|| refused(-32602, "info"))?;
                for k in ["id", "name", "description", "tags", "color", "revision", "parent", "hash", "origin"] {
                    if !info.contains_key(k) {
                        return Err(refused(-32602, &format!("Missing info.{k}")));
                    }
                }
                if doc["format"] != "oscairn.setlist" || doc["version"] != 2 || !doc["pages"].is_array() {
                    return Err(refused(-32602, "Not a setlist"));
                }
                // setlist.cpp `readSetlist`: the display has its mode, columns and rows, whatever the mode (1 to 8 each).
                let display = doc["display"].as_object().ok_or_else(|| refused(-32602, "not a usable setlist: its display is not an object"))?;
                if !["list", "scroll", "grid"].contains(&display.get("mode").and_then(Value::as_str).unwrap_or_default()) {
                    return Err(refused(-32602, "not a usable setlist: its display's mode is not list, scroll or grid"));
                }
                for k in ["columns", "rows"] {
                    if !display.get(k).and_then(Value::as_u64).is_some_and(|n| (1..=8).contains(&n)) {
                        return Err(refused(-32602, &format!("not a usable setlist: its display has no {k}")));
                    }
                }
                if display.keys().any(|k| !["mode", "columns", "rows"].contains(&k.as_str())) {
                    return Err(refused(-32602, "not a usable setlist: its display has a member it does not know"));
                }
                let mut id = doc["info"]["id"].as_str().unwrap_or_default().to_string();
                let mut doc = doc;
                if !self.docs.iter().any(|d| d["info"]["id"] == id) {
                    self.next += 1;
                    id = format!("setlist-{}", self.next);
                    doc["info"]["id"] = json!(id);
                    let name = doc["info"]["name"].as_str().unwrap_or_default().to_string();
                    self.items.push(stored("setlist", &id, &name));
                    self.docs.push(doc);
                } else if let Some(d) = self.docs.iter_mut().find(|d| d["info"]["id"] == id) {
                    *d = doc;
                }
                Ok(json!({"id": id}))
            }
            "performance.info" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"id": self.open.0, "name": self.open.1, "description": "", "tags": [], "color": "", "unsaved": self.open.2}))
            }
            "budget.estimate" => {
                self.fields(p, &["directory"], &[])?;
                Ok(json!({"cpu": 0.31, "memory": 412_000_000u64, "fits": true, "seamless": self.seamless, "longest": 1.2, "cores": 4, "uncertain": false,
                    "budget": {"cpu": 0.9, "memory": 1_800_000_000u64}, "plugins": []}))
            }
            "performance.prepare" => {
                self.fields(p, &["directory"], &["directory"])?;
                Ok(json!({"prepared": true}))
            }
            "parts.fromSound" => {
                self.fields(p, &["id"], &["id"])?;
                self.find(p["id"].as_str().unwrap_or_default())?;
                self.changed();
                Ok(json!({"part": 7}))
            }
            "audition.start" => {
                self.fields(p, &["id", "part"], &["id"])?;
                let item = self.find(p["id"].as_str().unwrap_or_default())?.clone();
                self.auditioning = Some((item.id, item.kind, p["part"].as_u64().unwrap_or(0) as u32));
                self.changed();
                Ok(json!({"accepted": true}))
            }
            "audition.stop" => {
                self.fields(p, &["keep"], &[])?;
                self.auditioning = None;
                self.changed();
                Ok(json!({"accepted": true}))
            }
            "audition.get" => {
                self.fields(p, &[], &[])?;
                let (id, kind, part) = self.auditioning.clone().unwrap_or_default();
                Ok(json!({"id": id, "kind": kind, "part": part}))
            }
            "packages.installed" => {
                self.fields(p, &[], &[])?;
                let mut list: Vec<Value> = (0..self.plugin_packages)
                    .map(|i| json!({"id": format!("org.example.plugin{i}"), "version": "1.0.0", "kind": "plugin", "repository": "", "sha256": "", "signers": [], "otherBuild": false, "withdrawn": null, "dependency": false}))
                    .collect();
                list.extend(self.installed_packages.clone());
                Ok(json!({"packages": list}))
            }
            "packages.preinstalled" => {
                self.fields(p, &[], &[])?;
                Ok(json!({"state": self.preinstall, "release": "1", "error": "", "job": 0, "packages": []}))
            }
            other => Err(refused(-32601, &format!("Method not implemented: {other}"))),
        }
    }
}
