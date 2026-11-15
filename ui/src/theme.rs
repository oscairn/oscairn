// SPDX-License-Identifier: AGPL-3.0-or-later
//! Themes (docs/design-language.md, docs/theme.schema.json): the same folders, the same files and
//! the same rules as the JUCE app (hosts/standalone/Theme.cpp `ThemeData::parse`, `Theme::load`,
//! `Theme::reload`), the same choice. A theme folder is `<id>/theme.json` with its fonts;
//! built-in ones are beside the program (`share/oscairn/themes`, the JUCE app's), installed ones
//! in the configuration folder's `themes/`, and `appearance.txt` there names the one chosen.
//! Read live: a new choice is seen within a second and repaints the window once.
//!
//! Everything a theme file can set is read: the nine colors (and the optional `warning` and
//! `focus`), `radius`, `fontScale`, `spacing`, the eight `style` choices, the six `wires` colors, the
//! `splash`, and the font files. A file is accepted or refused whole, as the reference does:
//! only `fontScale`, `spacing`, `radius`, `colors.warning`, `colors.focus`, `font.bold` and `splash` may be
//! absent (their defaults below); anything else missing, unknown, mistyped or out of range, text
//! contrast under 4.5:1, a folder not named after its id or a font the reference would not take
//! refuses the theme, which then shows the default theme, else the emergency one, as `reload`
//! does. Held per window: `apply` sets the window's `Theme` global, nothing is process-wide.

use crate::profile::{DEFAULT_THEME, THEMES};
use crate::{ButtonStyle, DepthStyle, EdgeStyle, MarkStyle, RowStyle, SectionStyle, Shell, SliderStyle, TabStyle, Theme};
use serde_json::{Map, Value};
use slint::{Color, ComponentHandle};
use std::path::{Path, PathBuf};

const MAX_THEME_BYTES: u64 = 16384;
const MAX_FONT_BYTES: u64 = 4 << 20;
const MAX_LICENSE_BYTES: u64 = 64 << 10;
const MAX_SPLASH_BYTES: u64 = 1 << 20;

/// The colors by role, in the reference's order (`Ink`).
pub const INKS: [&str; 9] = ["background", "surface", "raised", "border", "text", "muted", "accent", "positive", "danger"];

/// How controls are drawn (theme format 3): `ThemeStyle`, the reference's defaults.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    pub buttons: ButtonStyle,
    pub edges: EdgeStyle,
    pub depth: DepthStyle,
    pub sliders: SliderStyle,
    pub tabs: TabStyle,
    pub marks: MarkStyle,
    pub sections: SectionStyle,
    pub rows: RowStyle,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            buttons: ButtonStyle::Pill,
            edges: EdgeStyle::Hairline,
            depth: DepthStyle::Soft,
            sliders: SliderStyle::Line,
            tabs: TabStyle::Segmented,
            marks: MarkStyle::Square,
            sections: SectionStyle::Separate,
            rows: RowStyle::Left,
        }
    }
}

/// Wires in the routing views (theme format 4): `ThemeWires`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wires {
    pub audio: Color,
    pub notes: Color,
    pub disabled: Color,
    pub draining: Color,
    pub unconnected: Color,
    pub control: Color,
}

impl Default for Wires {
    fn default() -> Wires {
        Wires { audio: rgb(0x8cc2ff), notes: rgb(0xc29cff), disabled: rgb(0x6b7480), draining: rgb(0xff9a45), unconnected: rgb(0xff6b63), control: rgb(0x6fe0a8) }
    }
}

/// The splash screen (theme format 7), each part optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Splash {
    pub image: String,
    pub background: Option<Color>,
    pub ink: Option<Color>,
}

/// Font files as paths relative to the theme's folder (empty only for the emergency theme).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontFiles {
    pub regular: String,
    pub bold: String,
    pub license: String,
}

/// A theme file as read: `ThemeData`, the reference's defaults where it has them.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeData {
    pub id: String,
    pub name: String,
    pub font: FontFiles,
    pub font_scale: f32,
    /// A scale on the paddings and gaps (0.6 to 1.6).
    pub spacing: f32,
    pub radius: f32,
    /// By `INKS`.
    pub colors: [Color; 9],
    pub warning: Option<Color>,
    pub focus: Option<Color>,
    pub style: Style,
    pub wires: Wires,
    pub splash: Splash,
}

