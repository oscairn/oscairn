// SPDX-License-Identifier: AGPL-3.0-or-later
//! Installing, exporting and managing what the library holds (ui/LIBRARY-APP.md C7, C8, D1 to D3, D8 to D12; LibraryPane.cpp
//! `installMenu`, `install`, `exportItem`, `exportLibrary`, `librariesMenu`, `removePlugin`, `addPluginBuild`): the
//! Install and Libraries menus, the file chooser's results, the install and export choices, the folders indexed in place,
//! the plugins in the store, a library uninstalled, a signing key made.

use super::export::{Export, Key, Whole};
use super::files::{self, After, Files, Pressed, Want};
use super::install::Install;
use super::jobs::Kind as JobKind;
use super::*;
use std::path::PathBuf;

/// This device's platform as the plugin store names it (package.cpp `platform`).
pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, if std::env::consts::ARCH == "aarch64" { "arm64" } else { std::env::consts::ARCH })
}

/// Every platform a build may be for.
const PLATFORMS: [&str; 5] = ["linux-arm64", "linux-x86_64", "macos-arm64", "macos-x86_64", "windows-x86_64"];

/// A plugin the store holds, or one the open Performance uses that it does not.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginRef {
    pub format: String,
    pub id: String,
    pub version: String,
    pub name: String,
}

impl PluginRef {
    fn title(&self) -> String {
        format!("{} {}", if self.name.is_empty() { &self.id } else { &self.name }, self.version)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StoredPlugin {
    pub r: PluginRef,
    pub platforms: Vec<String>,
    pub bytes: u64,
    pub used_by: Vec<String>,
}

/// What the Libraries menu was made of, so a choice is decoded as it was shown.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    folders: Vec<String>,
    hidden: Vec<String>,
    plugins: Vec<StoredPlugin>,
    /// The open Performance's CLAP plugins the store does not hold.
    in_performance: Vec<PluginRef>,
    installed: Vec<Installed>,
}

/// Answers the Libraries menu waits for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Waiting {
    pub folders: Option<Vec<String>>,
    pub plugins: Option<Vec<StoredPlugin>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Req {
    /// library.folders.
    Folders,
    /// plugins.list.
    Plugins,
    /// keys.list; the key to choose once it is listed.
    Keys(Option<String>),
    /// keys.make.
    MakeKey,
    /// library.query of the setlists, for the library's export.
    Setlists,
    /// library.contents of the archive chosen.
    Contents,
    /// performance.contents of the Performance to import from.
    Held,
}

/// What a menu was opened for here.
#[derive(Clone, Debug, PartialEq)]
pub enum Menus {
    Install,
    Libraries(Box<Snapshot>),
    /// The platform a build is for: the plugin and the build's path.
    Platform(PluginRef, PathBuf),
    /// The key an export is signed with.
    Signature(Vec<Key>),
}

/// Menu entries' numbers (LibraryPane.cpp `librariesMenu`).
const EXPORT_LIBRARY: usize = 1;
const ADD_FOLDER: usize = 2999;
const FOLDER: usize = 3000;
const TAG: usize = 1000;
const BUILD: usize = 100_000;
const NO_KEY: usize = 1;
const MAKE_KEY: usize = 1000;

impl Screen {
    pub(super) fn enter(&mut self, mode: Mode) {
        let old = std::mem::replace(&mut self.mode, mode);
        self.stack.push(old);
        self.b.reset();
        self.tab_at = None;
    }

    /// Back out of a screen made by `enter`.
    pub(super) fn leave(&mut self) {
        self.mode = self.stack.pop().unwrap_or(Mode::List);
        self.b.reset_cursor();
        self.tab_at = None;
    }

    /// A problem said in a question with one answer, as the JUCE app's alert window ("Not installed").
    pub(super) fn alert(&mut self, title: &str, message: &str) {
        self.notice = Some((title.to_string(), message.to_string(), vec!["OK".into()]));
        self.notice_for = Some(NoticeFor::Alert);
    }

