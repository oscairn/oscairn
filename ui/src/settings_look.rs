// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app's pages for how Oscairn looks (ui/SETTINGS-APP.md A and T): Appearance, the theme chosen from the
//! installed ones and applied at once (the reference has no such page: its choice is the Performance window's, which
//! calls `Theme::apply` and `Theme::install`, Theme.cpp), and the Top bar page (TopBarPage.cpp), what the top bar's
//! CPU and RAM meters show. Both keep their choice in the configuration folder the JUCE app reads
//! (`appearance.txt`, `top-bar.json`), so the two programs stay in step.

use crate::library::files::{self, After, Files, Pressed, Want};
use crate::menu;
use crate::settings_app::{choice, failed, head, line, row, said, Ctx, Page};
use crate::theme::{self, Loaded, Places, ThemeData};
use crate::topbar::{MeterSettings, Tool};
use crate::{InfoLine, SettingData};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A file replaced whole: written beside it and moved over it, its folder made first (`Theme.cpp` `write`).
pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".tmp");
    let temporary = PathBuf::from(temporary);
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

// ---- Appearance ----

/// An installed theme: its id, its name and what its file sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub data: ThemeData,
}

/// `Theme::installed`: the built-in themes, then the installed ones; a folder not named after its id, one that does not
/// parse, and an installed one with a built-in's id are left out.
pub fn installed(places: &Places) -> Vec<Entry> {
    let mut found: Vec<Entry> = Vec::new();
    let entry = |text: &str, folder: Option<&str>| -> Option<Entry> {
        let data = ThemeData::parse(text).ok()?;
        folder.is_none_or(|f| f == data.id).then(|| Entry { id: data.id.clone(), name: data.name.clone(), data })
    };
    let folders = |dir: &Path| -> Vec<(String, String)> {
        let mut names: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names.into_iter().filter_map(|n| Some((n.clone(), std::fs::read_to_string(dir.join(&n).join("theme.json")).ok()?))).collect()
    };
    match &places.built_in {
        Some(dir) => found.extend(folders(dir).into_iter().filter_map(|(n, t)| entry(&t, Some(&n)))),
        None => found.extend(crate::profile::THEMES.iter().filter_map(|(_, t)| entry(t, None))),
    }
    let built_in: Vec<String> = found.iter().map(|e| e.id.clone()).collect();
    if let Some(dir) = &places.installed {
        found.extend(folders(dir).into_iter().filter_map(|(n, t)| entry(&t, Some(&n))).filter(|e| !built_in.contains(&e.id)));
    }
    found
}

pub struct Appearance {
    places: Places,
    /// The profile's theme, which is shown until one is chosen.
    screen: String,
    config: Option<PathBuf>,
    themes: Vec<Entry>,
    /// What `appearance.txt` says (empty: none chosen) and the theme shown now.
    chosen: String,
    in_use: String,
    /// Why the choice could not be used, and why the last one asked for was not taken.
    refused: String,
    failure: String,
    said: String,
    /// The file chooser, while a theme to install is being chosen.
    chooser: Option<Files>,
    /// Where the chooser starts (none: the player's Documents folder).
    files_dir: Option<PathBuf>,
}

impl Appearance {
    pub fn new(places: Places, screen: String, config: Option<PathBuf>) -> Appearance {
        Appearance { places, screen, config, themes: vec![], chosen: String::new(), in_use: String::new(), refused: String::new(), failure: String::new(), said: String::new(), chooser: None, files_dir: None }
    }

    #[cfg(test)]
    pub fn set_files_dir(&mut self, dir: PathBuf) {
        self.files_dir = Some(dir);
    }

    #[cfg(test)]
    pub fn set_folders(&mut self, config: Option<PathBuf>, places: Places) {
        self.config = config;
        self.places = places;
    }

    fn file(&self) -> Option<PathBuf> {
        self.config.as_ref().map(|c| c.join("appearance.txt"))
    }

