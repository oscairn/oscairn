// SPDX-License-Identifier: AGPL-3.0-or-later
//! The file chooser (ui/LIBRARY-APP.md D3): the JUCE `FileChooser` the Library uses to pick an archive to install, a
//! name and place for an export, a folder to index in place and a plugin build. The Rust shell has no native
//! dialog, and the files are this device's own, so it is a list of the folder's folders and files over the local
//! file system (`std::fs`; no engine request): Up, then folders (a press goes into one) and files (a press
//! chooses one); to choose a folder there is a row for the folder shown; to save, a name row and a row that saves here.

use super::*;
use std::path::{Path, PathBuf};

/// What is being chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    /// A file with the extension.
    File,
    /// A name, in the folder shown, with the extension.
    Save,
    /// The folder shown.
    Folder,
    /// A file or a bundle (a folder) with the extension, as a plugin build is.
    Build,
}

/// What the chosen path is for.
#[derive(Clone, Debug, PartialEq)]
pub enum After {
    /// An archive to install.
    Install,
    /// An item exported to the file: its ID.
    ExportItem(String),
    /// The library exported: its name and the setlists it takes.
    ExportLibrary(String, Vec<String>),
    /// A folder to index in place.
    Folder,
    /// A build of an installed plugin for another platform: the plugin and the platform come after.
    Build(Box<super::libraries::PluginRef>),
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    name: String,
    folder: bool,
    size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Files {
    pub dir: PathBuf,
    pub want: Want,
    /// The extension files carry, without the dot ("olib", "clap").
    pub ext: &'static str,
    /// What the file is called, when it is saved.
    pub name: String,
    pub after: After,
    /// What this was opened to say: "Install an archive".
    pub title: String,
    entries: Vec<Entry>,
    /// Why the folder could not be read.
    error: String,
}

/// Where a chooser starts: the user's Documents folder, else the home folder, else the root.
pub fn start_dir(music: bool) -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    let wanted = home.join(if music { "Music" } else { "Documents" });
    if wanted.is_dir() {
        wanted
    } else if home.is_dir() {
        home
    } else {
        PathBuf::from("/")
    }
}

impl Files {
    pub fn new(dir: PathBuf, want: Want, ext: &'static str, name: &str, after: After, title: &str) -> Files {
        let mut f = Files { dir, want, ext, name: name.to_string(), after, title: title.to_string(), entries: vec![], error: String::new() };
        f.read();
        f
    }

    /// A bundle is chosen, not entered: a folder with the file extension, where a build is wanted.
    fn bundle(&self, e: &Entry) -> bool {
        self.want == Want::Build && e.folder && e.name.ends_with(&format!(".{}", self.ext))
    }