    // ---- Install... ----

    pub(super) fn install_menu(&mut self) {
        self.menu = Some(("Install".into(), vec![menu::Item::choice("Catalog", 1), menu::Item::choice("File", 2)]));
        self.menu_for = Some(MenuFor::Library(Menus::Install));
    }

    /// Tests: the folder the chooser starts in.
    #[cfg(test)]
    pub fn set_files_dir(&mut self, dir: PathBuf) {
        self.files_dir = Some(dir);
    }

    pub(super) fn files_open(&mut self, want: Want, ext: &'static str, name: &str, after: After, title: &str, music: bool) {
        let dir = self.files_dir.clone().unwrap_or_else(|| files::start_dir(music));
        self.enter(Mode::Files(Files::new(dir, want, ext, name, after, title)));
    }

    /// A row of the file chooser pressed.
    pub(super) fn files_press(&mut self, at: usize) -> Outcome {
        let Mode::Files(files) = &mut self.mode else { return Outcome::Stay };
        let pressed = files.press(at);
        let (name, after, want) = (files.name.clone(), files.after.clone(), files.want);
        match pressed {
            Pressed::AskName => {
                self.asking = Some(Asking::FileName);
                return Outcome::Prompt("The file's name".into(), name);
            }
            Pressed::Chosen(path) => {
                if want == Want::Save && path.exists() {
                    let shown = path.file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
                    self.notice = Some((format!("Replace {shown}?"), "A file of that name is there already. It is replaced once the new one is written.".into(), vec!["Replace".into(), "Cancel".into()]));
                    self.notice_for = Some(NoticeFor::Overwrite(path, after));
                } else {
                    self.files_chosen(path, after);
                }
            }
            Pressed::Moved | Pressed::Named | Pressed::Nothing => {}
        }
        Outcome::Stay
    }

    /// The path chosen: what it was for happens.
    pub(super) fn files_chosen(&mut self, path: PathBuf, after: After) {
        if let Some(dir) = path.parent().filter(|_| path.is_file() || matches!(after, After::ExportItem(_) | After::ExportLibrary(..))) {
            self.files_dir = Some(dir.to_path_buf());
        }
        match after {
            After::Install => {
                self.say("Reading the archive...");
                self.push(Request::Lib(Req::Contents), "library.contents", json!({"file": path.display().to_string()}));
                self.install_file = Some(path);
            }
            After::ExportItem(id) => {
                let Some(item) = self.items.iter().find(|i| i.id == id).cloned() else { return self.leave() };
                self.leave();
                self.enter(Mode::Export(Export { id, name: item.name.clone(), performance: item.kind == "performance", file: path, edited: false, key: None, keys: vec![] }));
                self.push(Request::Lib(Req::Keys(None)), "keys.list", json!({}));
            }
            After::ExportLibrary(name, setlists) => {
                self.leave();
                let (file, replace) = export::plan(&path);
                let whole = Whole { name, setlists: setlists.into_iter().map(|id| (id, String::new(), true)).collect() };
                self.start_job(JobKind::ExportLibrary(replace), "library.exportLibrary", whole.request(&file), "Exporting...");
            }
            After::Folder => {
                self.leave();
                let shown = path.file_name().map_or(path.display().to_string(), |n| n.to_string_lossy().into_owned());
                self.start_job(JobKind::AddFolder(shown), "library.addFolder", json!({"folder": path.display().to_string()}), "Indexing...");
            }
            After::Build(plugin) => {
                self.leave();
                let items: Vec<menu::Item> = PLATFORMS.iter().enumerate().filter(|(_, p)| **p != platform()).map(|(i, p)| menu::Item::choice(format!("For {p}"), i + 1)).collect();
                self.menu = Some((format!("A build of {}", plugin.title()), items));
                self.menu_for = Some(MenuFor::Library(Menus::Platform(*plugin, path)));
            }
        }
    }

    /// The question about replacing a file, answered.
    pub(super) fn overwrite_answer(&mut self, path: PathBuf, after: After, choice: Option<usize>) {
        if choice == Some(0) {
            self.files_chosen(path, after);
        }
    }