    /// What is chosen and what that comes to (`Theme::reload`'s fallbacks).
    fn read(&mut self) {
        self.themes = installed(&self.places);
        self.chosen = self.file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default().trim().to_string();
        let shown = theme::chosen(&self.places, &self.chosen, &self.screen);
        self.in_use = shown.id;
        self.refused = shown.refused.unwrap_or_default();
    }

    /// The theme shown changed elsewhere (the JUCE app, another window): the page marks it.
    pub fn shows(&mut self, id: &str) {
        if self.in_use != id && !id.is_empty() {
            self.read();
            self.in_use = id.to_string();
        }
    }

    /// Chooses a theme: read and checked first, applied, then kept (`Theme::apply`).
    fn pick(&mut self, id: &str, c: &mut Ctx) {
        self.failure.clear();
        let loaded: Loaded = match theme::find(&self.places, id) {
            Ok(l) => l,
            Err(why) => {
                self.failure = why.to_string();
                return;
            }
        };
        let Some(file) = self.file() else {
            self.failure = "Cannot save appearance file".into();
            return;
        };
        if write_atomic(&file, id).is_err() {
            self.failure = "Cannot save appearance file".into();
            return;
        }
        self.chosen = id.to_string();
        self.in_use = loaded.id.clone();
        self.refused.clear();
        self.said = format!("{} is shown.", self.themes.iter().find(|t| t.id == id).map_or(id, |t| t.name.as_str()));
        c.theme = Some(loaded);
    }
}

impl Appearance {
    /// `Theme::install`: a theme folder or its theme.json, read and checked as `load` does, its id not a built-in's, the files
    /// it names copied into a staging folder that is renamed into place (an id installed already must be identical), then chosen.
    pub fn install(&mut self, source: &Path, c: &mut Ctx) {
        self.said.clear();
        match self.copy_in(source) {
            Ok(id) => {
                self.read();
                self.pick(&id, c);
            }
            Err(why) => self.failure = why.to_string(),
        }
    }

    fn copy_in(&mut self, source: &Path) -> Result<String, &'static str> {
        let folder = if source.is_dir() { source } else { source.parent().unwrap_or(source) };
        if !source.is_dir() && source.file_name().and_then(|n| n.to_str()) != Some("theme.json") {
            return Err("Choose a theme folder or its theme.json");
        }
        let loaded = theme::load_folder(folder)?;
        let id = loaded.data.id.clone();
        let reserved = match &self.places.built_in {
            Some(dir) => dir.join(&id).is_dir(),
            None => crate::profile::THEMES.iter().any(|(t, _)| *t == id),
        };
        if reserved {
            return Err("Built-in theme IDs are reserved");
        }
        let (font, splash) = (&loaded.data.font, &loaded.data.splash);
        let mut names: Vec<&str> = vec!["theme.json", font.regular.as_str(), font.license.as_str()];
        if !font.bold.is_empty() {
            names.push(font.bold.as_str());
        }
        if !splash.image.is_empty() {
            names.push(splash.image.as_str());
        }
        let themes = self.places.installed.clone().ok_or("Cannot create theme directory")?;
        let destination = themes.join(&id);
        if destination.exists() {
            let same = names.iter().all(|n| matches!((std::fs::read(destination.join(n)), std::fs::read(folder.join(n))), (Ok(a), Ok(b)) if a == b));
            return if same { Ok(id) } else { Err("That theme ID is already installed; use a new ID for a different theme") };
        }
        std::fs::create_dir_all(&themes).map_err(|_| "Cannot create theme directory")?;
        let staging = themes.join(format!(".{id}-install"));
        let _ = std::fs::remove_dir_all(&staging);
        let copied = (|| -> std::io::Result<()> {
            for name in &names {
                let target = staging.join(name);
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(folder.join(name), &target)?;
            }
            Ok(())
        })();
        if copied.is_err() {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("Cannot copy theme files");
        }
        std::fs::rename(&staging, &destination).map_err(|_| {
            let _ = std::fs::remove_dir_all(&staging);
            "Cannot install theme"
        })?;
        Ok(id)
    }
}

fn pretty(name: &str) -> String {
    name.to_lowercase()
}

impl Page for Appearance {
    fn open(&mut self, _c: &mut Ctx) {
        self.failure.clear();
        self.said.clear();
        self.read();
    }