fn rgb(v: u32) -> Color {
    Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The emergency theme (Theme.h's `ThemeData{}`), with no font files.
impl Default for ThemeData {
    fn default() -> ThemeData {
        ThemeData {
            id: "oscairn-dark".into(),
            name: "Oscairn Dark".into(),
            font: FontFiles::default(),
            font_scale: 1.,
            spacing: 1.,
            radius: 6.,
            colors: [0x15181d, 0x20252d, 0x2c333d, 0x526070, 0xedf1f5, 0xa3aebb, 0xffa45b, 0x5ad9a4, 0xff8d87].map(rgb),
            warning: None,
            focus: None,
            style: Style::default(),
            wires: Wires::default(),
            splash: Splash::default(),
        }
    }
}

type Refused = &'static str;

fn require(condition: bool, message: Refused) -> Result<(), Refused> {
    if condition {
        Ok(())
    } else {
        Err(message)
    }
}

fn fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), Refused> {
    require(object.keys().all(|k| allowed.contains(&k.as_str())), "Unknown theme field")
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 64 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `#RRGGBB`, as the reference's `hex`.
fn hex(v: Option<&Value>) -> Result<Color, Refused> {
    let text = v.and_then(Value::as_str).unwrap_or_default();
    require(text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit()), "Theme colors must be #RRGGBB")?;
    Ok(rgb(u32::from_str_radix(&text[1..], 16).unwrap_or_default()))
}

fn luminance(c: Color) -> f64 {
    let linear = |v: u8| {
        let v = f64::from(v) / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.red()) + 0.7152 * linear(c.green()) + 0.0722 * linear(c.blue())
}

fn contrast(x: Color, y: Color) -> f64 {
    let (a, b) = (luminance(x), luminance(y));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// JUCE's `Colour::getPerceivedBrightness`.
fn brightness(c: Color) -> f64 {
    let f = |v: u8| f64::from(v) / 255.;
    (f(c.red()).powi(2) * 0.241 + f(c.green()).powi(2) * 0.691 + f(c.blue()).powi(2) * 0.068).sqrt()
}

/// The reference's `darkTheme`: a background of perceived brightness under one half.
fn dark(background: Color) -> bool {
    brightness(background) < 0.5
}

impl ThemeData {
    /// `ThemeData::parse`: the same checks in the same order, the same words.
    pub fn parse(text: &str) -> Result<ThemeData, Refused> {
        require(text.len() as u64 <= MAX_THEME_BYTES, "Theme exceeds 16 KiB")?;
        let value: Value = serde_json::from_str(text).map_err(|_| "Invalid theme JSON")?;
        let object = value.as_object().ok_or("Theme must be an object")?;
        fields(object, &["version", "id", "name", "font", "fontScale", "spacing", "radius", "colors", "style", "wires", "splash"])?;
        require(object.get("version").is_some_and(|v| v.as_i64() == Some(7)), "Unsupported theme version")?;
        let string = |key: &str| object.get(key).and_then(Value::as_str);
        let id = string("id").unwrap_or_default().to_string();
        let name = string("name").unwrap_or_default().to_string();
        require(string("id").is_some() && valid_id(&id), "Theme ID must contain lowercase letters, digits or hyphens")?;
        require(string("name").is_some() && !name.is_empty() && name.chars().count() <= 80, "Theme needs a short name")?;
        let font = object.get("font").and_then(Value::as_object).ok_or("Theme needs its font files")?;
        fields(font, &["regular", "bold", "license"])?;
        let path = |key: &str, required: bool| -> Result<String, Refused> {
            let v = font.get(key).filter(|v| !v.is_null()); // JUCE reads a null as absent
            if !required && v.is_none() {
                return Ok(String::new());
            }
            Ok(v.and_then(Value::as_str).ok_or("Theme font paths must be strings")?.to_string())
        };
        let font = FontFiles { regular: path("regular", true)?, bold: path("bold", false)?, license: path("license", true)? };
        let number = |key: &str, fallback: f32, low: f64, high: f64| -> Result<f32, Refused> {
            let Some(v) = object.get(key) else { return Ok(fallback) };
            let n = v.as_f64().filter(|_| v.is_number()).ok_or("Theme metric must be a number")?;
            require(n.is_finite() && n >= low && n <= high, "Theme metric is out of range")?;
            Ok(n as f32)
        };
        let font_scale = number("fontScale", 1., 0.8, 1.4)?;
        let spacing = number("spacing", 1., 0.6, 1.6)?;
        let radius = number("radius", 6., 0., 16.)?;
        let colors = object.get("colors").and_then(Value::as_object).ok_or("Theme needs semantic colors")?;
        let mut allowed = INKS.to_vec();
        allowed.extend(["warning", "focus"]);
        fields(colors, &allowed)?;
        let optional = |key: &str| -> Result<Option<Color>, Refused> { colors.get(key).map(|v| hex(Some(v))).transpose() };
        let (warning, focus) = (optional("warning")?, optional("focus")?);
        let mut ink = [Color::default(); 9];
        for (i, key) in INKS.iter().enumerate() {
            ink[i] = hex(colors.get(*key))?;
        }
        let wires = object.get("wires").and_then(Value::as_object).ok_or("Theme needs its wire colors")?;
        fields(wires, &["audio", "notes", "disabled", "draining", "unconnected", "control"])?;
        let wires = Wires {
            audio: hex(wires.get("audio"))?,
            notes: hex(wires.get("notes"))?,
            disabled: hex(wires.get("disabled"))?,
            draining: hex(wires.get("draining"))?,
            unconnected: hex(wires.get("unconnected"))?,
            control: hex(wires.get("control"))?,
        };
        let style = object.get("style").and_then(Value::as_object).ok_or("Theme needs a style")?;
        fields(style, &["buttons", "edges", "depth", "sliders", "tabs", "marks", "sections", "rows"])?;
        let choice = |key: &str, options: &[&str]| -> Result<usize, Refused> {
            let v = style.get(key).and_then(Value::as_str);
            v.and_then(|v| options.iter().position(|o| *o == v)).ok_or("Theme style choice is not one of its options")
        };
        let style = Style {
            buttons: [ButtonStyle::Rounded, ButtonStyle::Pill][choice("buttons", &["rounded", "pill"])?],
            edges: [EdgeStyle::None, EdgeStyle::Hairline, EdgeStyle::Outlined][choice("edges", &["none", "hairline", "outlined"])?],
            depth: [DepthStyle::Flat, DepthStyle::Soft][choice("depth", &["flat", "soft"])?],
            sliders: [SliderStyle::Filled, SliderStyle::Line, SliderStyle::Cap][choice("sliders", &["filled", "line", "cap"])?],
            tabs: [TabStyle::Folder, TabStyle::Segmented][choice("tabs", &["folder", "segmented"])?],
            marks: [MarkStyle::Square, MarkStyle::Round][choice("marks", &["square", "round"])?],
            sections: [SectionStyle::Separate, SectionStyle::Joined][choice("sections", &["separate", "joined"])?],
            rows: [RowStyle::Left, RowStyle::Right, RowStyle::Dot, RowStyle::Fill][choice("rows", &["left", "right", "dot", "fill"])?],
        };
        for background in &ink[..3] {
            require(contrast(ink[4], *background) >= 4.5, "Theme text contrast must be at least 4.5:1")?;
        }
        let mut splash = Splash::default();
        if let Some(s) = object.get("splash") {
            let s = s.as_object().ok_or("Theme splash must be an object")?;
            fields(s, &["image", "background", "ink"])?;
            if let Some(image) = s.get("image") {
                let image = image.as_str().filter(|i| i.ends_with(".svg") || i.ends_with(".png")).ok_or("Theme splash image must be an .svg or .png file")?;
                splash.image = image.to_string();
            }
            if let Some(v) = s.get("background") {
                splash.background = Some(hex(Some(v))?);
            }
            if let Some(v) = s.get("ink") {
                splash.ink = Some(hex(Some(v))?);
            }
            let (ink_color, behind) = (splash.ink.unwrap_or(ink[4]), splash.background.unwrap_or(ink[0]));
            require(contrast(ink_color, behind) >= 4.5, "Theme splash text contrast must be at least 4.5:1")?;
        }
        Ok(ThemeData { id, name, font, font_scale, spacing, radius, colors: ink, warning, focus, style, wires, splash })
    }

    fn ink(&self, name: &str) -> Color {
        self.colors[INKS.iter().position(|n| *n == name).unwrap_or_default()]
    }
}

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

/// A theme read and checked: the data, its folder (none: built in, no font files) and the font
/// files' bytes. `refused` is why the theme asked for was not used (the reference's `error()`).
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub id: String,
    pub data: ThemeData,
    pub folder: Option<PathBuf>,
    pub regular: Option<Vec<u8>>,
    pub bold: Option<Vec<u8>>,
    pub refused: Option<String>,
}

/// A file named by a theme: a relative path inside its folder, no links, within a size limit.
fn inside(folder: &Path, relative: &str, limit: u64) -> Result<PathBuf, Refused> {
    require(
        !relative.is_empty() && relative.chars().count() <= 128 && relative.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/".contains(&b)),
        "Theme file paths must be relative names inside the theme",
    )?;
    require(relative.split('/').all(|p| !p.is_empty() && p != "." && p != ".."), "Theme file paths must stay inside the theme")?;
    let file = folder.join(relative);
    let mut at = file.as_path();
    while at != folder {
        require(!std::fs::symlink_metadata(at).is_ok_and(|m| m.file_type().is_symlink()), "Theme files must not be links")?;
        match at.parent() {
            Some(p) if p != at => at = p,
            _ => break,
        }
    }
    let meta = std::fs::metadata(&file).ok().filter(|m| m.is_file()).ok_or("Theme file is missing")?;
    require(meta.len() <= limit, "Theme file is too large")?;
    Ok(file)
}

/// The reference's `fontContainer`: signature, a table directory inside the file and the tables
/// a usable font needs. Not a full font validator.
fn font_container(b: &[u8]) -> bool {
    let u32_at = |at: usize| u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]);
    if b.len() < 12 {
        return false;
    }
    if !matches!(u32_at(0), 0x0001_0000 | 0x7472_7565 | 0x4f54_544f) {
        return false;
    }
    let tables = usize::from(b[4]) << 8 | usize::from(b[5]);
    if !(1..=64).contains(&tables) || 12 + 16 * tables > b.len() {
        return false;
    }
    let mut tags: Vec<&[u8]> = Vec::new();
    for i in 0..tables {
        let at = 12 + 16 * i;
        if u64::from(u32_at(at + 8)) + u64::from(u32_at(at + 12)) > b.len() as u64 {
            return false;
        }
        tags.push(&b[at..at + 4]);
    }
    let has = |t: &[u8]| tags.contains(&t);
    ["cmap", "head", "hhea", "hmtx", "maxp", "name"].iter().all(|t| has(t.as_bytes())) && ((has(b"glyf") && has(b"loca")) || has(b"CFF ") || has(b"CFF2"))
}