    // ---- the Install choices ----

    pub(super) fn contents_reply(&mut self, result: Result<Value, Error>) {
        let Some(file) = self.install_file.take() else { return };
        match result {
            Ok(r) => {
                self.say("");
                self.leave(); // the file chooser
                self.enter(Mode::Install(Box::new(Install::from_json(file, &r))));
            }
            Err(e) => {
                self.say("");
                self.alert("Not installed", &e.message);
            }
        }
    }

    pub(super) fn install_press(&mut self, at: usize) {
        let Mode::Install(install) = &mut self.mode else { return };
        let radios = if install.package() { 0 } else { 2 };
        let rows = install.rows().len();
        if at < radios {
            install.own = at == 0;
        } else if at < rows - 2 {
            let i = at - radios;
            install.items[i].ticked = !install.items[i].ticked;
        } else if at == rows - 2 {
            match install.request() {
                Ok(params) => {
                    self.start_job(JobKind::Install, "library.install", params, "Installing...");
                    self.leave();
                }
                Err(why) => self.say(why),
            }
        } else {
            self.leave();
        }
    }

    // ---- the Export choices ----

    pub(super) fn export_press(&mut self, at: usize) {
        let Mode::Export(e) = &mut self.mode else { return };
        let lead = usize::from(e.performance);
        if e.performance && at == 0 {
            e.edited = !e.edited;
        } else if at == lead {
            // The signature: not signed, one of the keys, or a new one.
            let mut items = vec![menu::Item::ticked("Not signed", NO_KEY, e.key.is_none())];
            items.extend(e.keys.iter().enumerate().map(|(i, k)| menu::Item::ticked(format!("Sign with {}", k.name), NO_KEY + 1 + i, e.key.as_deref() == Some(k.name.as_str()))));
            items.push(menu::Item::choice("Make a key...", MAKE_KEY));
            let keys = e.keys.clone();
            self.menu = Some(("Signature".into(), items));
            self.menu_for = Some(MenuFor::Library(Menus::Signature(keys)));
        } else if at == lead + 1 {
            let e = e.clone();
            let (file, replace) = export::plan(&e.file);
            let key = e.key.clone();
            self.start_job(JobKind::Export(replace, key), "library.export", e.request(&file), "Exporting...");
            self.leave();
        } else {
            self.leave();
        }
    }

    /// A choice in the signature menu.
    fn signature_chosen(&mut self, keys: Vec<Key>, choice: usize) -> Outcome {
        if choice == MAKE_KEY {
            self.asking = Some(Asking::KeyName);
            return Outcome::Prompt("Make a signing key: its name".into(), String::new());
        }
        if let Mode::Export(e) = &mut self.mode {
            e.key = if choice == NO_KEY { None } else { keys.get(choice - NO_KEY - 1).map(|k| k.name.clone()) };
        }
        Outcome::Stay
    }

    pub(super) fn key_named(&mut self, name: &str) {
        self.push(Request::Lib(Req::MakeKey), "keys.make", json!({"name": name.trim()}));
        self.made_key = Some(name.trim().to_string());
    }

    // ---- the library's export ----

    pub(super) fn export_library_start(&mut self) {
        let mut params = Filters::default().params(Kind::Setlist);
        params["type"] = json!(["oscairn.setlist"]);
        if let Some(o) = params.as_object_mut() {
            o.remove("kind");
        }
        self.push(Request::Lib(Req::Setlists), "library.query", params);
    }