    fn noun(&self) -> &'static str {
        "Themes"
    }

    fn rows(&self) -> Vec<SettingData> {
        if let Some(f) = &self.chooser {
            return f.rows();
        }
        if self.themes.is_empty() {
            return vec![row("No themes", "")];
        }
        self.themes.iter().map(|t| SettingData { on: t.id == self.in_use, ..row(t.name.as_str(), if t.id == self.in_use { "In use" } else { "" }) }).collect()
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        if let Some(f) = &self.chooser {
            return f.info();
        }
        let mut lines = vec![];
        if let Some(t) = at.and_then(|a| self.themes.get(a)) {
            let d = &t.data;
            let s = &d.style;
            lines.extend([head("Theme"), line(&t.name), line(&format!("id {}", t.id))]);
            lines.push(head("It sets"));
            lines.push(line(&format!("Spacing {:.2}, font scale {:.2}, corners {}", d.spacing, d.font_scale, d.radius)));
            lines.push(line(&format!("Tabs {}, placed {}", pretty(&format!("{:?}", s.tabs)), pretty(&format!("{:?}", s.tab_placement)))));
            lines.push(line(&format!("Buttons {}, rows {}", pretty(&format!("{:?}", s.buttons)), pretty(&format!("{:?}", s.rows)))));
            lines.push(line(&format!("Sections {}, edges {}", pretty(&format!("{:?}", s.sections)), pretty(&format!("{:?}", s.edges)))));
        }
        lines.push(head("In use"));
        lines.push(line(self.themes.iter().find(|t| t.id == self.in_use).map_or(self.in_use.as_str(), |t| t.name.as_str())));
        lines.extend(if self.chosen.is_empty() { said("None chosen: the device's theme, else the built-in default.") } else { vec![] });
        if !self.refused.is_empty() {
            lines.push(head("The theme chosen cannot be used"));
            lines.extend(failed(&format!("{}. The default is shown.", self.refused)));
        }
        if !self.failure.is_empty() {
            lines.push(head("Not applied"));
            lines.extend(failed(&self.failure));
        }
        lines
    }

    fn message(&self) -> String {
        self.said.clone()
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        self.said.clear();
        if let Some(f) = &mut self.chooser {
            if let Pressed::Chosen(path) = f.press(at) {
                self.chooser = None;
                self.install(&path, c);
            }
            return;
        }
        if let Some(id) = self.themes.get(at).map(|t| t.id.clone()) {
            self.pick(&id, c);
        }
    }

    fn tools(&self, _at: Option<usize>) -> Vec<Tool> {
        vec![Tool { label: "Install...".into(), enabled: self.chooser.is_none(), on: false }]
    }

    fn tool(&mut self, _i: usize, _at: Option<usize>, _c: &mut Ctx) {
        let dir = self.files_dir.clone().unwrap_or_else(|| files::start_dir(false));
        self.failure.clear();
        self.chooser = Some(Files::new(dir, Want::File, "json", "", After::Install, "Install a theme: choose its theme.json"));
    }

    fn back(&mut self, _c: &mut Ctx) -> bool {
        self.chooser.take().is_some()
    }

    fn path(&self) -> Vec<String> {
        if self.chooser.is_some() { vec!["Install a theme".into()] } else { vec![] }
    }
}

// ---- Top bar ----

/// Each choice as top-bar.json writes it (the first, the default, is left out; an old "device" reads as it) and as the list shows it.
const KEYS: [&str; 4] = ["both", "bar", "percent", "none"];
const NAMES: [&str; 4] = ["Bar and percent", "Bar", "Percent", "Hidden"];

pub struct TopBarPage {
    folder: Option<PathBuf>,
    /// The choice of each meter, by `KEYS`.
    cpu: usize,
    ram: usize,
    failure: String,
    asking: Option<bool>,
}

impl TopBarPage {
    pub fn new(folder: Option<PathBuf>) -> TopBarPage {
        TopBarPage { folder, cpu: 0, ram: 0, failure: String::new(), asking: None }
    }

