// SPDX-License-Identifier: AGPL-3.0-or-later
//! Themes (docs/design-language.md, docs/theme.schema.json): the same folders and the same
//! choice the JUCE app reads, so a theme chosen in either UI shows in both. A theme folder is
//! `<id>/theme.json` with its fonts; built-in ones are beside the program
//! (`share/oscairn/themes`, the JUCE app's), installed ones in the configuration folder's
//! `themes/`, and `appearance.txt` there names the one chosen. Read live: a new choice is seen
//! within a second and repaints the window once.
//!
//! What the Rust UI takes from a theme: the colors (the nine roles, and the optional `warning`
//! and `focus`), `radius`, `fontScale` and the two font files. Any of them missing or unusable
//! falls back to the default built-in theme's, key by key, so an odd theme never fails. `style`,
//! `wires` and `splash` are for screens the Rust UI does not draw yet.

use crate::profile::{DEFAULT_THEME, THEMES};
use crate::{Shell, Theme};
use serde_json::Value;
use slint::{Color, ComponentHandle};
use std::path::{Path, PathBuf};

/// Where themes are: the built-in folder (none: the built-in themes this program was built
/// with, without their font files) and the installed one.
#[derive(Clone, Debug, Default)]
pub struct Places {
    pub built_in: Option<PathBuf>,
    pub installed: Option<PathBuf>,
}

impl Places {
    /// Beside the program as the JUCE app has them: share/oscairn/themes (Linux) or themes/ beside it.
    pub fn found() -> Places {
        let exe = std::env::current_exe().ok();
        let dir = exe.as_deref().and_then(Path::parent);
        let built_in = dir.into_iter().flat_map(|d| [d.join("../share/oscairn/themes"), d.join("themes")]).find(|d| d.join("default").is_file());
        Places { built_in, installed: crate::settings::config_dir().map(|c| c.join("themes")) }
    }
}

/// A theme read: its theme.json over the default's, and its folder (none: built in, no fonts).
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub id: String,
    pub doc: Value,
    pub folder: Option<PathBuf>,
}

fn read(folder: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(folder.join("theme.json")).ok().filter(|t| t.len() <= 16 << 10)?;
    serde_json::from_str::<Value>(&text).ok().filter(Value::is_object)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The theme `id` names: a built-in folder's, an installed one's (built-in IDs win, as in the
/// JUCE app), else the one this program was built with.
fn find(places: &Places, id: &str) -> Option<(Value, Option<PathBuf>)> {
    if !valid_id(id) {
        return None;
    }
    for dir in [&places.built_in, &places.installed].into_iter().flatten() {
        let folder = dir.join(id);
        if let Some(doc) = read(&folder) {
            return Some((doc, Some(folder)));
        }
    }
    THEMES.iter().find(|(t, _)| *t == id).and_then(|(_, text)| serde_json::from_str(text).ok()).map(|doc| (doc, None))
}

/// `over` with every key `base` has and it lacks taken from `base`, object by object.
pub fn merged(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out = b.clone();
            for (k, v) in o {
                out.insert(k.clone(), merged(&b.get(k).cloned().unwrap_or(Value::Null), v));
            }
            Value::Object(out)
        }
        (_, Value::Null) => base.clone(),
        _ => over.clone(),
    }
}

/// The theme to show: the user's choice, else `screen`'s (the profile's), else the built-in
/// default (`themes/default`), else Oscairn Dark; over the default for anything it lacks.
pub fn chosen(places: &Places, choice: &str, screen: &str) -> Loaded {
    let default_id = places.built_in.as_ref().and_then(|d| std::fs::read_to_string(d.join("default")).ok()).map(|t| t.trim().to_string()).filter(|t| valid_id(t)).unwrap_or(DEFAULT_THEME.into());
    let base = find(places, &default_id).or_else(|| find(places, "oscairn-dark")).map(|t| t.0).unwrap_or_default();
    for id in [choice, screen, &default_id, "oscairn-dark"] {
        if let Some((doc, folder)) = find(places, id) {
            return Loaded { id: id.into(), doc: merged(&base, &doc), folder };
        }
    }
    Loaded { id: String::new(), doc: base, folder: None }
}

fn color(v: &Value) -> Option<Color> {
    let hex = v.as_str()?.strip_prefix('#').filter(|h| h.len() == 6)?;
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// A font file of the theme's folder, by the theme's relative path (never outside the folder),
/// registered for Slint's renderers; its family's name.
#[cfg(not(test))]
fn register_font(folder: &Path, file: &Value) -> Option<String> {
    use slint::fontique_011::fontique;
    let file = file.as_str().filter(|f| !f.contains("..") && !f.starts_with('/'))?;
    let data = std::fs::read(folder.join(file)).ok().filter(|d| d.len() <= 4 << 20)?;
    let mut collection = slint::fontique_011::shared_collection();
    let fonts = collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(data)), None);
    let family = fonts.first()?.0;
    collection.family_name(family).map(String::from)
}