fn typeface(file: &Path) -> Result<Vec<u8>, Refused> {
    let ext = file.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    require(matches!(ext.as_deref(), Some("ttf" | "otf")), "Theme fonts must be .ttf or .otf files")?;
    let data = std::fs::read(file).map_err(|_| "Cannot read theme font")?;
    require(font_container(&data), "Theme font is not a TrueType or OpenType font")?;
    Ok(data)
}

/// `Theme::load`: a theme folder, checked as the reference checks it.
pub fn load_folder(folder: &Path) -> Result<Loaded, Refused> {
    let file = inside(folder, "theme.json", MAX_THEME_BYTES)?;
    let text = String::from_utf8_lossy(&std::fs::read(file).map_err(|_| "Theme file is missing")?).into_owned();
    let data = ThemeData::parse(&text)?;
    require(folder.file_name().and_then(|n| n.to_str()) == Some(data.id.as_str()), "Theme folder must be named after its ID")?;
    let regular = typeface(&inside(folder, &data.font.regular, MAX_FONT_BYTES)?)?;
    let bold = if data.font.bold.is_empty() { None } else { Some(typeface(&inside(folder, &data.font.bold, MAX_FONT_BYTES)?)?) };
    inside(folder, &data.font.license, MAX_LICENSE_BYTES)?;
    if !data.splash.image.is_empty() {
        let image = inside(folder, &data.splash.image, MAX_SPLASH_BYTES)?;
        // The reference decodes it; here its form is checked (a PNG's signature, an SVG's root), not its pixels.
        let bytes = std::fs::read(image).map_err(|_| "Cannot read theme splash image")?;
        let readable = if data.splash.image.ends_with(".png") { bytes.starts_with(b"\x89PNG\r\n\x1a\n") } else { String::from_utf8_lossy(&bytes).contains("<svg") };
        require(readable, "Theme splash image cannot be read")?;
    }
    Ok(Loaded { id: data.id.clone(), data, folder: Some(folder.to_path_buf()), regular: Some(regular), bold, refused: None })
}

