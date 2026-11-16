// SPDX-License-Identifier: AGPL-3.0-or-later
//! The long requests the Library makes: an install, an export, a build copied in, a library uninstalled, a
//! plugin removed, a folder indexed (ui/LIBRARY-APP.md C8, D2, D8 to D11). The JUCE app runs each on a connection that
//! waits its turn behind another's long request (Engine.cpp `refreshSounds`); the Rust shell has one, so one the
//! engine refuses as busy (-32009: it did not run) is asked again after 100 ms, and the line under the list says
//! what is going on.

use super::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// library.install, from an archive.
    Install,
    /// library.export: where the archive was really written, and the key it was signed with.
    Export(Option<export::ReplaceInfo>, Option<String>),
    /// library.exportLibrary.
    ExportLibrary(Option<export::ReplaceInfo>),
    /// plugins.addBuild, for this platform.
    AddBuild(String),
    /// library.uninstall: the library's name.
    Uninstall(String),
    /// plugins.remove: the plugin's name and version.
    RemovePlugin(String),
    /// library.addFolder: the folder's name.
    AddFolder(String),
    /// library.removeFolder.
    RemoveFolder,
    /// performance.import.
    Import,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub kind: Kind,
    pub method: &'static str,
    pub params: Value,
}

/// The archive's notes, each as a sentence after the message ("Installed 2. Skipped one.").
fn with_notes(mut said: String, notes: &Value) -> String {
    for n in notes.as_array().into_iter().flatten().filter_map(Value::as_str) {
        said += &format!(" {n}.");
    }
    said
}

impl Screen {
    /// Starts a long request: the line says what, and the answer is `Request::Job`'s.
    pub(super) fn start_job(&mut self, kind: Kind, method: &'static str, params: Value, saying: &str) {
        self.say(saying);
        self.push(Request::Job(Job { kind, method, params: params.clone() }), method, params);
    }

    /// An answer. True when the Performance changed under the screen.
    pub(super) fn job_reply(&mut self, job: Job, result: Result<Value, Error>) -> bool {
        if let Err(e) = &result {
            if e.code == BUSY {
                let (method, params) = (job.method, job.params.clone());
                self.push_after(Duration::from_millis(100), Request::Job(job), method, params);
                return false;
            }
        }
        let ok = result.is_ok();
        // An archive written beside the old file goes over it now (or the folder goes if it failed).
        let kind = match job.kind {
            Kind::Export(Some(replace), key) => {
                if let Err(e) = replace.finish(ok) {
                    self.say(e);
                    self.refresh_after_job();
                    return false;
                }
                Kind::Export(None, key)
            }
            Kind::ExportLibrary(Some(replace)) => {
                if let Err(e) = replace.finish(ok) {
                    self.say(e);
                    self.refresh_after_job();
                    return false;
                }
                Kind::ExportLibrary(None)
            }
            k => k,
        };
        let r = match result {
            Ok(r) => r,
            Err(e) => {
                // The reason, as the engine words it (the JUCE pane shows `soundMessage`).
                self.say(e.message);
                self.refresh_after_job();
                return false;
            }
        };
        let imported = kind == Kind::Import;
        let said = match kind {
            Kind::Install => {
                let skipped = r["skipped"].as_u64().unwrap_or(0);
                with_notes(format!("Installed {}{}.", r["added"].as_u64().unwrap_or(0), if skipped > 0 { format!(", {skipped} already here") } else { String::new() }), &r["notes"])
            }
            Kind::Export(_, key) => with_notes(format!("Exported.{}", key.map_or(String::new(), |k| format!(" Signed with {k}."))), &r["notes"]),
            Kind::ExportLibrary(_) => with_notes("Library exported.".into(), &r["notes"]),
            Kind::AddBuild(platform) => format!("Added the {platform} build."),
            Kind::Uninstall(name) => format!("{name} uninstalled."),
            Kind::RemovePlugin(name) => format!("{name} removed."),
            Kind::AddFolder(name) => format!("{name} is indexed in place."),
            Kind::RemoveFolder => String::new(),
            Kind::Import => "Performance imported.".into(),
        };
        self.say(said);
        self.refresh_after_job();
        imported
    }

    /// What a job may have changed is read again: the list, the libraries installed, the plugin packages.
    pub(super) fn refresh_after_job(&mut self) {
        self.query();
        self.push(Request::Libraries, "library.libraries", json!({}));
        self.push(Request::Plugins, "packages.installed", json!({}));
    }
}
