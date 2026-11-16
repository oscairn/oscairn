// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Library app (RFC 0021; hosts/standalone/LibraryPane.cpp and Catalog.cpp are the reference, ui/LIBRARY-APP.md
//! the checklist): the saved Performances, Sounds and setlists, found by typing, by where they are from (the user's
//! own or an installed library), by tags and by favorites, and what is done with them: a Performance is opened, a
//! Sound made a new part, either tried in place, and each item edited, colored, duplicated or deleted.
//!
//! On the shared list screen (`browser.rs`, the Part screen's component): the kinds are the tabs above it (the
//! shared tab family, `slint/library.slint`), the rows are a Filters row, which opens the sound chooser's Favorites,
//! Recent, Library and Tags lists (`filters.rs`), then the items, and the panel beside them says what the item under
//! the cursor is. Search, Audition and Options are the top bar's actions. Without touch: the dial and the left knob
//! walk the tabs then the rows, a press on a Performance opens it, a press on anything else lists what can be done
//! with it, the right knob scrolls the panel, Cancel goes back one level.
//!
//! Nothing here waits on the engine: a request goes out and its answer comes back as `reply`. A Performance takes
//! seconds to open (`open.rs`), and the engine answers anything else as busy meanwhile, so each step of the
//! opening is asked again until it is not refused, and the screen shows what is loading.

mod actions;
mod catalog;
mod catalog_flow;
mod export;
mod files;
mod filters;
mod import;
mod install;
mod item;
mod jobs;
mod libraries;
mod open;
mod setlist;

pub use filters::Installed;
pub use item::{count, empty_saying, library_line, wrap, Item, Kind, INFO_WIDTH};
#[cfg(test)]
pub use libraries::platform;
#[cfg(test)]
pub use open::Step;

use crate::browser::{Browser, Nav};
use crate::client::{Error, BUSY};
use crate::menu;
use crate::nav::Action;
use crate::setlist::color;
use crate::tabs;
use crate::topbar::Tool;
use crate::{InfoLine, LibraryState, SettingData, Shell, TabLabel};
use filters::Filters;
use serde_json::{json, Value};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    /// library.query: the generation it was asked in, so a late answer to an older query is let go.
    Query(u64),
    /// library.libraries: the installed libraries, for the Library filter.
    Libraries,
    /// packages.installed: the plugin packages installed, which the empty list and Home's line count.
    Plugins,
    /// packages.preinstalled: the packages that come with the release.
    Setup,
    /// A step of opening a Performance.
    Open(open::Step),
    /// A step of adding a Performance to a setlist.
    Setlist(setlist::Step),
    /// What installing, exporting and managing ask (libraries.rs).
    Lib(libraries::Req),
    /// A long request: an install, an export, a build copied in (jobs.rs).
    Job(jobs::Job),
    /// The Catalog's reads and installs (catalog_flow.rs).
    Pkg(catalog_flow::Req),
    /// audition.start, audition.stop, audition.get.
    Audition(open::Aud),
    /// An edit of an item or a tag: the answer says what happened.
    Do(Do),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Do {
    Favorite,
    /// parts.fromSound, with the Sound's name.
    NewPart(String),
    /// library.edit, with the name it was saved under.
    Edit(String),
    /// library.copy: the name and whether it was the user's own.
    Copy(String, bool),
    /// library.delete, with the name.
    Delete(String),
    Tag,
}

/// What the screen asks the engine: the request, and when to ask it (a step refused as busy is asked again).
pub struct Send {
    pub request: Request,
    pub method: &'static str,
    pub params: Value,
    pub after: Option<Duration>,
}

pub type Out = Send;

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
    /// Text asked for on the keyboard: its title and what it starts with; `entered` takes it.
    Prompt(String, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Filters,
    Item(usize),
    /// The empty list's button to the Catalog.
    Browse,
    /// Row `n` of the page the mode shows (the file chooser, the install and export choices).
    Page(usize),
    Favorites,
    Recent,
    Library,
    Tags,
    Source(usize),
    Tag(usize),
    Nothing,
    EditName,
    EditDescription,
    EditTags,
    EditSave,
    EditCancel,
}

/// An item's name, description and tags being edited (the JUCE editor's three fields).
#[derive(Clone, Debug, PartialEq)]
struct Draft {
    id: String,
    name: String,
    description: String,
    tags: String,
    color: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    List,
    /// Favorites, Recent, the library and the tags.
    Filters,
    Sources,
    Tags,
    Edit(Draft),
    /// The file chooser (files.rs).
    Files(files::Files),
    /// An archive's contents and what to do with it (install.rs).
    Install(Box<install::Install>),
    /// An item's export (export.rs).
    Export(export::Export),
    /// The library's export.
    ExportLibrary(export::Whole),
    /// Another Performance's parts and scenes to bring in (import.rs).
    Import(import::Import),
    /// The packages the repositories offer (catalog_flow.rs).
    Catalog(Box<catalog_flow::Catalog>),
    /// One package of the Catalog.
    Package(Box<catalog::Offer>),
}

/// What the keyboard is open for.
#[derive(Clone, Debug, PartialEq)]
enum Asking {
    Search,
    EditName,
    EditDescription,
    EditTags,
    /// The name a Performance never saved is saved under before another is opened.
    SaveName,
    /// A file's name, in the file chooser.
    FileName,
    /// A new signing key's name.
    KeyName,
    /// The exported library's name.
    LibraryName,
    /// What the Catalog is searched for.
    CatalogSearch,
}