    #[cfg(test)]
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.folder = folder;
    }

    fn file(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|f| f.join("top-bar.json"))
    }

    fn document(&self) -> Value {
        let mut chosen = serde_json::Map::new();
        for (key, i) in [("cpu", self.cpu), ("ram", self.ram)] {
            if i > 0 {
                chosen.insert(key.into(), json!(KEYS[i]));
            }
        }
        Value::Object(chosen)
    }

    /// The meters as they are shown with this choice: Bar and percent, the choice over it (`settings::over`).
    pub fn meters(&self) -> MeterSettings {
        crate::settings::over(MeterSettings::default(), &self.document())
    }
}

/// The saved `top-bar.json` choice of one meter: its index into `KEYS`, or the default (0, `both`) for a missing,
/// wrongly typed, or unknown value.
fn pick(saved: &Value, key: &str) -> usize {
    (1..KEYS.len()).find(|&i| saved[key].as_str() == Some(KEYS[i])).unwrap_or(0)
}

impl Page for TopBarPage {
    fn open(&mut self, _c: &mut Ctx) {
        self.failure.clear();
        let saved: Value = self.file().and_then(|f| std::fs::read_to_string(f).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
        self.cpu = pick(&saved, "cpu");
        self.ram = pick(&saved, "ram");
    }

    fn rows(&self) -> Vec<SettingData> {
        vec![choice("CPU", NAMES[self.cpu]), choice("RAM", NAMES[self.ram])]
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![];
        if at != Some(1) {
            lines.push(head("CPU"));
            lines.extend(said("How full the CPU is: the share of each audio block's time the engine takes."));
        }
        if at != Some(0) {
            lines.push(head("RAM"));
            lines.extend(said("How much of the device's memory is in use."));
        }
        if !self.failure.is_empty() {
            lines.push(head("Not saved"));
            lines.extend(failed(&self.failure));
        }
        lines
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        let (which, current) = match at {
            0 => (false, self.cpu),
            1 => (true, self.ram),
            _ => return,
        };
        self.asking = Some(which);
        c.menu = Some((if which { "RAM" } else { "CPU" }.into(), NAMES.iter().enumerate().map(|(i, n)| menu::Item::ticked(*n, i, i == current)).collect()));
    }

    fn chosen(&mut self, id: usize, c: &mut Ctx) {
        let Some(which) = self.asking.take() else { return };
        if id >= KEYS.len() {
            return;
        }
        if which {
            self.ram = id;
        } else {
            self.cpu = id;
        }
        // Stored at once, the file replaced whole (TopBarPage.cpp `save`); shown at once.
        let text = format!("{}\n", self.document());
        let saved = self.file().is_some_and(|f| write_atomic(&f, &text).is_ok());
        self.failure = if saved { String::new() } else { format!("Not saved: {} cannot be written.", self.file().map(|f| f.display().to_string()).unwrap_or_else(|| "the configuration folder".into())) };
        c.meters = Some(self.meters());
    }
}

// ---- Start ----

/// The choices start.txt keeps, as the list shows them.
const OPTIONS: [crate::settings::StartPage; 4] = [crate::settings::StartPage::Home, crate::settings::StartPage::Setlist, crate::settings::StartPage::Performance, crate::settings::StartPage::Library];

/// What Oscairn opens to at start, as the Settings' Start page keeps it: the word in start.txt beside the theme,
/// whole by `write_atomic`, which the shell reads when it starts (`settings::start_page`).
pub struct Start {
    folder: Option<PathBuf>,
    /// The choice, by `settings::StartPage`.
    at: crate::settings::StartPage,
    failure: String,
}

impl Start {
    pub fn new(folder: Option<PathBuf>) -> Start {
        Start { folder, at: crate::settings::StartPage::Home, failure: String::new() }
    }

    #[cfg(test)]
    pub fn set_folder(&mut self, folder: Option<PathBuf>) {
        self.folder = folder;
    }

    fn file(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|f| f.join("start.txt"))
    }
}