    /// The folder's entries: folders first, then the files with the extension, by name, hidden ones left out.
    pub fn read(&mut self) {
        self.entries.clear();
        self.error.clear();
        let read = match std::fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) => {
                self.error = format!("Cannot read {}: {e}", self.dir.display());
                return;
            }
        };
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let meta = entry.metadata();
            let folder = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.map(|m| m.len()).unwrap_or(0);
            if folder || (self.want != Want::Folder && name.ends_with(&format!(".{}", self.ext))) {
                self.entries.push(Entry { name, folder, size });
            }
        }
        self.entries.sort_by_key(|e| (!e.folder, e.name.to_lowercase()));
    }

    /// What a row does, by its number: the rows are Up, then what the want needs (the folder row; the name and Save
    /// rows), then the entries.
    fn lead(&self) -> usize {
        match self.want {
            Want::Folder => 2,
            Want::Save => 3,
            _ => 1,
        }
    }

    pub fn rows(&self) -> Vec<SettingData> {
        let row = |label: String, value: String, kind: i32| SettingData { label: label.into(), value: value.into(), kind, ..Default::default() };
        let parent = self.dir.parent().map(|p| p.file_name().map_or("/".to_string(), |n| n.to_string_lossy().into_owned()));
        let mut rows = vec![SettingData { label: "‹ Up".into(), value: parent.unwrap_or_default().into(), kind: 2, ..Default::default() }];
        match self.want {
            Want::Folder => rows.push(row("Choose this folder".into(), format!("{} ›", self.dir.file_name().map_or("/".to_string(), |n| n.to_string_lossy().into_owned())), 2)),
            Want::Save => {
                rows.push(row("Name".into(), format!("{} ›", if self.name.is_empty() { "none" } else { &self.name }), 2));
                rows.push(row("Save here".into(), String::new(), 2));
            }
            _ => {}
        }
        rows.extend(self.entries.iter().map(|e| {
            if e.folder && !self.bundle(e) {
                row(e.name.clone(), "›".into(), 2)
            } else {
                row(e.name.clone(), if e.folder { String::new() } else { size_text(e.size) }, 2)
            }
        }));
        rows
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let mut lines = vec![head(&self.title)];
        lines.extend(wrap_path(&self.dir.display().to_string(), INFO_WIDTH).into_iter().map(line));
        if !self.error.is_empty() {
            lines.push(InfoLine { text: self.error.as_str().into(), failed: true, ..Default::default() });
        } else if self.entries.is_empty() {
            lines.push(line(match self.want {
                Want::Folder => "No folders here".into(),
                _ => format!("No folders or .{} files here", self.ext),
            }));
        }
        lines
    }

    /// Up to the parent; false at the root.
    pub fn up(&mut self) -> bool {
        match self.dir.parent().map(Path::to_path_buf) {
            Some(p) => {
                self.dir = p;
                self.read();
                true
            }
            None => false,
        }
    }

    /// A row pressed: what it does.
    pub fn press(&mut self, at: usize) -> Pressed {
        let lead = self.lead();
        if at == 0 {
            self.up();
            return Pressed::Moved;
        }
        match (self.want, at) {
            (Want::Folder, 1) => return Pressed::Chosen(self.dir.clone()),
            (Want::Save, 1) => return Pressed::AskName,
            (Want::Save, 2) => {
                let name = self.name.trim();
                if name.is_empty() {
                    return Pressed::Nothing;
                }
                return Pressed::Chosen(self.dir.join(if name.ends_with(&format!(".{}", self.ext)) { name.to_string() } else { format!("{name}.{}", self.ext) }));
            }
            _ => {}
        }
        let Some(e) = self.entries.get(at - lead).cloned() else { return Pressed::Nothing };
        if e.folder && !self.bundle(&e) {
            self.dir = self.dir.join(&e.name);
            self.read();
            return Pressed::Moved;
        }
        if self.want == Want::Save {
            // A file of the folder: its name is the one to save as.
            self.name = e.name.trim_end_matches(&format!(".{}", self.ext)).to_string();
            return Pressed::Named;
        }
        Pressed::Chosen(self.dir.join(&e.name))
    }
}

#[derive(Debug, PartialEq)]
pub enum Pressed {
    Nothing,
    /// The folder changed: the list is new.
    Moved,
    /// The name is asked for on the keyboard.
    AskName,
    /// The name was set from a file in the folder.
    Named,
    Chosen(PathBuf),
}

/// A path on lines of at most `width` characters, broken after a slash (a word of a path is not cut unless it must be).
pub fn wrap_path(path: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for part in path.split_inclusive('/') {
        match lines.last_mut() {
            Some(l) if l.chars().count() + part.chars().count() <= width => l.push_str(part),
            _ => lines.push(part.to_string()),
        }
    }
    lines.into_iter().flat_map(|l| l.chars().collect::<Vec<_>>().chunks(width).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>()).collect()
}