/// What a menu the screen asked for is about.
#[derive(Clone, Debug, PartialEq)]
enum MenuFor {
    Item(String),
    Tag(String),
    /// A menu of libraries.rs.
    Library(libraries::Menus),
}

/// What a question the screen asked for is about.
#[derive(Clone, Debug, PartialEq)]
enum NoticeFor {
    SaveFirst,
    Delete(String),
    /// A problem, with one answer.
    Alert,
    /// A file that exists is to be written over: the path and what it is for.
    Overwrite(std::path::PathBuf, files::After),
    RemovePlugin(Box<libraries::PluginRef>),
    /// A library to uninstall: its ID and name.
    Uninstall(String, String),
    /// A different build installed: the repository's is offered over it.
    Replace(Box<catalog::Offer>),
    /// Code this device does not trust: the install and the question.
    Code(catalog_flow::Ctx, Box<catalog::CodeQuestion>),
}

/// What the top bar's actions are, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Search,
    Audition,
    Keep,
    Options,
    Install,
    Libraries,
}

/// The Performance or Sound being tried, as `audition.get` says.
#[derive(Clone, Debug, Default, PartialEq)]
struct Auditioning {
    id: String,
    kind: String,
    part: u32,
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    b: Browser,
    kind: Kind,
    filters: Filters,
    mode: Mode,
    /// The query asked last.
    generation: u64,
    items: Vec<Item>,
    total: u64,
    /// The tags of the matches with their counts, as the engine gave them.
    tags: Vec<(String, u64)>,
    /// The colors the user gave tags (`tagColors`), and the tags they hid (`hiddenTags`).
    tag_colors: Vec<(String, String)>,
    hidden: Vec<String>,
    libraries: Vec<Installed>,
    /// The plugin packages installed.
    plugins: usize,
    /// The packages that come with the release are installing: the line that says so, or its error.
    setup: String,
    /// The tab the cursor is on, when it is on the tabs.
    tab_at: Option<usize>,
    /// The item last pressed or under the cursor: what Options is about.
    selected: Option<String>,
    auditioning: Auditioning,
    /// The part a Sound is tried in: the one the Part screen shows, 0 for none.
    pub part: u32,
    /// The Performance being opened, and the one chosen meanwhile (opened next, the last chosen).
    job: Option<open::Open>,
    waiting: Option<open::Open>,
    message: String,
    asking: Option<Asking>,
    menu: Option<(String, Vec<menu::Item>)>,
    menu_for: Option<MenuFor>,
    notice: Option<(String, String, Vec<String>)>,
    notice_for: Option<NoticeFor>,
    /// Whether this screen is the one shown: the list and the tabs are written only then.
    active: bool,
    /// The tabs as last written: whether shown, which, the cursor.
    tabs_shown: Option<(u8, usize, Option<usize>)>,
    /// The Setlist app's state file, which says which setlist is open (setlist.rs).
    state_file: Option<std::path::PathBuf>,
    /// The modes this one was entered from (libraries.rs `enter`, `leave`).
    stack: Vec<Mode>,
    /// The folder the file chooser last chose in.
    files_dir: Option<std::path::PathBuf>,
    /// The archive whose contents were asked for.
    install_file: Option<std::path::PathBuf>,
    /// The key being made.
    made_key: Option<String>,
    /// What the Libraries menu waits for.
    waiting_menu: Option<libraries::Waiting>,
    /// The Performance's graph, for the plugins it uses.
    graph: Value,
    /// The Catalog being read: which, and the plugins the scanner found that come with the app.
    catalog_generation: u64,
    scanned: bool,
    bundled: std::collections::BTreeMap<String, String>,
    want_scan: bool,
    /// Keys still being trusted before an install is tried again.
    trusting_left: usize,
    /// The Performance whose parts and scenes are being read: its name and folder.
    importing: Option<(String, String)>,
    pub out: Vec<Out>,
}

impl Screen {
    pub fn new(ui: &Shell) -> Screen {
        Screen {
            ui: ui.as_weak(),
            b: Browser::new(ui),
            kind: Kind::Performance,
            filters: Filters::default(),
            mode: Mode::List,
            generation: 0,
            items: vec![],
            total: 0,
            tags: vec![],
            tag_colors: vec![],
            hidden: vec![],
            libraries: vec![],
            plugins: 0,
            setup: String::new(),
            tab_at: None,
            selected: None,
            auditioning: Auditioning::default(),
            part: 0,
            job: None,
            waiting: None,
            message: String::new(),
            asking: None,
            menu: None,
            menu_for: None,
            notice: None,
            notice_for: None,
            active: false,
            tabs_shown: None,
            state_file: None,
            stack: vec![],
            files_dir: None,
            install_file: None,
            made_key: None,
            waiting_menu: None,
            graph: Value::Null,
            catalog_generation: 0,
            scanned: false,
            bundled: Default::default(),
            want_scan: false,
            trusting_left: 0,
            importing: None,
            out: vec![],
        }
    }

    fn push(&mut self, request: Request, method: &'static str, params: Value) {
        self.out.push(Send { request, method, params, after: None });
    }

    fn push_after(&mut self, after: Duration, request: Request, method: &'static str, params: Value) {
        self.out.push(Send { request, method, params, after: Some(after) });
    }

    /// Asks for the list again, narrowed as it is: an older answer still out is let go.
    fn query(&mut self) {
        self.generation += 1;
        let params = self.filters.params(self.kind);
        self.push(Request::Query(self.generation), "library.query", params);
    }