/// The theme `id` names: a built-in folder's if there is one (built-in IDs win, and a broken one is
/// refused, not skipped), else an installed one's; with no built-in folder, the themes this
/// program was built with.
fn find(places: &Places, id: &str) -> Result<Loaded, Refused> {
    require(valid_id(id), "Invalid theme ID")?;
    match &places.built_in {
        Some(dir) if dir.join(id).is_dir() => return load_folder(&dir.join(id)),
        Some(_) => {}
        None => {
            if let Some((_, text)) = THEMES.iter().find(|(t, _)| *t == id) {
                let data = ThemeData::parse(text)?;
                return Ok(Loaded { id: data.id.clone(), data, folder: None, regular: None, bold: None, refused: None });
            }
        }
    }
    load_folder(&places.installed.as_deref().unwrap_or(Path::new("/nonexistent")).join(id))
}

fn exists(places: &Places, id: &str) -> bool {
    let built_in = match &places.built_in {
        Some(dir) => dir.join(id).is_dir(),
        None => THEMES.iter().any(|(t, _)| *t == id),
    };
    built_in || places.installed.as_ref().is_some_and(|d| d.join(id).is_dir())
}

/// `Theme::defaultId`: the screen's theme if it exists, else `themes/default`'s, else Oscairn Dark.
fn default_id(places: &Places, screen: &str) -> String {
    if valid_id(screen) && exists(places, screen) {
        return screen.into();
    }
    let id = match &places.built_in {
        Some(dir) => std::fs::read_to_string(dir.join("default")).unwrap_or_default().trim().to_string(),
        None => DEFAULT_THEME.to_string(),
    };
    let built_in = match &places.built_in {
        Some(dir) => dir.join(&id).is_dir(),
        None => THEMES.iter().any(|(t, _)| *t == id),
    };
    if valid_id(&id) && built_in {
        id
    } else {
        "oscairn-dark".into()
    }
}