impl Page for Start {
    fn open(&mut self, _c: &mut Ctx) {
        self.failure.clear();
        let word = self.file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
        self.at = crate::settings::StartPage::parse(word.trim());
    }

    fn rows(&self) -> Vec<SettingData> {
        vec![choice("Opens to", self.at.name())]
    }

    fn info(&self, at: Option<usize>) -> Vec<InfoLine> {
        let mut lines = vec![head("Start")];
        lines.extend(said("What Oscairn opens to when it starts: Home, the Setlist, the Performance or the Library. The shell opens it after the splash."));
        if at.is_some() {
            lines.push(head("Now"));
            lines.push(line(&format!("Opens to {}", self.at.name())));
        }
        if !self.failure.is_empty() {
            lines.push(head("Not saved"));
            lines.extend(failed(&self.failure));
        }
        lines
    }

    fn press(&mut self, at: usize, c: &mut Ctx) {
        if at != 0 {
            return;
        }
        c.menu = Some(("Start".into(), OPTIONS.iter().enumerate().map(|(i, p)| menu::Item::ticked(p.name(), i, *p == self.at)).collect()));
    }

    fn chosen(&mut self, id: usize, _c: &mut Ctx) {
        let Some(&page) = OPTIONS.get(id) else { return };
        self.at = page;
        // Stored at once, the file replaced whole; read at the next start.
        let text = format!("{}\n", page.word());
        let saved = self.file().is_some_and(|f| write_atomic(&f, &text).is_ok());
        self.failure = if saved { String::new() } else { format!("Not saved: {} cannot be written.", self.file().map(|f| f.display().to_string()).unwrap_or_else(|| "the configuration folder".into())) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topbar::Show;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oscairn-ui-look-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A real font file from the built-in dark theme, so `load_folder`'s font check passes.
    fn font() -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../hosts/standalone/themes/oscairn-dark/fonts/Inter-Regular.ttf")).unwrap()
    }

    /// A theme.json a folder of that id and name would hold, with its font files in `fonts/`.
    fn theme(id: &str, name: &str) -> String {
        json!({"version": 7, "id": id, "name": name, "radius": 6,
            "font": {"regular": "fonts/Inter-Regular.ttf", "license": "fonts/OFL.txt"},
            "colors": {"background": "#15181d", "surface": "#1d2127", "raised": "#2c333d", "border": "#526070", "text": "#edf1f5", "muted": "#a3aebb", "accent": "#ffa45b", "positive": "#5ad9a4", "danger": "#ff8d87"},
            "style": {"buttons": "pill", "edges": "hairline", "depth": "soft", "sliders": "line", "tabs": "segmented", "marks": "square", "sections": "separate", "rows": "left"},
            "wires": {"audio": "#8cc2ff", "notes": "#c29cff", "disabled": "#6b7480", "draining": "#ffa45b", "unconnected": "#ff6b63", "control": "#6fe0a8"}})
            .to_string()
    }

    /// A folder `dir/<id>` named after its id, with a theme.json and its font files, usable to install.
    fn theme_folder(dir: &Path, id: &str, name: &str) -> PathBuf {
        let folder = dir.join(id);
        std::fs::create_dir_all(folder.join("fonts")).unwrap();
        std::fs::write(folder.join("fonts/Inter-Regular.ttf"), font()).unwrap();
        std::fs::write(folder.join("fonts/OFL.txt"), "license\n").unwrap();
        std::fs::write(folder.join("theme.json"), theme(id, name)).unwrap();
        folder
    }

    // `write_atomic` writes the whole text to a new path, its folder made first, and leaves no `.tmp` beside it.
    #[test]
    fn write_atomic_writes_a_new_file_whole_with_no_temp_left_over() {
        let dir = temp("atomic");
        let file = dir.join("nested").join("notes.txt");
        write_atomic(&file, "one\n").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "one\n");
        assert!(!dir.join("nested").join("notes.txt.tmp").exists(), "the temporary is renamed, not left behind");
    }