    /// The screen opens: what it lists, which libraries are installed, what is installing.
    pub fn open(&mut self) {
        self.mode = Mode::List;
        self.stack.clear();
        self.tab_at = None;
        self.b.reset();
        self.query();
        self.push(Request::Libraries, "library.libraries", json!({}));
        self.push(Request::Plugins, "packages.installed", json!({}));
        self.push(Request::Setup, "packages.preinstalled", json!({}));
    }

    /// How many plugin packages are installed (`packages.installed`), as Home's line counts them.
    pub fn plugins(&self) -> usize {
        self.plugins
    }

    /// The kind shown, and the screen's own state when the engine was asked something that no longer matters.
    pub fn show_page(&mut self, page: usize) {
        if matches!(self.mode, Mode::Catalog(_)) {
            self.tab_at = None;
            self.catalog_tab(page);
            return self.show();
        }
        let Some(kind) = Kind::ALL.get(page).copied() else { return };
        self.tab_at = None;
        if kind != self.kind {
            // The tags chosen for another kind are cleared (LibraryPane.cpp `showKind`).
            self.filters.chosen.clear();
            self.kind = kind;
            self.b.reset_cursor();
            self.query();
        }
        self.show();
    }

    pub fn at_top(&self) -> bool {
        if self.tabs_visible() {
            self.tab_at == Some(0)
        } else {
            self.b.at_top()
        }
    }

    fn tabs_visible(&self) -> bool {
        matches!(self.mode, Mode::List | Mode::Catalog(_))
    }

    /// The tabs of the mode shown: the kinds, or the Catalog's four; and the one chosen.
    fn tab_names(&self) -> Vec<String> {
        match &self.mode {
            Mode::List => Kind::ALL.iter().map(|k| k.plural().to_string()).collect(),
            Mode::Catalog(_) => catalog_flow::TABS.iter().map(|t| t.1.to_string()).collect(),
            _ => vec![],
        }
    }

    fn tab_current(&self) -> usize {
        match &self.mode {
            Mode::Catalog(c) => catalog_flow::TABS.iter().position(|t| t.0 == c.kind).unwrap_or(0),
            _ => Kind::ALL.iter().position(|k| *k == self.kind).unwrap_or(0),
        }
    }

    /// The bar's title: the Library, the kind, and the list or the item open in it.
    pub fn path(&self) -> Vec<String> {
        let mut path = vec!["Library".to_string()];
        match &self.mode {
            Mode::List => path.push(self.kind.plural().to_string()),
            Mode::Filters => path.extend([self.kind.plural().to_string(), "Filters".into()]),
            Mode::Sources => path.extend([self.kind.plural().to_string(), "Filters".into(), "Library".into()]),
            Mode::Tags => path.extend([self.kind.plural().to_string(), "Filters".into(), "Tags".into()]),
            Mode::Edit(d) => path.extend([self.kind.plural().to_string(), format!("Edit {}", if d.name.is_empty() { "Untitled" } else { &d.name })]),
            Mode::Files(f) => path.extend([self.kind.plural().to_string(), f.title.clone()]),
            Mode::Install(i) => path.extend([self.kind.plural().to_string(), format!("Install {}", if i.name.is_empty() { "an archive" } else { &i.name })]),
            Mode::Export(e) => path.extend([self.kind.plural().to_string(), format!("Export {}", if e.name.is_empty() { "Untitled" } else { &e.name })]),
            Mode::ExportLibrary(_) => path.extend([self.kind.plural().to_string(), "Export library".into()]),
            Mode::Import(i) => path.extend([self.kind.plural().to_string(), format!("Import from {}", if i.name.is_empty() { "Untitled" } else { &i.name })]),
            Mode::Catalog(_) => path.push("Catalog".into()),
            Mode::Package(o) => path.extend(["Catalog".to_string(), o.title().to_string()]),
        }
        path
    }

    pub fn hidden(&mut self) {
        self.active = false;
        self.b.hidden();
        self.tab_at = None;
        self.mode = Mode::List;
        self.menu = None;
        self.menu_for = None;
        self.asking = None;
        self.stack.clear();
        self.waiting_menu = None;
    }

    pub fn shown(&mut self) {
        self.active = true;
        self.b.install();
        self.show();
    }

    fn say(&mut self, text: impl Into<String>) {
        self.message = text.into();
        if self.active {
            self.b.say(&self.message);
        }
    }

    // ---- what is shown ----

    /// An item's detail as the reference's row writes it: its tags and description, behind what is said first.
    fn detail(&self, item: &Item) -> String {
        let sep = "  ·  ";
        let mut detail = item.tags.join(sep);
        if !item.description.is_empty() {
            detail += &format!("{}{}", if item.tags.is_empty() { "" } else { "   " }, item.description);
        }
        let first = |what: &str, detail: String| if detail.is_empty() { what.to_string() } else { format!("{what}{sep}{detail}") };
        if !item.library.is_empty() {
            detail = first("Locked", detail);
        }
        if !item.unfit.is_empty() {
            detail = first("Does not fit this device", detail);
        }
        if self.job.as_ref().is_some_and(|j| j.id == item.id) {
            detail = first("Opening", detail);
        }
        if item.id == self.auditioning.id && !item.id.is_empty() {
            detail = first("Auditioning", detail);
        }
        if item.favorite {
            detail = if detail.is_empty() { "★".into() } else { format!("{detail}  ★") };
        }
        detail
    }