    pub(super) fn export_library_press(&mut self, at: usize) -> Outcome {
        let Mode::ExportLibrary(w) = &mut self.mode else { return Outcome::Stay };
        let rows = w.rows().len();
        if at == 0 {
            self.asking = Some(Asking::LibraryName);
            return Outcome::Prompt("The library's name".into(), w.name.clone());
        } else if at < rows - 2 {
            w.setlists[at - 1].2 = !w.setlists[at - 1].2;
        } else if at == rows - 2 {
            let (name, setlists) = (w.name.clone(), w.setlists.iter().filter(|s| s.2).map(|s| s.0.clone()).collect());
            self.leave();
            let name_shown = if name.trim().is_empty() { "Library".to_string() } else { name.trim().to_string() };
            self.files_open(Want::Save, "olib", &name_shown, After::ExportLibrary(name, setlists), "Export library", false);
        } else {
            self.leave();
        }
        Outcome::Stay
    }

    // ---- Libraries... ----

    pub(super) fn libraries_menu_start(&mut self) {
        self.waiting_menu = Some(Waiting::default());
        self.push(Request::Lib(Req::Folders), "library.folders", json!({}));
        self.push(Request::Lib(Req::Plugins), "plugins.list", json!({}));
    }

    /// An answer to what the Libraries menu waited for; the menu opens once both are in.
    pub(super) fn lib_reply(&mut self, request: Req, result: Result<Value, Error>) {
        match (request, result) {
            (Req::Folders, Ok(r)) => {
                if let Some(w) = &mut self.waiting_menu {
                    w.folders = Some(r["folders"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect());
                }
                self.menu_ready();
            }
            (Req::Plugins, Ok(r)) => {
                if let Some(w) = &mut self.waiting_menu {
                    w.plugins = Some(
                        r["plugins"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|p| {
                                let text = |k: &str| p[k].as_str().unwrap_or_default().to_string();
                                let list = |k: &str| p[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>();
                                StoredPlugin { r: PluginRef { format: text("format"), id: text("id"), version: text("version"), name: text("name") }, platforms: list("platforms"), bytes: p["bytes"].as_u64().unwrap_or(0), used_by: list("usedBy") }
                            })
                            .collect(),
                    );
                }
                self.menu_ready();
            }
            (Req::Folders | Req::Plugins, Err(e)) => {
                self.waiting_menu = None;
                self.say(if e.code == BUSY { "The engine is busy; try again in a moment.".to_string() } else { e.message });
            }
            (Req::Keys(choose), Ok(r)) => {
                let keys: Vec<Key> = r["keys"].as_array().into_iter().flatten().filter_map(Key::from_json).collect();
                if let Mode::Export(e) = &mut self.mode {
                    e.keys = keys;
                    if let Some(name) = choose {
                        e.key = e.keys.iter().find(|k| k.name == name).map(|k| k.name.clone());
                    }
                }
            }
            (Req::Keys(_), Err(e)) => self.say(e.message),
            (Req::MakeKey, Ok(r)) => {
                let name = r["name"].as_str().map(String::from).or_else(|| self.made_key.take());
                self.push(Request::Lib(Req::Keys(name)), "keys.list", json!({}));
            }
            (Req::MakeKey, Err(e)) => {
                self.made_key = None;
                self.alert("No key made", &e.message);
            }
            (Req::Setlists, Ok(r)) => {
                let setlists = r["items"].as_array().into_iter().flatten().filter_map(|s| Some((s["id"].as_str()?.to_string(), s["name"].as_str().unwrap_or_default().to_string(), false))).collect();
                self.enter(Mode::ExportLibrary(Whole { name: "My library".into(), setlists }));
            }
            (Req::Setlists, Err(_)) => self.say("The engine is busy; try again in a moment."),
            (Req::Contents, result) => self.contents_reply(result),
            (Req::Held, result) => self.held_reply(result),
        }
    }

    fn menu_ready(&mut self) {
        let Some(w) = &self.waiting_menu else { return };
        let (Some(folders), Some(plugins)) = (w.folders.clone(), w.plugins.clone()) else { return };
        self.waiting_menu = None;
        // The open Performance's CLAP plugins installed some other way, for their other platforms' builds.
        let mut in_performance: Vec<PluginRef> = Vec::new();
        for n in self.graph["nodes"].as_array().into_iter().flatten() {
            let plugin = &n["plugin"];
            let path = plugin["path"].as_str().unwrap_or_default();
            let (id, version) = (plugin["id"].as_str().unwrap_or_default(), plugin["version"].as_str().unwrap_or_default());
            if path.ends_with(".clap") && !plugins.iter().any(|p| p.r.id == id && p.r.version == version) && !in_performance.iter().any(|p| p.id == id) {
                in_performance.push(PluginRef { format: "clap".into(), id: id.into(), version: version.into(), name: plugin["name"].as_str().unwrap_or_default().into() });
            }
        }
        let snapshot = Snapshot { folders, hidden: self.hidden.clone(), plugins, in_performance, installed: self.libraries.clone() };
        let mut items = vec![menu::Item::choice("Export library...", EXPORT_LIBRARY)];
        let mut folder_items: Vec<menu::Item> = snapshot.folders.iter().enumerate().map(|(i, f)| menu::Item::choice(format!("Stop indexing {f}"), FOLDER + i)).collect();
        folder_items.push(menu::Item::choice("Index a folder in place...", ADD_FOLDER));
        items.push(menu::Item::sub("Folders indexed in place", folder_items));
        items.push(menu::Item::sub("Show a hidden tag", snapshot.hidden.iter().enumerate().map(|(i, t)| menu::Item::choice(t.as_str(), TAG + i)).collect()));
        // Plugins installed with archives (RFC 0006): what uses each, another platform's build, removal.
        let build = |i: usize| menu::Item::choice("Add a build for another platform...", BUILD + i * 2);
        let mut plugin_items: Vec<menu::Item> = Vec::new();
        for (i, p) in snapshot.plugins.iter().enumerate() {
            let used = if p.used_by.is_empty() { "Nothing in the library uses it".to_string() } else { format!("Used by {}", p.used_by.join(", ")) };
            let builds = format!("Builds: {}  ({})", p.platforms.join(", "), files::size_text(p.bytes));
            plugin_items.push(menu::Item::sub(p.r.title(), vec![menu::Item::choice(used, 0).off(), menu::Item::choice(builds, 0).off(), build(i), menu::Item::choice("Remove from this device...", BUILD + i * 2 + 1)]));
        }
        if !snapshot.in_performance.is_empty() {
            plugin_items.push(menu::Item::choice("In this Performance", 0).off());
            for (k, p) in snapshot.in_performance.iter().enumerate() {
                plugin_items.push(menu::Item::sub(p.title(), vec![build(snapshot.plugins.len() + k)]));
            }
        }
        items.push(menu::Item::sub("Plugins", plugin_items));
        for (i, l) in snapshot.installed.iter().enumerate() {
            items.push(menu::Item::choice(format!("Uninstall {} ({} item{})", l.name, l.items, if l.items == 1 { "" } else { "s" }), i + 2));
        }
        self.menu = Some(("Libraries".into(), items));
        self.menu_for = Some(MenuFor::Library(Menus::Libraries(Box::new(snapshot))));
    }

    /// A choice in a menu made here.
    pub(super) fn library_chosen(&mut self, what: Menus, choice: usize) -> Outcome {
        match what {
            Menus::Install => {
                match choice {
                    1 => self.catalog_for_kind(),
                    _ => self.files_open(Want::File, "olib", "", After::Install, "Install a library, an exported Performance or Sound, or a plugin package", false),
                }
                Outcome::Stay
            }
            Menus::Signature(keys) => self.signature_chosen(keys, choice),
            Menus::Platform(plugin, path) => {
                let Some(platform) = PLATFORMS.iter().filter(|p| **p != platform()).nth(choice - 1) else { return Outcome::Stay };
                let params = json!({"format": plugin.format, "id": plugin.id, "version": plugin.version, "platform": platform, "path": path.display().to_string()});
                self.start_job(JobKind::AddBuild((*platform).to_string()), "plugins.addBuild", params, &format!("Adding the {platform} build..."));
                Outcome::Stay
            }
            Menus::Libraries(snap) => {
                let snap = *snap;
                if choice == EXPORT_LIBRARY {
                    self.export_library_start();
                } else if choice == ADD_FOLDER {
                    self.files_open(Want::Folder, "", "", After::Folder, "Index a folder in place", true);
                } else if (FOLDER..FOLDER + snap.folders.len()).contains(&choice) {
                    let folder = snap.folders[choice - FOLDER].clone();
                    self.start_job(JobKind::RemoveFolder, "library.removeFolder", json!({"folder": folder}), "Taking the folder out...");
                } else if choice >= BUILD {
                    let n = (choice - BUILD) / 2;
                    let plugin = snap.plugins.get(n).map(|p| p.r.clone()).or_else(|| snap.in_performance.get(n - snap.plugins.len()).cloned());
                    let Some(plugin) = plugin else { return Outcome::Stay };
                    if (choice - BUILD).is_multiple_of(2) {
                        self.files_open(Want::Build, "clap", "", After::Build(Box::new(plugin)), "A build of the plugin for another platform", false);
                    } else {
                        let used = snap.plugins.get(n).map_or(0, |p| p.used_by.len());
                        let message = if used == 0 {
                            "Nothing in the library uses it. Every platform's build is removed.".to_string()
                        } else {
                            format!("{used} {} it. {} until it is installed again.", if used == 1 { "item uses" } else { "items use" }, if used == 1 { "It keeps its place and saved state and plays silence" } else { "They keep its place and saved state and play silence" })
                        };
                        self.notice = Some((format!("Remove {}?", plugin.title()), message, vec!["Remove".into(), "Cancel".into()]));
                        self.notice_for = Some(NoticeFor::RemovePlugin(Box::new(plugin)));
                    }
                } else if (TAG..TAG + snap.hidden.len()).contains(&choice) {
                    // A hidden tag shown again, its color kept.
                    let tag = snap.hidden[choice - TAG].clone();
                    let color = self.tag_colors.iter().find(|(t, _)| *t == tag).map(|(_, c)| c.clone()).unwrap_or_default();
                    self.push(Request::Do(Do::Tag), "library.tag", json!({"tag": tag, "hidden": false, "color": color}));
                } else if let Some(l) = snap.installed.get(choice.wrapping_sub(2)) {
                    self.notice = Some((format!("Uninstall {}?", l.name), "Its Sounds and Performances are removed. Your Performances keep their parts and samples, and your own copies stay.".into(), vec!["Uninstall".into(), "Cancel".into()]));
                    self.notice_for = Some(NoticeFor::Uninstall(l.id.clone(), l.name.clone()));
                }
                Outcome::Stay
            }
        }
    }

    /// The answer to a question made here.
    pub(super) fn library_noticed(&mut self, what: NoticeFor, choice: Option<usize>) {
        match what {
            NoticeFor::Overwrite(path, after) => self.overwrite_answer(path, after, choice),
            NoticeFor::RemovePlugin(plugin) if choice == Some(0) => {
                let params = json!({"format": plugin.format, "id": plugin.id, "version": plugin.version, "platform": ""});
                self.start_job(JobKind::RemovePlugin(plugin.title()), "plugins.remove", params, "Removing...");
            }
            NoticeFor::Uninstall(id, name) if choice == Some(0) => self.start_job(JobKind::Uninstall(name), "library.uninstall", json!({"library": id}), "Uninstalling..."),
            _ => {}
        }
    }

    /// The text for a field made here.
    pub(super) fn text_entered(&mut self, what: Asking, text: &str) {
        match what {
            Asking::KeyName => self.key_named(text),
            Asking::FileName => {
                if let Mode::Files(f) = &mut self.mode {
                    f.name = text.trim().to_string();
                }
            }
            Asking::LibraryName => {
                if let Mode::ExportLibrary(w) = &mut self.mode {
                    w.name = text.trim().to_string();
                }
            }
            _ => {}
        }
    }

    /// The Performance's graph, for the plugins it uses.
    pub fn graph(&mut self, graph: &Value) {
        self.graph = graph.clone();
    }
}