// Headless tests keep the built-in Inter: the renderer's fonts are shared by every test thread.
#[cfg(test)]
fn register_font(_: &Path, _: &Value) -> Option<String> {
    None
}

/// Puts the theme in the window's `Theme` global. Every key is set, so a switch leaves nothing of
/// the theme before; one that cannot be read is the default theme's (`chosen` merged it in).
pub fn apply(window: &Shell, theme: &Loaded) {
    let g = window.global::<Theme>();
    let c = &theme.doc["colors"];
    let pick = |key: &str, or: Color| color(&c[key]).unwrap_or(or);
    let accent = pick("accent", Color::from_rgb_u8(0xff, 0x9a, 0x45));
    g.set_background(pick("background", Color::from_rgb_u8(0x11, 0x13, 0x17)));
    g.set_surface(pick("surface", Color::from_rgb_u8(0x19, 0x1c, 0x21)));
    g.set_raised(pick("raised", Color::from_rgb_u8(0x26, 0x2b, 0x32)));
    g.set_border(pick("border", Color::from_rgb_u8(0x3a, 0x41, 0x4b)));
    g.set_text(pick("text", Color::from_rgb_u8(0xf6, 0xf8, 0xfa)));
    g.set_muted(pick("muted", Color::from_rgb_u8(0xb4, 0xbd, 0xc8)));
    g.set_accent(accent);
    g.set_positive(pick("positive", Color::from_rgb_u8(0x5a, 0xd9, 0xa4)));
    g.set_danger(pick("danger", Color::from_rgb_u8(0xff, 0x6b, 0x63)));
    g.set_warning(pick("warning", Color::from_rgb_u8(0xe8, 0xc5, 0x47))); // the JUCE meters' yellow
    g.set_focus(pick("focus", accent));
    // The JUCE app's ranges (Theme.cpp): a radius of 0 to 16, a font scale of 0.8 to 1.4.
    g.set_radius(theme.doc["radius"].as_f64().filter(|r| r.is_finite()).unwrap_or(6.).clamp(0., 16.) as f32);
    g.set_font_scale(theme.doc["fontScale"].as_f64().filter(|s| s.is_finite()).unwrap_or(1.).clamp(0.8, 1.4) as f32);
    let family = theme.folder.as_deref().and_then(|f| {
        let regular = register_font(f, &theme.doc["font"]["regular"]);
        register_font(f, &theme.doc["font"]["bold"]);
        regular
    });
    g.set_font_family(family.unwrap_or("Inter".into()).into());
}

/// Watches the user's choice, once a second (a file read; nothing is drawn unless it changed),
/// and applies a new one. The timer lives as long as the window's event loop.
pub fn follow(window: &Shell, places: Places, screen: String, timer: &slint::Timer) {
    let weak = window.as_weak();
    let mut shown = crate::settings::chosen_theme();
    timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
        let choice = crate::settings::chosen_theme();
        if choice == shown {
            return;
        }
        shown = choice;
        if let Some(window) = weak.upgrade() {
            apply(&window, &chosen(&places, &shown, &screen));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("oscairn-ui-themes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn chosen_as_the_juce_app_chooses() {
        let built_in = Places::default(); // the ones built in
        assert_eq!(chosen(&built_in, "", "oscairn-block").id, "oscairn-block");
        assert_eq!(chosen(&built_in, "oscairn-light", "oscairn-block").id, "oscairn-light");
        assert_eq!(chosen(&built_in, "Not An Id", "").id, DEFAULT_THEME);
        assert_eq!(chosen(&built_in, "../etc", "").id, DEFAULT_THEME);
    }

    #[test]
    fn installed_themes_and_missing_keys() {
        let installed = temp("installed");
        std::fs::create_dir_all(installed.join("odd")).unwrap();
        std::fs::write(installed.join("odd/theme.json"), json!({"version": 7, "id": "odd", "colors": {"accent": "#00FF00", "text": "red"}, "fontScale": 9}).to_string()).unwrap();
        let places = Places { built_in: None, installed: Some(installed.clone()) };
        let t = chosen(&places, "odd", "");
        assert_eq!((t.id.as_str(), t.folder.as_deref()), ("odd", Some(installed.join("odd").as_path())));
        assert_eq!(t.doc["colors"]["accent"], "#00FF00");
        let dark = chosen(&places, "", "");
        assert_eq!(t.doc["colors"]["background"], dark.doc["colors"]["background"], "missing: the default's");
        assert_eq!((color(&t.doc["colors"]["text"]), color(&json!("#FFF"))), (None, None)); // unusable: apply falls back
        std::fs::write(installed.join("odd/theme.json"), "{ not json").unwrap();
        assert_eq!(chosen(&places, "odd", "").id, DEFAULT_THEME);
        let _ = std::fs::remove_dir_all(&installed);
    }
}