/// `Theme::reload`'s choice: the user's (empty: the default), and if that is refused the default,
/// and if that is too the emergency theme. `screen` is the profile's theme (OSCAIRN_SCREEN_THEME).
pub fn chosen(places: &Places, choice: &str, screen: &str) -> Loaded {
    let default = default_id(places, screen);
    let wanted = if choice.is_empty() { default.as_str() } else { choice };
    let refused = match find(places, wanted) {
        Ok(loaded) => return loaded,
        Err(why) => why,
    };
    match find(places, &default) {
        Ok(loaded) => Loaded { refused: Some(refused.into()), ..loaded },
        Err(_) => Loaded { id: "oscairn-dark".into(), data: ThemeData::default(), folder: None, regular: None, bold: None, refused: Some(refused.into()) },
    }
}

/// A font's bytes registered for Slint's renderers; its family's name.
#[cfg(not(test))]
fn register_font(data: &[u8]) -> Option<String> {
    use slint::fontique_011::fontique;
    let mut collection = slint::fontique_011::shared_collection();
    let fonts = collection.register_fonts(fontique::Blob::new(std::sync::Arc::new(data.to_vec())), None);
    let family = fonts.first()?.0;
    collection.family_name(family).map(String::from)
}

// Headless tests keep the built-in Inter: the renderer's fonts are shared by every test thread.
#[cfg(test)]
fn register_font(_: &[u8]) -> Option<String> {
    None
}