    // A file already there is replaced whole: a longer second write leaves none of the first.
    #[test]
    fn write_atomic_replaces_a_whole_existing_file() {
        let dir = temp("atomic-over");
        let file = dir.join("notes.txt");
        write_atomic(&file, "a short line").unwrap();
        write_atomic(&file, "a very much longer second line that must fully replace it").unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "a very much longer second line that must fully replace it");
        assert!(!dir.join("notes.txt.tmp").exists());
    }

    // `installed` takes the built-in folders named after their id first, leaving out a folder its id does not name,
    // one whose file does not parse, and one with no file; then the installed folders not already there.
    #[test]
    fn installed_lists_the_built_in_folders_that_parse_and_their_id_matches() {
        let dir = temp("installed");
        let built_in = dir.join("built-in");
        std::fs::create_dir_all(&built_in).unwrap();
        std::fs::create_dir_all(built_in.join("known")).unwrap();
        std::fs::write(built_in.join("known/theme.json"), theme("known", "Known")).unwrap();
        std::fs::create_dir_all(built_in.join("folder1")).unwrap();
        std::fs::write(built_in.join("folder1/theme.json"), theme("other-id", "Other")).unwrap(); // folder name is not its id
        std::fs::create_dir_all(built_in.join("broken")).unwrap();
        std::fs::write(built_in.join("broken/theme.json"), "{not json").unwrap();
        std::fs::create_dir_all(built_in.join("bare")).unwrap(); // no theme.json at all
        let places = Places { built_in: Some(built_in), installed: None };
        let have = installed(&places);
        let ids: Vec<&str> = have.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["known"], "only the folder whose name is its id and that parses is listed: {ids:?}");
        assert_eq!(have[0].name, "Known");
    }

    // An installed theme whose id a built-in already has is not listed again; a new one is, after the built-in ones,
    // in folder order.
    #[test]
    fn installed_leaves_out_an_installed_theme_with_a_built_in_id() {
        let dir = temp("installed-back");
        let built_in = dir.join("built-in");
        std::fs::create_dir_all(&built_in).unwrap();
        std::fs::create_dir_all(built_in.join("a")).unwrap();
        std::fs::write(built_in.join("a/theme.json"), theme("a", "A")).unwrap();
        let installed_dir = dir.join("installed");
        std::fs::create_dir_all(installed_dir.join("b")).unwrap();
        std::fs::write(installed_dir.join("b/theme.json"), theme("b", "B")).unwrap();
        std::fs::create_dir_all(installed_dir.join("a")).unwrap();
        std::fs::write(installed_dir.join("a/theme.json"), theme("a", "A again")).unwrap(); // same id as a built-in
        let places = Places { built_in: Some(built_in), installed: Some(installed_dir) };
        let have = installed(&places);
        let ids: Vec<&str> = have.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"], "the copy of a built-in id is not listed again");
        assert_eq!(have[1].name, "B");
    }

    // `pretty` is the name made lowercase: empty stays empty, capitals and Unicode map down, an already-lower name
    // is unchanged.
    #[test]
    fn pretty_lowercases_the_style_words() {
        assert_eq!(pretty(""), "");
        assert_eq!(pretty("Tabs"), "tabs");
        assert_eq!(pretty("JoinEd Sections"), "joined sections");
        assert_eq!(pretty("ÜNÍCÖDE"), "ünícöde");
        assert_eq!(pretty("already lower"), "already lower");
    }

    // `copy_in` copies a theme folder or its theme.json into the installed place, named after its id.
    #[test]
    fn copy_in_installs_a_theme_folder_or_its_theme_json() {
        let base = temp("copy");
        let places = Places { built_in: Some(base.join("built-in")), installed: Some(base.join("installed")) };
        std::fs::create_dir_all(places.built_in.as_ref().unwrap()).unwrap();
        let mut a = Appearance::new(places.clone(), "screen".into(), None);
        let folder = theme_folder(&base, "mine", "Mine");
        assert_eq!(a.copy_in(&folder), Ok("mine".into()));
        let dest = places.installed.as_ref().unwrap().join("mine");
        assert!(dest.join("theme.json").is_file() && dest.join("fonts/Inter-Regular.ttf").is_file(), "the theme's files are copied");
        // Its theme.json itself is a mountable source too.
        let mut a = Appearance::new(places.clone(), "screen".into(), None);
        assert_eq!(a.copy_in(&folder.join("theme.json")), Ok("mine".into()));
    }

    // `copy_in` refuses a file that is not a theme.json, a theme whose id a built-in has, a folder already installed
    // differently, and accepts one already installed identically.
    #[test]
    fn copy_in_refuses_non_theme_and_reserved_or_different_ids() {
        let base = temp("copy-refusal");
        let built_in = base.join("built-in");
        std::fs::create_dir_all(built_in.join("built")).unwrap();
        let places = Places { built_in: Some(built_in), installed: Some(base.join("installed")) };
        let mut a = Appearance::new(places.clone(), "screen".into(), None);
        let folder = theme_folder(&base, "mine", "Mine");
        std::fs::write(base.join("else.txt"), "not a theme").unwrap();
        assert_eq!(a.copy_in(&base.join("else.txt")), Err("Choose a theme folder or its theme.json"));
        let built = theme_folder(&base, "built", "Built");
        assert_eq!(a.copy_in(&built), Err("Built-in theme IDs are reserved"));
        // A second, identical install is accepted; a different file is not.
        assert_eq!(a.copy_in(&folder), Ok("mine".into()));
        assert_eq!(a.copy_in(&folder), Ok("mine".into()));
        let json = folder.join("theme.json");
        std::fs::write(&json, format!("{}\n", theme("mine", "Mine"))).unwrap();
        assert_eq!(a.copy_in(&folder), Err("That theme ID is already installed; use a new ID for a different theme"));
    }

    // `document` holds each meter's `KEYS` word, leaving a meter at the default out.
    #[test]
    fn document_writes_each_meter_word_and_leaves_the_default_out() {
        let both: TopBarPage = TopBarPage::new(None);
        assert_eq!(both.document(), Value::Object(Default::default()), "both at the default writes an empty file");
        let mut t = TopBarPage::new(None);
        t.cpu = 1;
        assert_eq!(t.document(), json!({"cpu": "bar"}));
        let mut t = TopBarPage::new(None);
        t.ram = 3;
        assert_eq!(t.document(), json!({"ram": "none"}));
        let mut t = TopBarPage::new(None);
        t.cpu = 2;
        t.ram = 1;
        assert_eq!(t.document(), json!({"cpu": "percent", "ram": "bar"}));
    }

    // Reading a saved `top-bar.json` back: a missing, wrongly typed, or unknown field is the default (`both`); each
    // `KEYS` word is its meter.
    #[test]
    fn pick_reads_the_saved_word_and_defaults_the_rest() {
        assert_eq!(pick(&Value::Null, "cpu"), 0);
        assert_eq!(pick(&json!({}), "cpu"), 0);
        assert_eq!(pick(&json!({"cpu": 1}), "cpu"), 0, "a number is not a choice");
        assert_eq!(pick(&json!({"cpu": ["bar"]}), "cpu"), 0, "an array is not a choice");
        assert_eq!(pick(&json!({"cpu": "sideways"}), "cpu"), 0, "an unknown word is the default");
        assert_eq!(pick(&json!({"cpu": "both"}), "cpu"), 0, "the first key is the default, not written out");
        assert_eq!(pick(&json!({"cpu": "bar"}), "cpu"), 1);
        assert_eq!(pick(&json!({"cpu": "percent"}), "cpu"), 2);
        assert_eq!(pick(&json!({"ram": "none"}), "ram"), 3);
        assert_eq!(pick(&json!({"cpu": "bar"}), "ram"), 0, "the other meter is left alone");
    }

    // `meters`: the choice shows on the matching meter over the default (both), each word to its `Show`.
    #[test]
    fn meters_map_the_choice_to_what_the_top_bar_shows() {
        let t = TopBarPage::new(None);
        assert_eq!(t.meters(), MeterSettings::default());
        let mut t = TopBarPage::new(None);
        t.cpu = 3;
        t.ram = 1;
        assert_eq!(t.meters(), MeterSettings { cpu: Show::None, ram: Show::Bar });
    }
}