    fn tag_color(&self, tag: &str) -> Option<slint::Color> {
        self.tag_colors.iter().find(|(t, _)| t == tag).and_then(|(_, c)| color(c))
    }

    fn rows(&self) -> Vec<(Row, SettingData)> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let toggle = |label: &str, on: bool| SettingData { label: label.into(), value: if on { "On" } else { "Off" }.into(), kind: 1, on, ..Default::default() };
        match &self.mode {
            Mode::List => {
                let mut rows = vec![(Row::Filters, row("Filters".into(), format!("{} ›", self.filters.summary(&self.libraries)), 2))];
                rows.extend(self.items.iter().enumerate().map(|(i, item)| {
                    let tint = color(&item.color);
                    (Row::Item(i), SettingData { tint: tint.unwrap_or_default(), tinted: tint.is_some(), ..row(item.title().into(), self.detail(item), 2) })
                }));
                // Nothing of the kind and nothing narrowing it: a way to the Catalog (LibraryPane.cpp: "Browse the Catalog").
                if self.total == 0 && !self.filters.narrowed() && self.kind != Kind::Setlist {
                    rows.push((Row::Browse, row("Browse the Catalog".into(), String::new(), 2)));
                }
                rows
            }
            Mode::Filters => vec![
                (Row::Favorites, toggle("Favorites", self.filters.favorites)),
                (Row::Recent, toggle("Recent", self.filters.recent)),
                (Row::Library, row("Library".into(), format!("{} ›", self.filters.source_name(&self.filters.source, &self.libraries)), 2)),
                (Row::Tags, row("Tags".into(), format!("{} ›", if self.filters.chosen.is_empty() { "none".to_string() } else { self.filters.chosen.join(", ") }), 2)),
            ],
            Mode::Sources => self
                .filters
                .sources(&self.libraries)
                .into_iter()
                .enumerate()
                .map(|(i, (id, name, items))| {
                    let on = id == self.filters.source;
                    let count = items.map(|n| n.to_string()).unwrap_or_default();
                    (Row::Source(i), SettingData { value: if on { "Chosen".into() } else { count.into() }, on, ..toggle(&name, on) })
                })
                .collect(),
            Mode::Tags => {
                let tags = self.filters.tag_list(&self.tags);
                if tags.is_empty() {
                    return vec![(Row::Nothing, row("No tags".into(), String::new(), 4))];
                }
                tags.into_iter()
                    .enumerate()
                    .map(|(i, (tag, count))| {
                        let on = self.filters.chosen.contains(&tag);
                        let tint = self.tag_color(&tag);
                        (Row::Tag(i), SettingData { value: if on { format!("✓ {count}") } else { count.to_string() }.into(), on, tint: tint.unwrap_or_default(), tinted: tint.is_some(), ..toggle(&tag, on) })
                    })
                    .collect()
            }
            Mode::Import(i) => i.rows().into_iter().enumerate().map(|(k, r)| (Row::Page(k), r)).collect(),
            Mode::Catalog(c) => c.rows().into_iter().enumerate().map(|(i, r)| (Row::Page(i), r)).collect(),
            Mode::Package(o) => Screen::package_rows(o).into_iter().enumerate().map(|(i, r)| (Row::Page(i), r.0)).collect(),
            Mode::Files(f) => f.rows().into_iter().enumerate().map(|(i, r)| (Row::Page(i), r)).collect(),
            Mode::Install(i) => i.rows().into_iter().enumerate().map(|(k, r)| (Row::Page(k), r)).collect(),
            Mode::Export(e) => e.rows().into_iter().enumerate().map(|(k, r)| (Row::Page(k), r)).collect(),
            Mode::ExportLibrary(w) => w.rows().into_iter().enumerate().map(|(k, r)| (Row::Page(k), r)).collect(),
            Mode::Edit(d) => {
                let shown = |s: &str| if s.is_empty() { "none ›".to_string() } else { format!("{s} ›") };
                vec![
                    (Row::EditName, row("Name".into(), shown(&d.name), 2)),
                    (Row::EditDescription, row("Description".into(), shown(&d.description), 2)),
                    (Row::EditTags, row("Tags".into(), shown(&d.tags), 2)),
                    (Row::EditSave, row("Save".into(), String::new(), 2)),
                    (Row::EditCancel, row("Cancel".into(), String::new(), 2)),
                ]
            }
        }
    }

    /// The item the cursor is on.
    fn cursor_item(&self) -> Option<usize> {
        let at = self.b.cursor.at?;
        match self.rows().get(at)?.0 {
            Row::Item(i) => Some(i),
            _ => None,
        }
    }

    fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        match &self.mode {
            Mode::Import(i) => return i.info(),
            Mode::Catalog(c) => return c.info(),
            Mode::Package(o) => return self.package_info(o),
            Mode::Files(f) => return f.info(),
            Mode::Install(i) => return i.info(),
            Mode::Export(e) => return e.info(),
            Mode::ExportLibrary(w) => return w.info(),
            _ => {}
        }
        let mut lines = Vec::new();
        if let (Mode::List, Some(item)) = (&self.mode, self.cursor_item().and_then(|i| self.items.get(i))) {
            lines.push(head(self.kind.singular()));
            lines.push(line(item.title().into()));
            if !item.tags.is_empty() {
                lines.push(line(item.tags.join(", ")));
            }
            lines.extend(wrap(&item.description, INFO_WIDTH).into_iter().map(line));
            let from = if item.own() { "My own".to_string() } else { format!("Locked: {}", self.filters.source_name(&item.library, &self.libraries)) };
            lines.push(line(from));
            if !item.unfit.is_empty() {
                lines.push(head("Does not fit this device"));
                lines.extend(wrap(&item.unfit, INFO_WIDTH).into_iter().map(line));
            }
            if self.job.as_ref().is_some_and(|j| j.id == item.id) {
                lines.push(head("Opening"));
            }
            if item.id == self.auditioning.id {
                lines.push(head("Auditioning"));
            }
        }
        lines.push(head("Library"));
        match &self.mode {
            Mode::List => {
                lines.push(line(format!("{} of {}", self.items.len(), count(self.total, self.kind))));
                if self.total == 0 && !self.filters.narrowed() && !self.setup.is_empty() {
                    lines.extend(wrap(&self.setup, INFO_WIDTH).into_iter().map(line));
                } else if self.total == 0 && !self.filters.narrowed() {
                    lines.extend(wrap(&empty_saying(self.kind, self.plugins), INFO_WIDTH).into_iter().map(line));
                } else if self.total == 0 {
                    lines.push(line("Nothing matches".into()));
                }
            }
            Mode::Tags => lines.push(line(format!("{} tags", self.filters.tag_list(&self.tags).len()))),
            _ => lines.push(line(self.kind.plural().into())),
        }
        if !self.setup.is_empty() && self.total != 0 {
            lines.extend(wrap(&self.setup, INFO_WIDTH).into_iter().map(line));
        }
        lines
    }

    fn show_tabs(&mut self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let set = match &self.mode {
            Mode::List => 1,
            Mode::Catalog(_) => 2,
            _ => 0,
        };
        let state = (set, self.tab_current(), self.tab_at.filter(|_| self.tabs_visible()));
        if self.tabs_shown == Some(state) {
            return;
        }
        let was = self.tabs_shown.replace(state);
        let lib = ui.global::<LibraryState>();
        if was.map(|w| w.0) != Some(state.0) {
            let names: Vec<String> = self.tab_names();
            let tabs::Abbreviated { heading, shorts } = tabs::abbreviate(&names);
            let labels: Vec<TabLabel> = shorts
                .into_iter()
                .map(|short| {
                    let (top, bottom) = tabs::two_lines(&short);
                    TabLabel { short: short.into(), top: top.into(), bottom: bottom.into() }
                })
                .collect();
            lib.set_pages(ModelRc::new(VecModel::from(names.iter().map(|n| n.as_str().into()).collect::<Vec<slint::SharedString>>())));
            lib.set_page_labels(ModelRc::new(VecModel::from(labels)));
            lib.set_page_heading(heading.into());
        }
        lib.set_page(state.1 as i32);
        lib.set_page_cursor(state.2.map_or(-1, |c| c as i32));
    }

    /// The list, the tabs and the panel, each row set only when it changed.
    pub fn show(&mut self) {
        self.show_tabs();
        if !self.active {
            return;
        }
        if self.tab_at.is_some() {
            self.b.cursor = self.b.cursor.clear();
        }
        let rows: Vec<SettingData> = self.rows().into_iter().map(|r| r.1).collect();
        let noun = match self.mode {
            Mode::List => self.kind.plural(),
            Mode::Filters => "Filters",
            Mode::Sources => "Libraries",
            Mode::Tags => "Tags",
            Mode::Edit(_) => "Fields",
            Mode::Files(_) => "Files",
            Mode::Import(_) => "Choices",
            Mode::Catalog(_) => "Packages",
            Mode::Package(_) => "Details",
            Mode::Install(_) | Mode::Export(_) | Mode::ExportLibrary(_) => "Choices",
        };
        let n = rows.len();
        let info = self.info();
        let title = self.path().last().cloned().unwrap_or_default();
        let items_view = self.mode == Mode::List;
        self.b.show("", &title, noun, rows, info);
        // The footer counts the items, not the Filters row (LibraryPane.cpp: "N Performances").
        if items_view {
            if let Some(ui) = self.ui.upgrade() {
                let per = ui.global::<crate::PartState>().get_rows_per_view().max(1) as usize;
                // Row k is item k: the Filters row is row 0.
                let (from, to) = (self.b.cursor.first.max(1), n.min(self.b.cursor.first + per).saturating_sub(1));
                let text = if to >= from { format!("{} {from}–{to} of {}", self.kind.plural(), self.items.len()) } else { String::new() };
                let text = if self.total > self.items.len() as u64 { format!("{text}, {} in all", self.total) } else { text };
                ui.global::<crate::PartState>().set_view(text.into());
            }
        }
        let message = self.message.clone();
        self.b.say(&message);
    }

    // ---- the top bar ----

    fn acts(&self) -> Vec<Act> {
        let mut acts = vec![Act::Search, Act::Audition];
        if !self.auditioning.id.is_empty() {
            acts.push(Act::Keep);
        }
        acts.extend([Act::Options, Act::Install, Act::Libraries]);
        acts
    }

    /// The screen's actions on the top bar.
    pub fn bar_tools(&self) -> Vec<Tool> {
        if let Mode::Catalog(c) = &self.mode {
            return vec![Tool::new(if c.search.is_empty() { "Search".to_string() } else { format!("Search: {}", c.search) })];
        }
        if self.mode != Mode::List {
            return if self.mode == Mode::Tags { vec![Tool { label: "Options".into(), enabled: self.tag_under_cursor().is_some(), on: false }] } else { vec![] };
        }
        let target = self.target().is_some();
        self.acts()
            .into_iter()
            .map(|a| match a {
                Act::Search => Tool::new(if self.filters.search.is_empty() { "Search".to_string() } else { format!("Search: {}", self.filters.search) }),
                Act::Audition => Tool { label: if self.auditioning.id.is_empty() { "Audition" } else { "Stop" }.into(), enabled: !self.auditioning.id.is_empty() || self.can_audition(), on: !self.auditioning.id.is_empty() },
                Act::Keep => Tool::new("Keep"),
                Act::Options => Tool { label: "Options".into(), enabled: target, on: false },
                Act::Install => Tool::new("Install..."),
                Act::Libraries => Tool::new("Libraries..."),
            })
            .collect()
    }

    /// Does bar action `i`.
    pub fn tool(&mut self, i: usize) -> Outcome {
        if let Mode::Catalog(c) = &self.mode {
            self.asking = Some(Asking::CatalogSearch);
            return Outcome::Prompt("Search the Catalog by name, tag or publisher".into(), c.search.clone());
        }
        if self.mode == Mode::Tags {
            if let Some(tag) = self.tag_under_cursor() {
                self.tag_menu(&tag);
            }
            return Outcome::Stay;
        }
        let Some(act) = self.acts().get(i).copied() else { return Outcome::Stay };
        match act {
            Act::Search => {
                self.asking = Some(Asking::Search);
                return Outcome::Prompt(format!("Search the {}", self.kind.plural()), self.filters.search.clone());
            }
            Act::Audition => self.audition_toggle(),
            Act::Keep => self.stop_audition(true),
            Act::Options => {
                if let Some(id) = self.target() {
                    self.item_menu(&id);
                }
            }
            Act::Install => self.install_menu(),
            Act::Libraries => self.libraries_menu_start(),
        }
        self.show();
        Outcome::Stay
    }

    /// The item Options is about: the one under the cursor, else the one last pressed (a tap leaves no cursor).
    fn target(&self) -> Option<String> {
        if self.mode != Mode::List {
            return None;
        }
        self.cursor_item().and_then(|i| self.items.get(i)).map(|i| i.id.clone()).or_else(|| self.selected.clone().filter(|s| self.items.iter().any(|i| i.id == *s)))
    }

    fn tag_under_cursor(&self) -> Option<String> {
        if self.mode != Mode::Tags {
            return None;
        }
        let at = self.b.cursor.at?;
        match self.rows().get(at)?.0 {
            Row::Tag(i) => self.filters.tag_list(&self.tags).get(i).map(|t| t.0.clone()),
            _ => None,
        }
    }

    // ---- what the engine answers ----

    /// An answer. True when the Performance changed under the screen (the shell reads it again).
    pub fn reply(&mut self, request: Request, result: Result<Value, Error>) -> bool {
        let changed = match request {
            Request::Query(generation) => {
                match result {
                    Ok(r) if generation == self.generation => self.listed(&r),
                    Err(e) if e.code == BUSY && generation == self.generation => {
                        // The engine is busy with a load: the list is asked again in a moment (the JUCE pane's timer).
                        self.generation += 1;
                        let params = self.filters.params(self.kind);
                        self.push_after(Duration::from_millis(500), Request::Query(self.generation), "library.query", params);
                    }
                    Err(e) if e.code != 0 && generation == self.generation => self.say(format!("The engine refused: {}", e.message)),
                    _ => {}
                }
                false
            }
            Request::Libraries => {
                if let Ok(r) = result {
                    self.libraries = r["libraries"].as_array().into_iter().flatten().filter_map(|l| Some(Installed { id: l["id"].as_str()?.to_string(), name: l["name"].as_str().unwrap_or_default().to_string(), items: l["items"].as_u64().unwrap_or(0) })).collect();
                    // A library uninstalled while it was chosen: everything shows (LibraryPane.cpp `timerCallback`).
                    let source = self.filters.source.clone();
                    if !source.is_empty() && source != "mine" && !self.libraries.iter().any(|l| l.id == source) {
                        self.filters.source.clear();
                        self.query();
                    }
                }
                false
            }
            Request::Plugins => {
                if let Ok(r) = result {
                    self.plugins = r["packages"].as_array().into_iter().flatten().filter(|p| p["kind"] == "plugin").count();
                }
                false
            }
            Request::Setup => {
                self.setup_read(result);
                false
            }
            Request::Open(step) => self.opened(step, result),
            Request::Setlist(step) => self.setlisted(step, result),
            Request::Lib(r) => {
                self.lib_reply(r, result);
                false
            }
            Request::Job(job) => self.job_reply(job, result),
            Request::Pkg(r) => self.pkg_reply(r, result),
            Request::Audition(what) => self.auditioned(what, result),
            Request::Do(what) => self.done(what, result),
        };
        self.show();
        changed
    }

    /// A list the engine gave.
    fn listed(&mut self, r: &Value) {
        // The cursor stays on the item it was on, by its ID, wherever the new list has it (LibraryPane.cpp `timerCallback`).
        let on = self.cursor_item().and_then(|i| self.items.get(i)).map(|i| i.id.clone());
        self.items = r["items"].as_array().into_iter().flatten().filter_map(Item::from_json).collect();
        self.total = r["total"].as_u64().unwrap_or(self.items.len() as u64);
        self.tags = r["tags"].as_array().into_iter().flatten().filter_map(|t| Some((t["tag"].as_str()?.to_string(), t["count"].as_u64().unwrap_or(0)))).collect();
        self.tag_colors = r["tagColors"].as_array().into_iter().flatten().filter_map(|t| Some((t["tag"].as_str()?.to_string(), t["color"].as_str().unwrap_or_default().to_string()))).collect();
        self.hidden = r["hiddenTags"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
        if let (Some(id), Mode::List) = (on, &self.mode) {
            if let Some(at) = self.items.iter().position(|i| i.id == id) {
                self.b.cursor.at = Some(at + 1); // the Filters row is the first
            }
        }
        // The row last pressed stays what Options is about while it is listed.
        if self.selected.as_ref().is_some_and(|s| !self.items.iter().any(|i| i.id == *s)) {
            self.selected = None;
        }
    }

    /// The line while the packages that come with the release install, from `packages.preinstalled`.
    fn setup_read(&mut self, result: Result<Value, Error>) {
        let Ok(r) = result else { return };
        let running = r["state"] == "running";
        let error = r["error"].as_str().unwrap_or_default();
        let was = !self.setup.is_empty();
        self.setup = if running {
            "Setting up the instruments that come with Oscairn…".into()
        } else {
            error.to_string()
        };
        if running {
            // Asked again each second while it runs, and not otherwise: nothing wakes at rest.
            self.push_after(Duration::from_secs(1), Request::Setup, "packages.preinstalled", json!({}));
        } else if was {
            self.query(); // what it installed, listed
        }
        if running && self.message.is_empty() {
            let s = self.setup.clone();
            self.say(s);
        } else if !running && self.message == "Setting up the instruments that come with Oscairn…" {
            self.say("");
        }
    }

    // ---- the keys, a press and a tap ----

    fn press(&mut self, at: usize) -> Outcome {
        let Some((row, _)) = self.rows().into_iter().nth(at) else { return Outcome::Stay };
        match row {
            Row::Filters => {
                self.mode = Mode::Filters;
                self.b.reset_cursor();
            }
            Row::Favorites => {
                self.filters.favorites = !self.filters.favorites;
                self.query();
            }
            Row::Recent => {
                self.filters.recent = !self.filters.recent;
                self.query();
            }
            Row::Library => {
                self.mode = Mode::Sources;
                self.b.reset_cursor();
                self.push(Request::Libraries, "library.libraries", json!({}));
            }
            Row::Tags => {
                self.mode = Mode::Tags;
                self.b.reset_cursor();
            }
            Row::Source(i) => {
                if let Some((id, _, _)) = self.filters.sources(&self.libraries).get(i) {
                    self.filters.source = id.clone();
                    self.mode = Mode::Filters;
                    self.b.reset_cursor();
                    self.query();
                }
            }
            Row::Tag(i) => {
                if let Some((tag, _)) = self.filters.tag_list(&self.tags).get(i).cloned() {
                    self.filters.toggle_tag(&tag);
                    self.query();
                }
            }
            Row::Item(i) => return self.item_pressed(i),
            Row::Browse => self.catalog_for_kind(),
            Row::Page(i) => return self.page_press(i),
            Row::EditName => return self.ask(Asking::EditName),
            Row::EditDescription => return self.ask(Asking::EditDescription),
            Row::EditTags => return self.ask(Asking::EditTags),
            Row::EditSave => self.edit_save(),
            Row::EditCancel => {
                self.mode = Mode::List;
                self.b.reset_cursor();
            }
            Row::Nothing => {}
        }
        Outcome::Stay
    }

    /// A row of the page a mode shows (files, install, export).
    fn page_press(&mut self, at: usize) -> Outcome {
        match &self.mode {
            Mode::Files(_) => return self.files_press(at),
            Mode::Import(_) => self.import_press(at),
            Mode::Catalog(_) => self.catalog_press(at),
            Mode::Package(_) => self.package_press(at),
            Mode::Install(_) => self.install_press(at),
            Mode::Export(_) => self.export_press(at),
            Mode::ExportLibrary(_) => return self.export_library_press(at),
            _ => {}
        }
        Outcome::Stay
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        let outcome = match nav {
            Nav::Press(at) => self.press(at),
            Nav::Pane(by) => {
                self.b.scroll_info(by.signum());
                Outcome::Stay
            }
            Nav::Back => self.back_one(),
            Nav::Dial(..) | Nav::None => Outcome::Stay,
        };
        self.show();
        outcome
    }

    /// Back from anywhere on the screen: up a level, else out of it.
    pub fn back(&mut self) -> Outcome {
        let outcome = self.back_one();
        self.show();
        outcome
    }

    fn back_one(&mut self) -> Outcome {
        self.tab_at = None;
        match self.mode {
            Mode::List => Outcome::Back,
            Mode::Sources | Mode::Tags => {
                self.mode = Mode::Filters;
                self.b.reset_cursor();
                Outcome::Stay
            }
            Mode::Filters | Mode::Edit(_) => {
                self.mode = Mode::List;
                self.b.reset_cursor();
                Outcome::Stay
            }
            Mode::Files(_) | Mode::Install(_) | Mode::Export(_) | Mode::ExportLibrary(_) | Mode::Import(_) | Mode::Catalog(_) | Mode::Package(_) => {
                self.leave();
                Outcome::Stay
            }
        }
    }

    /// The tabs have the cursor: the dial walks them and leaves for the list past the last; left and right along them.
    fn act_tabs(&mut self, action: Action, at: usize) -> Outcome {
        let last = self.tab_names().len().saturating_sub(1);
        match action {
            Action::Move { pane: None | Some(0), by } => {
                let to = at as i64 + i64::from(by);
                if to > last as i64 {
                    self.tab_at = None;
                    self.b.cursor = self.b.cursor.moved(0);
                } else {
                    self.tab_at = Some(to.max(0) as usize);
                }
            }
            Action::Arrow { dx, dy } => {
                let by = if dx != 0 { dx } else { dy };
                let to = at as i64 + i64::from(by);
                if to > last as i64 && dy > 0 {
                    self.tab_at = None;
                    self.b.cursor = self.b.cursor.moved(0);
                } else {
                    self.tab_at = Some(to.clamp(0, last as i64) as usize);
                }
            }
            Action::End(by) => self.tab_at = Some(if by < 0 { 0 } else { last }),
            Action::Confirm(_) => {
                self.show_page(at);
                self.tab_at = Some(at);
            }
            _ => {}
        }
        self.show();
        Outcome::Stay
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        if let (Some(at), true) = (self.tab_at, self.tabs_visible()) {
            return self.act_tabs(action, at);
        }
        // Moving up out of the first row reaches the tabs.
        let up = matches!(action, Action::Move { pane: None | Some(0), by } if by < 0) || matches!(action, Action::Arrow { dy, .. } if dy < 0);
        if up && self.tabs_visible() && matches!(self.b.cursor.at, Some(0)) {
            self.b.cursor = self.b.cursor.clear();
            self.tab_at = Some(self.tab_current());
            self.show();
            return Outcome::Stay;
        }
        let nav = self.b.nav(action);
        self.handle(nav)
    }

    pub fn tap(&mut self, row: i32) -> Outcome {
        self.tab_at = None;
        let nav = self.b.tap(row);
        self.handle(nav)
    }

    pub fn scroll_info(&mut self, by: i32) {
        self.b.scroll_info(by as i64);
        self.show();
    }

    pub fn step_row(&mut self, _row: i32, _by: i32) {}

    /// A tab tapped: its kind shown.
    pub fn tap_page(&mut self, page: usize) {
        self.show_page(page);
    }

    // ---- text ----

    fn ask(&mut self, what: Asking) -> Outcome {
        let (title, text) = match (&what, &self.mode) {
            (Asking::EditName, Mode::Edit(d)) => ("Name".to_string(), d.name.clone()),
            (Asking::EditDescription, Mode::Edit(d)) => ("Description".to_string(), d.description.clone()),
            (Asking::EditTags, Mode::Edit(d)) => ("Tags, separated by commas".to_string(), d.tags.clone()),
            _ => return Outcome::Stay,
        };
        self.asking = Some(what);
        Outcome::Prompt(title, text)
    }

    /// The keyboard's text, after Done.
    pub fn entered(&mut self, text: &str) {
        match self.asking.take() {
            Some(Asking::Search) => {
                self.filters.search = text.trim().to_string();
                self.b.reset_cursor();
                self.query();
            }
            Some(Asking::EditName) => {
                if let Mode::Edit(d) = &mut self.mode {
                    d.name = text.trim().to_string();
                }
            }
            Some(Asking::EditDescription) => {
                if let Mode::Edit(d) = &mut self.mode {
                    d.description = text.trim().to_string();
                }
            }
            Some(Asking::EditTags) => {
                if let Mode::Edit(d) = &mut self.mode {
                    d.tags = text.trim().to_string();
                }
            }
            Some(Asking::CatalogSearch) => {
                if let Mode::Catalog(c) = &mut self.mode {
                    c.search = text.trim().to_string();
                }
                self.b.reset_cursor();
            }
            Some(Asking::SaveName) => self.save_named(text.trim()),
            Some(other) => self.text_entered(other, text),
            None => {}
        }
        self.show();
    }

    pub fn not_entered(&mut self) {
        if self.asking.take() == Some(Asking::SaveName) {
            self.save_cancelled();
        }
        self.show();
    }

    /// Whether `request` is a long one: the engine may take seconds, and the musician waits on it.
    pub fn waits(request: &Request) -> bool {
        matches!(request, Request::Open(open::Step::Prepare | open::Step::Load | open::Step::Estimate | open::Step::Save) | Request::Audition(open::Aud::Start | open::Aud::Stop(_)) | Request::Do(Do::NewPart(_)) | Request::Job(_))
    }

    /// The menu the screen has ready, to open over it.
    pub fn take_menu(&mut self) -> Option<(String, Vec<menu::Item>)> {
        self.menu.take()
    }

    /// The question the screen has ready, to open over it: title, message and choices.
    pub fn take_notice(&mut self) -> Option<(String, String, Vec<String>)> {
        self.notice.take()
    }

    /// The Performance's changes come from the engine (`graph.changed`): an audition may have ended.
    pub fn changed(&mut self) {
        if !self.auditioning.id.is_empty() {
            self.push(Request::Audition(open::Aud::Get), "audition.get", json!({}));
        }
    }

    /// Elapsed time on what the engine is busy with, said on the line under the list.
    pub fn tick(&mut self) {
        if let Some(job) = &self.job {
            let line = job.saying();
            self.say(line);
            self.show();
        }
    }
}