/// `drawListRow`'s tinted row: the background blended toward the row's color, by .42 (.3 on an odd
/// row) less .05 at a time while text on it would be under 4.5:1.
fn row_fill(color: Color, base: Color, text: Color, odd: bool) -> Color {
    let blend = |amount: f32| {
        let mix = |a: u8, b: u8| (f32::from(a) * (1. - amount) + f32::from(b) * amount).round() as u8;
        Color::from_rgb_u8(mix(base.red(), color.red()), mix(base.green(), color.green()), mix(base.blue(), color.blue()))
    };
    let mut amount: f32 = if odd { 0.3 } else { 0.42 };
    while amount > 0.05 && contrast(text, blend(amount)) < 4.5 {
        amount -= 0.05;
    }
    blend(amount)
}

/// Gives a window its theme's computed pieces; before the first `apply`.
pub fn attach(window: &Shell) {
    window.global::<Theme>().on_row_fill(row_fill);
}

/// JUCE's `Colour::contrasting`: black on a bright color, white on a dark one.
fn contrasting(c: Color) -> Color {
    if brightness(c) >= 0.5 {
        Color::from_rgb_u8(0, 0, 0)
    } else {
        Color::from_rgb_u8(255, 255, 255)
    }
}

/// What a screen laying out in pixels needs: the gap between sections, `separate` times the theme's
/// spacing unless they join.
pub fn section_gap(window: &Shell, separate: f32) -> f32 {
    let theme = window.global::<Theme>();
    if theme.get_sections() == SectionStyle::Joined {
        0.
    } else {
        separate * theme.get_spacing()
    }
}

/// Puts the theme in the window's `Theme` global. Every property is set, so a switch leaves
/// nothing of the theme before.
pub fn apply(window: &Shell, theme: &Loaded) {
    let g = window.global::<Theme>();
    let d = &theme.data;
    let c = |name: &str| d.ink(name);
    g.set_background(c("background"));
    g.set_surface(c("surface"));
    g.set_raised(c("raised"));
    g.set_border(c("border"));
    g.set_text(c("text"));
    g.set_muted(c("muted"));
    g.set_accent(c("accent"));
    g.set_positive(c("positive"));
    g.set_danger(c("danger"));
    g.set_warning(d.warning.unwrap_or(rgb(0xe8c547))); // the JUCE meters' yellow
    g.set_focus(d.focus.unwrap_or(c("accent")));
    g.set_dark(dark(c("background")));
    g.set_on_accent(contrasting(c("accent")));
    let (text, surface) = (c("text"), c("surface"));
    let brighter = |a: Color, b: Color| brightness(a) > brightness(b);
    g.set_key_light(if brighter(text, surface) { text } else { surface });
    g.set_key_dark(if brighter(text, surface) { surface } else { text });
    g.set_radius(d.radius);
    g.set_font_scale(d.font_scale);
    g.set_spacing(d.spacing);
    let s = d.style;
    g.set_buttons(s.buttons);
    g.set_edges(s.edges);
    g.set_depth(s.depth);
    g.set_sliders(s.sliders);
    g.set_tabs(s.tabs);
    g.set_marks(s.marks);
    g.set_sections(s.sections);
    g.set_rows(s.rows);
    g.set_wire_audio(d.wires.audio);
    g.set_wire_notes(d.wires.notes);
    g.set_wire_disabled(d.wires.disabled);
    g.set_wire_draining(d.wires.draining);
    g.set_wire_unconnected(d.wires.unconnected);
    g.set_wire_control(d.wires.control);
    g.set_splash_background(d.splash.background.unwrap_or(c("background")));
    g.set_splash_ink(d.splash.ink.unwrap_or(c("text")));
    let image = theme.folder.as_deref().filter(|_| !d.splash.image.is_empty()).and_then(|f| slint::Image::load_from_path(&f.join(&d.splash.image)).ok());
    g.set_has_splash_image(image.is_some());
    g.set_splash_image(image.unwrap_or_default());
    let family = theme.regular.as_deref().and_then(register_font);
    if let Some(bold) = theme.bold.as_deref() {
        register_font(bold);
    }
    g.set_font_family(family.unwrap_or("Inter".into()).into());
    // A screen laid out in pixels (the grid's gap is the theme's) lays out again.
    window.global::<crate::GridState>().invoke_resized();
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
mod tests;