/// "1.2 MB", as `juce::File::descriptionOfSizeInBytes` words a size.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    if bytes == 1 {
        return "1 byte".into();
    }
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024. && unit < UNITS.len() - 1 {
        size /= 1024.;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oscairn-ui-files-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Songs")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::create_dir_all(dir.join("Plug.clap")).unwrap();
        std::fs::write(dir.join("b.olib"), vec![0u8; 3000]).unwrap();
        std::fs::write(dir.join("a.olib"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        std::fs::write(dir.join("Songs").join("c.olib"), b"x").unwrap();
        dir
    }

    #[test]
    fn folders_then_files_of_the_kind_hidden_ones_left_out() {
        let dir = folder("list");
        let f = Files::new(dir.clone(), Want::File, "olib", "", After::Install, "Install");
        let labels: Vec<String> = f.rows().into_iter().map(|r| r.label.to_string()).collect();
        assert_eq!(labels, ["‹ Up", "Plug.clap", "Songs", "a.olib", "b.olib"]);
        assert_eq!(f.rows()[4].value.to_string(), "2.9 KB");
        assert_eq!(f.rows()[2].value.to_string(), "›");
    }

    #[test]
    fn a_press_enters_goes_up_and_chooses() {
        let dir = folder("press");
        let mut f = Files::new(dir.clone(), Want::File, "olib", "", After::Install, "Install");
        assert_eq!(f.press(2), Pressed::Moved, "Songs");
        assert_eq!(f.dir, dir.join("Songs"));
        assert_eq!(f.press(1), Pressed::Chosen(dir.join("Songs").join("c.olib")));
        assert_eq!(f.press(0), Pressed::Moved);
        assert_eq!(f.dir, dir);
        assert_eq!(f.press(4), Pressed::Chosen(dir.join("b.olib")));
    }

    #[test]
    fn a_folder_is_chosen_by_its_own_row() {
        let dir = folder("folder");
        let mut f = Files::new(dir.clone(), Want::Folder, "", "", After::Folder, "Index");
        let labels: Vec<String> = f.rows().into_iter().map(|r| r.label.to_string()).collect();
        assert_eq!(labels, ["‹ Up", "Choose this folder", "Plug.clap", "Songs"], "folders only");
        assert_eq!(f.press(1), Pressed::Chosen(dir.clone()));
        assert_eq!(f.press(3), Pressed::Moved);
        assert_eq!(f.press(1), Pressed::Chosen(dir.join("Songs")));
    }

    #[test]
    fn a_name_is_asked_and_the_extension_added() {
        let dir = folder("save");
        let mut f = Files::new(dir.clone(), Want::Save, "olib", "Evening", After::ExportItem("p".into()), "Export");
        let labels: Vec<String> = f.rows().into_iter().map(|r| r.label.to_string()).collect();
        assert_eq!(labels[..3], ["‹ Up", "Name", "Save here"]);
        assert_eq!(f.press(1), Pressed::AskName);
        assert_eq!(f.press(2), Pressed::Chosen(dir.join("Evening.olib")));
        f.name = "Evening.olib".into();
        assert_eq!(f.press(2), Pressed::Chosen(dir.join("Evening.olib")), "not twice");
        f.name.clear();
        assert_eq!(f.press(2), Pressed::Nothing);
        // A file of the folder gives its name.
        assert_eq!(f.press(5), Pressed::Named);
        assert_eq!(f.name, "a");
    }

    #[test]
    fn a_bundle_is_chosen_not_entered() {
        let dir = folder("build");
        let mut f = Files::new(dir.clone(), Want::Build, "clap", "", After::Folder, "Build");
        assert_eq!(f.press(1), Pressed::Chosen(dir.join("Plug.clap")));
        assert_eq!(f.press(2), Pressed::Moved, "Songs is entered");
    }

    #[test]
    fn a_folder_that_cannot_be_read_says_so() {
        let f = Files::new(PathBuf::from("/nonexistent-oscairn-folder"), Want::File, "olib", "", After::Install, "Install");
        assert!(f.info().iter().any(|l| l.failed && l.text.starts_with("Cannot read")));
    }

    #[test]
    fn a_path_is_wrapped_after_its_slashes() {
        assert_eq!(wrap_path("/var/folders/hq/T/oscairn", 16), ["/var/folders/hq/", "T/oscairn"]);
        let lines = wrap_path("/a/b/c/d", 4);
        assert_eq!(lines, ["/a/", "b/c/", "d"]);
        assert!(wrap_path("/an-extremely-long-folder-name-without-any-break", 10).iter().all(|l| l.chars().count() <= 10));
    }

    #[test]
    fn sizes_are_worded_as_juces_are() {
        assert_eq!(size_text(1), "1 byte");
        assert_eq!(size_text(10), "10 bytes");
        assert_eq!(size_text(2048), "2.0 KB");
        assert_eq!(size_text(5 << 20), "5.0 MB");
    }
}
