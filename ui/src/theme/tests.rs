// SPDX-License-Identifier: AGPL-3.0-or-later
//! The theme loader against the reference's rules (hosts/standalone/Theme.cpp `ThemeData::parse`,
//! `Theme::load`, `Theme::reload`): the built-in theme files read from the repository itself, the
//! reference's defaults for what a file may leave out, and a refusal, in the reference's words, for
//! everything else.

use super::*;
use serde_json::{json, Value};

fn themes_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hosts/standalone/themes")
}

fn built_in() -> Places {
    Places { built_in: Some(themes_dir()), installed: None }
}

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("oscairn-ui-themes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn dark_doc() -> Value {
    serde_json::from_str(&std::fs::read_to_string(themes_dir().join("oscairn-dark/theme.json")).unwrap()).unwrap()
}

fn parse(doc: &Value) -> Result<ThemeData, Refused> {
    ThemeData::parse(&doc.to_string())
}

fn c(v: u32) -> Color {
    rgb(v)
}

// Every built-in theme folder loads as the reference loads it (the folder named after its id, its
// fonts real fonts, its license there) and has the values its file has: spot-checked per theme.
#[test]
fn every_built_in_theme_loads_with_its_file_values() {
    let places = built_in();
    let ids: Vec<String> = std::fs::read_dir(themes_dir()).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(ids.len(), 4, "the built-in themes are four: {ids:?}");
    for id in &ids {
        let loaded = find(&places, id).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(&loaded.id, id);
        assert!(loaded.regular.is_some() && loaded.bold.is_some(), "{id}: both font files read");
        assert_eq!(loaded.data.font, FontFiles { regular: "fonts/Inter-Regular.ttf".into(), bold: "fonts/Inter-Bold.ttf".into(), license: "fonts/OFL.txt".into() });
        // The embedded copy (what a build without the folder uses) is the same file.
        let embedded = THEMES.iter().find(|(t, _)| t == id).map(|(_, text)| ThemeData::parse(text).unwrap()).unwrap();
        assert_eq!(embedded, loaded.data, "{id}: the built-in text and the folder agree");
    }
    let d = find(&places, "oscairn-dark").unwrap().data;
    assert_eq!((d.name.as_str(), d.font_scale, d.spacing, d.radius), ("Oscairn Dark", 1., 1., 6.));
    assert_eq!(d.colors, [0x111317, 0x1d2127, 0x2d333b, 0x4a535e, 0xf6f8fa, 0xb4bdc8, 0xff9a45, 0x5ad9a4, 0xff6b63].map(c));
    assert_eq!(d.style, Style::default(), "the dark theme's style is the reference's default: pill, hairline, soft, line, segmented, square, separate, left");
    assert_eq!((d.wires.audio, d.wires.control), (c(0x8cc2ff), c(0x6fe0a8)));
    assert_eq!((d.warning, d.focus, d.splash.image.as_str()), (None, None, ""));

    let b = find(&places, "oscairn-block").unwrap().data;
    assert_eq!(b.radius, 0.);
    assert_eq!(b.colors[3], c(0x3a414b));
    assert_eq!(
        b.style,
        Style {
            buttons: ButtonStyle::Rounded,
            edges: EdgeStyle::Outlined,
            depth: DepthStyle::Flat,
            sliders: SliderStyle::Cap,
            tabs: TabStyle::Folder,
            tab_placement: crate::TabPlacement::Auto,
            marks: MarkStyle::Square,
            sections: SectionStyle::Joined,
            rows: RowStyle::Fill
        }
    );

    let k = find(&places, "oscairn-classic").unwrap().data;
    assert_eq!((k.radius, k.colors[0], k.colors[3], k.colors[8]), (6., c(0x15181d), c(0x526070), c(0xff8d87)));
    assert_eq!((k.style.sliders, k.style.edges, k.style.depth, k.style.tabs, k.style.buttons), (SliderStyle::Filled, EdgeStyle::Outlined, DepthStyle::Flat, TabStyle::Folder, ButtonStyle::Rounded));
    assert_eq!((k.wires.disabled, k.wires.draining), (c(0x6b7685), c(0xffa45b)));

    let l = find(&places, "oscairn-light").unwrap().data;
    assert_eq!((l.colors[0], l.colors[1], l.colors[4], l.colors[6]), (c(0xeef1f5), c(0xffffff), c(0x10161e), c(0xb85a0a)));
    assert_eq!((l.wires.audio, l.wires.notes), (c(0x2f6fd0), c(0x7b4bd6)));
    assert!(!dark(l.colors[0]) && dark(d.colors[0]), "darkTheme: the light theme is not, the dark one is");
}

// What the reference lets a file leave out has its default: `fontScale` 1, `spacing` 1, `radius` 6, no
// `warning` or `focus`, no bold font, no splash. Everything else is required.
#[test]
fn what_a_file_may_leave_out_has_the_references_default() {
    let mut doc = dark_doc();
    let o = doc.as_object_mut().unwrap();
    for key in ["fontScale", "spacing", "radius"] {
        o.remove(key);
    }
    o["font"].as_object_mut().unwrap().remove("bold");
    let t = parse(&doc).unwrap();
    assert_eq!((t.font_scale, t.spacing, t.radius, t.font.bold.as_str(), t.warning, t.focus, &t.splash), (1., 1., 6., "", None, None, &Splash::default()));
    // A bold font given as null reads as absent, as the reference's JSON reads it.
    doc["font"]["bold"] = Value::Null;
    assert_eq!(parse(&doc).unwrap().font.bold, "");
    // And given, they are taken.
    doc["fontScale"] = json!(1.2);
    doc["spacing"] = json!(0.75);
    doc["radius"] = json!(11.5);
    doc["colors"]["warning"] = json!("#112233");
    doc["colors"]["focus"] = json!("#AABBCC");
    doc["splash"] = json!({"image": "splash.png", "background": "#000000", "ink": "#FFFFFF"});
    let t = parse(&doc).unwrap();
    assert_eq!((t.font_scale, t.spacing, t.radius, t.warning, t.focus), (1.2, 0.75, 11.5, Some(c(0x112233)), Some(c(0xaabbcc))));
    assert_eq!((t.splash.image.as_str(), t.splash.background, t.splash.ink), ("splash.png", Some(c(0)), Some(c(0xffffff))));
    // The splash may give one of its colors; the other is the theme's (contrast checked with it).
    doc["splash"] = json!({"ink": "#FFFFFF"});
    assert_eq!(parse(&doc).unwrap().splash.ink, Some(c(0xffffff)));
    // The range ends are in the range (docs/theme.schema.json: minimum and maximum): radius 0 and 16, font scale 0.8 and 1.4, spacing 0.6 and 1.6.
    for (key, ok, bad) in [("radius", [0., 16.], [-0.1, 16.1]), ("fontScale", [0.8, 1.4], [0.79, 1.41]), ("spacing", [0.6, 1.6], [0.59, 1.61])] {
        let mut doc = dark_doc();
        for v in ok {
            doc[key] = json!(v);
            assert!(parse(&doc).is_ok(), "{key} {v}");
        }
        for v in bad {
            doc[key] = json!(v);
            assert_eq!(parse(&doc), Err("Theme metric is out of range"), "{key} {v}");
        }
    }
}

// A bad file is refused whole, with the reference's words, in the reference's order.
#[test]
fn a_bad_file_is_refused_as_the_reference_refuses_it() {
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut doc = dark_doc();
        f(&mut doc);
        parse(&doc)
    };
    assert_eq!(ThemeData::parse("{ not json"), Err("Invalid theme JSON"));
    assert_eq!(ThemeData::parse("[1]"), Err("Theme must be an object"));
    assert_eq!(ThemeData::parse(&" ".repeat(16385)), Err("Theme exceeds 16 KiB"));
    assert_eq!(edit(&|d| d["extra"] = json!(1)), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["colors"]["extra"] = json!("#000000")), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["wires"]["extra"] = json!("#000000")), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["style"]["extra"] = json!("flat")), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["font"]["extra"] = json!("x.ttf")), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["version"] = json!(6)), Err("Unsupported theme version"));
    assert_eq!(edit(&|d| d["version"] = json!("7")), Err("Unsupported theme version"));
    assert_eq!(ThemeData::parse(&dark_doc().to_string().replace("\"version\":7", "\"version\":7.0")), Err("Unsupported theme version"), "7.0 is a double to the reference's isInt");
    assert_eq!(edit(&|d| d.as_object_mut().unwrap().remove("version").map(|_| ()).unwrap_or(())), Err("Unsupported theme version"));
    for id in [json!("Dark"), json!(""), json!("a_b"), json!("x".repeat(65)), json!(7)] {
        assert_eq!(edit(&|d| d["id"] = id.clone()), Err("Theme ID must contain lowercase letters, digits or hyphens"), "{id}");
    }
    for name in [json!(""), json!("x".repeat(81)), json!(3)] {
        assert_eq!(edit(&|d| d["name"] = name.clone()), Err("Theme needs a short name"), "{name}");
    }
    assert_eq!(edit(&|d| d["name"] = json!("x".repeat(80))).map(|t| t.name.len()), Ok(80));
    assert_eq!(edit(&|d| {
        d.as_object_mut().unwrap().remove("font");
    }), Err("Theme needs its font files"));
    assert_eq!(edit(&|d| {
        d["font"].as_object_mut().unwrap().remove("regular");
    }), Err("Theme font paths must be strings"));
    assert_eq!(edit(&|d| {
        d["font"].as_object_mut().unwrap().remove("license");
    }), Err("Theme font paths must be strings"));
    assert_eq!(edit(&|d| d["font"]["bold"] = json!(4)), Err("Theme font paths must be strings"));
    for v in [json!("1"), json!(null), json!(true)] {
        assert_eq!(edit(&|d| d["fontScale"] = v.clone()), Err("Theme metric must be a number"), "{v}");
        assert_eq!(edit(&|d| d["spacing"] = v.clone()), Err("Theme metric must be a number"), "{v}");
        assert_eq!(edit(&|d| d["radius"] = v.clone()), Err("Theme metric must be a number"), "{v}");
    }
    assert_eq!(edit(&|d| {
        d.as_object_mut().unwrap().remove("colors");
    }), Err("Theme needs semantic colors"));
    for key in INKS {
        assert_eq!(edit(&|d| {
            d["colors"].as_object_mut().unwrap().remove(key);
        }), Err("Theme colors must be #RRGGBB"), "{key} is required");
    }
    for bad in ["#FFF", "red", "#12345G", "FF0000", "#1234567", ""] {
        assert_eq!(edit(&|d| d["colors"]["accent"] = json!(bad)), Err("Theme colors must be #RRGGBB"), "{bad:?}");
        assert_eq!(edit(&|d| d["colors"]["warning"] = json!(bad)), Err("Theme colors must be #RRGGBB"), "{bad:?}");
        assert_eq!(edit(&|d| d["colors"]["focus"] = json!(bad)), Err("Theme colors must be #RRGGBB"), "{bad:?}");
        assert_eq!(edit(&|d| d["wires"]["audio"] = json!(bad)), Err("Theme colors must be #RRGGBB"), "{bad:?}");
    }
    assert_eq!(edit(&|d| d["colors"]["accent"] = json!(7)), Err("Theme colors must be #RRGGBB"));
    assert_eq!(edit(&|d| d["colors"]["accent"] = json!("#aabbcc")).map(|t| t.colors[6]), Ok(c(0xaabbcc)), "lowercase hex is taken");
    assert_eq!(edit(&|d| {
        d.as_object_mut().unwrap().remove("wires");
    }), Err("Theme needs its wire colors"));
    for key in ["audio", "notes", "disabled", "draining", "unconnected", "control"] {
        assert_eq!(edit(&|d| {
            d["wires"].as_object_mut().unwrap().remove(key);
        }), Err("Theme colors must be #RRGGBB"), "wires.{key} is required");
    }
    assert_eq!(edit(&|d| {
        d.as_object_mut().unwrap().remove("style");
    }), Err("Theme needs a style"));
    // tabPlacement is optional (absent: auto) and, when there, one of auto, top and side.
    assert_eq!(edit(&|d| {
        d["style"].as_object_mut().unwrap().remove("tabPlacement");
    }).map(|t| t.style.tab_placement), Ok(crate::TabPlacement::Auto));
    for (word, want) in [("auto", crate::TabPlacement::Auto), ("top", crate::TabPlacement::Top), ("side", crate::TabPlacement::Side)] {
        assert_eq!(edit(&|d| d["style"]["tabPlacement"] = json!(word)).map(|t| t.style.tab_placement), Ok(want), "{word}");
    }
    for bad in [json!("left"), json!("Side"), json!(""), json!(3), json!(null), json!(true), json!(["side"])] {
        assert_eq!(edit(&|d| d["style"]["tabPlacement"] = bad.clone()), Err("Theme style choice is not one of its options"), "{bad}");
    }
    for (key, bad) in [("buttons", "square"), ("edges", "thin"), ("depth", "hard"), ("sliders", "round"), ("tabs", "pills"), ("marks", "oval"), ("sections", "apart"), ("rows", "top")] {
        assert_eq!(edit(&|d| d["style"][key] = json!(bad)), Err("Theme style choice is not one of its options"), "{key}");
        assert_eq!(edit(&|d| d["style"][key] = json!(3)), Err("Theme style choice is not one of its options"), "{key}");
        assert_eq!(edit(&|d| {
            d["style"].as_object_mut().unwrap().remove(key);
        }), Err("Theme style choice is not one of its options"), "style.{key} is required");
    }
    // Text must read on the three backgrounds, 4.5:1; the check is the reference's luminance ratio.
    assert_eq!(edit(&|d| d["colors"]["text"] = json!("#4A535E")), Err("Theme text contrast must be at least 4.5:1"));
    assert_eq!(edit(&|d| d["colors"]["raised"] = json!("#F6F8FA")), Err("Theme text contrast must be at least 4.5:1"));
    assert_eq!(edit(&|d| d["splash"] = json!(3)), Err("Theme splash must be an object"));
    assert_eq!(edit(&|d| d["splash"] = json!({"x": 1})), Err("Unknown theme field"));
    assert_eq!(edit(&|d| d["splash"] = json!({"image": "s.gif"})), Err("Theme splash image must be an .svg or .png file"));
    assert_eq!(edit(&|d| d["splash"] = json!({"image": 3})), Err("Theme splash image must be an .svg or .png file"));
    assert_eq!(edit(&|d| d["splash"] = json!({"ink": "#222222"})), Err("Theme splash text contrast must be at least 4.5:1"));
    assert_eq!(edit(&|d| d["splash"] = json!({"background": "#FFFFFF"})), Err("Theme splash text contrast must be at least 4.5:1"));
    assert_eq!(edit(&|d| d["splash"] = json!({"background": "white"})), Err("Theme colors must be #RRGGBB"));
}

/// A copy of a built-in theme's folder under a new id's name, for a test to damage.
fn copy_of(name: &str, id: &str) -> PathBuf {
    let folder = temp(name).join(id);
    std::fs::create_dir_all(folder.join("fonts")).unwrap();
    for f in ["theme.json", "fonts/Inter-Regular.ttf", "fonts/Inter-Bold.ttf", "fonts/OFL.txt"] {
        std::fs::copy(themes_dir().join("oscairn-dark").join(f), folder.join(f)).unwrap();
    }
    let mut doc = dark_doc();
    doc["id"] = json!(id);
    std::fs::write(folder.join("theme.json"), doc.to_string()).unwrap();
    folder
}

// The folder rules of `Theme::load` and `inside`: the folder is named after the id, the files are
// there, within their limits, real fonts, not links, and named inside the folder.
#[test]
fn a_folder_is_refused_as_the_reference_refuses_it() {
    let good = copy_of("folder-good", "mine");
    assert_eq!(load_folder(&good).map(|l| l.id), Ok("mine".to_string()));

    let wrong = copy_of("folder-name", "mine");
    let renamed = wrong.parent().unwrap().join("other");
    std::fs::rename(&wrong, &renamed).unwrap();
    assert_eq!(load_folder(&renamed).err(), Some("Theme folder must be named after its ID"));

    let no_font = copy_of("folder-nofont", "mine");
    std::fs::remove_file(no_font.join("fonts/Inter-Regular.ttf")).unwrap();
    assert_eq!(load_folder(&no_font).err(), Some("Theme file is missing"));

    let no_license = copy_of("folder-nolicense", "mine");
    std::fs::remove_file(no_license.join("fonts/OFL.txt")).unwrap();
    assert_eq!(load_folder(&no_license).err(), Some("Theme file is missing"));

    let not_a_font = copy_of("folder-notfont", "mine");
    std::fs::write(not_a_font.join("fonts/Inter-Regular.ttf"), b"this is not a font at all, just text").unwrap();
    assert_eq!(load_folder(&not_a_font).err(), Some("Theme font is not a TrueType or OpenType font"));

    let truncated = copy_of("folder-truncated", "mine");
    let bytes = std::fs::read(truncated.join("fonts/Inter-Bold.ttf")).unwrap();
    std::fs::write(truncated.join("fonts/Inter-Bold.ttf"), &bytes[..bytes.len() / 2]).unwrap();
    assert_eq!(load_folder(&truncated).err(), Some("Theme font is not a TrueType or OpenType font"), "a table past the end of the file");

    let wrong_ext = copy_of("folder-ext", "mine");
    std::fs::rename(wrong_ext.join("fonts/OFL.txt"), wrong_ext.join("fonts/font.woff")).unwrap();
    let mut doc = dark_doc();
    doc["id"] = json!("mine");
    doc["font"]["regular"] = json!("fonts/font.woff");
    std::fs::write(wrong_ext.join("theme.json"), doc.to_string()).unwrap();
    assert_eq!(load_folder(&wrong_ext).err(), Some("Theme fonts must be .ttf or .otf files"));

    let escaping = copy_of("folder-escape", "mine");
    for path in ["../mine/fonts/Inter-Regular.ttf", "/etc/passwd", "fonts//Inter-Regular.ttf", "fonts/./Inter-Regular.ttf", "fonts/Inter Regular.ttf"] {
        let mut doc = dark_doc();
        doc["id"] = json!("mine");
        doc["font"]["regular"] = json!(path);
        std::fs::write(escaping.join("theme.json"), doc.to_string()).unwrap();
        let why = load_folder(&escaping).err();
        assert!(matches!(why, Some("Theme file paths must stay inside the theme" | "Theme file paths must be relative names inside the theme")), "{path}: {why:?}");
    }

    let large = copy_of("folder-large", "mine");
    std::fs::write(large.join("fonts/OFL.txt"), vec![b'x'; (64 << 10) + 1]).unwrap();
    assert_eq!(load_folder(&large).err(), Some("Theme file is too large"));

    #[cfg(unix)]
    {
        let linked = copy_of("folder-link", "mine");
        std::fs::remove_file(linked.join("fonts/OFL.txt")).unwrap();
        std::os::unix::fs::symlink(themes_dir().join("oscairn-dark/fonts/OFL.txt"), linked.join("fonts/OFL.txt")).unwrap();
        assert_eq!(load_folder(&linked).err(), Some("Theme files must not be links"));
    }

    let splash = copy_of("folder-splash", "mine");
    let mut doc = dark_doc();
    doc["id"] = json!("mine");
    doc["splash"] = json!({"image": "splash.png"});
    std::fs::write(splash.join("theme.json"), doc.to_string()).unwrap();
    assert_eq!(load_folder(&splash).err(), Some("Theme file is missing"));
    std::fs::write(splash.join("splash.png"), b"not a png").unwrap();
    assert_eq!(load_folder(&splash).err(), Some("Theme splash image cannot be read"));
    std::fs::write(splash.join("splash.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
    assert_eq!(load_folder(&splash).map(|l| l.data.splash.image), Ok("splash.png".to_string()));
}

// The choice, as `reload` and `defaultId` make it: the user's, else the screen's, else
// `themes/default`, else Oscairn Dark; a refused one shows the default, and says why.
#[test]
fn chosen_as_the_juce_app_chooses() {
    let places = built_in();
    assert_eq!(chosen(&places, "", "").id, "oscairn-dark", "themes/default names it");
    assert_eq!(chosen(&places, "", "oscairn-block").id, "oscairn-block", "the screen's theme while none is chosen");
    assert_eq!(chosen(&places, "oscairn-light", "oscairn-block").id, "oscairn-light", "the user's over the screen's");
    assert_eq!(chosen(&places, "", "no-such-theme").id, "oscairn-dark", "a screen theme that does not exist");
    let bad = chosen(&places, "Not An Id", "oscairn-block");
    assert_eq!((bad.id.as_str(), bad.refused.as_deref()), ("oscairn-block", Some("Invalid theme ID")), "refused: the default, which is the screen's");
    assert_eq!(chosen(&places, "../etc", "").refused.as_deref(), Some("Invalid theme ID"));
    assert_eq!(chosen(&places, "oscairn-light", "").refused, None);
    // With no built-in folder the themes this program was built with stand in.
    let embedded = Places::default();
    assert_eq!(chosen(&embedded, "", "oscairn-block").id, "oscairn-block");
    assert_eq!(chosen(&embedded, "oscairn-light", "").id, "oscairn-light");
    assert_eq!(chosen(&embedded, "Not An Id", "").id, DEFAULT_THEME);
}

// An installed theme is read from the configuration folder; a refused one (here a partial file: a
// color missing) is not merged over the default but replaced by it; a built-in id wins over an
// installed folder of the same name; the last resort is the emergency theme.
#[test]
fn installed_themes_a_partial_one_and_the_last_resort() {
    let installed = temp("installed");
    let mine = installed.join("mine");
    std::fs::create_dir_all(mine.join("fonts")).unwrap();
    for f in ["fonts/Inter-Regular.ttf", "fonts/Inter-Bold.ttf", "fonts/OFL.txt"] {
        std::fs::copy(themes_dir().join("oscairn-dark").join(f), mine.join(f)).unwrap();
    }
    let mut doc = dark_doc();
    doc["id"] = json!("mine");
    doc["colors"]["accent"] = json!("#00FF00");
    std::fs::write(mine.join("theme.json"), doc.to_string()).unwrap();
    let places = Places { built_in: Some(themes_dir()), installed: Some(installed.clone()) };
    let t = chosen(&places, "mine", "");
    assert_eq!((t.id.as_str(), t.folder.as_deref(), t.data.colors[6]), ("mine", Some(mine.as_path()), c(0x00ff00)));
    assert_eq!(t.data.colors[0], c(0x111317));

    // A partial file: a key the reference requires is gone. Refused whole, the default shows.
    doc["colors"].as_object_mut().unwrap().remove("text");
    std::fs::write(mine.join("theme.json"), doc.to_string()).unwrap();
    let t = chosen(&places, "mine", "");
    assert_eq!((t.id.as_str(), t.refused.as_deref()), ("oscairn-dark", Some("Theme colors must be #RRGGBB")));
    std::fs::write(mine.join("theme.json"), "{ not json").unwrap();
    assert_eq!(chosen(&places, "mine", "").refused.as_deref(), Some("Invalid theme JSON"));

    // Built-in ids win: an installed folder named like a built-in theme is not read.
    let same = installed.join("oscairn-light");
    std::fs::create_dir_all(&same).unwrap();
    std::fs::write(same.join("theme.json"), "{}").unwrap();
    let t = chosen(&places, "oscairn-light", "");
    assert_eq!((t.id.as_str(), t.refused), ("oscairn-light", None));

    // No default theme and a refused choice: the emergency theme, without fonts.
    let empty = temp("empty");
    std::fs::write(empty.join("default"), "oscairn-dark").unwrap();
    let t = chosen(&Places { built_in: Some(empty), installed: None }, "mine", "");
    assert_eq!((t.id.as_str(), &t.data, t.regular.as_deref()), ("oscairn-dark", &ThemeData::default(), None));
    assert_eq!(ThemeData::default().colors[0], c(0x15181d), "Theme.h's emergency colors");
    let _ = std::fs::remove_dir_all(&installed);
}

// The row color blended into a list row: the reference's amounts (.42, .3 on an odd row), lowered
// while text on it would be under 4.5:1.
#[test]
fn a_tinted_row_keeps_the_text_readable() {
    let (dark_base, text) = (c(0x1d2127), c(0xf6f8fa));
    let bright = c(0xffff00);
    let filled = row_fill(bright, dark_base, text, false);
    assert!(contrast(text, filled) >= 4.5, "light text on a yellow-tinted dark row");
    assert_ne!(filled, dark_base, "but tinted");
    let green = row_fill(c(0x00aa00), dark_base, text, false);
    assert_eq!(green, Color::from_rgb_u8(((0x1d as f32) * 0.58 + 0.0).round() as u8, ((0x21 as f32) * 0.58 + 0xaa as f32 * 0.42).round() as u8, ((0x27 as f32) * 0.58).round() as u8), "where text still reads the blend is the full .42");
    let odd = row_fill(c(0x00aa00), dark_base, text, true);
    assert_eq!(odd.green(), ((0x21 as f32) * 0.7 + 0xaa as f32 * 0.3).round() as u8, ".3 on an odd row");
}

// `valid_id` (theme ids are lowercase ASCII letters, digits and hyphens, up to 64): the empty id,
// a unicode one (multi-byte), one of exactly 64 characters, and 65, and a trailing hyphen.
#[test]
fn valid_id_takes_only_lowercase_ascii_names() {
    for ok in ["a", "oscairn-dark", "123", "a-b-9", "x".repeat(64).as_str()] {
        assert!(valid_id(ok), "{ok:?}");
    }
    for bad in ["", "Dark", "a_b", "a b", "é", "å", "x".repeat(65).as_str()] {
        assert!(!valid_id(bad), "{bad:?}");
    }
}

// `ThemeData::ink`: the color named by its INKS slot, and, for an unknown name, the first (the
// background) as `unwrap_or_default` picks index 0.
#[test]
fn ink_names_its_slot_and_unknown_is_the_first() {
    let data = ThemeData::default();
    for (i, name) in INKS.iter().enumerate() {
        assert_eq!(data.ink(name), data.colors[i], "{name} is slot {i}");
    }
    assert_eq!(data.ink("no-such-role"), data.colors[0], "an unknown role reads as the background");
}

// `hex` (#RRGGBB): an exact value, lowercase hex taken, and every mistyped or out-of-range input
// — a missing field, a non-string, a short or long value, a non-hex digit — refused.
#[test]
fn hex_parses_and_refuses_mistyped_and_out_of_range() {
    fn h(v: Option<&Value>) -> Result<Color, Refused> {
        hex(v)
    }
    assert_eq!(h(Some(&json!("#aabbcc"))), Ok(c(0xaabbcc)), "lowercase hex");
    assert_eq!(h(Some(&json!("#FF0000"))), Ok(c(0xff0000)));
    assert_eq!(h(Some(&json!("#000000"))), Ok(c(0)));
    for v in [None, Some(&json!("")), Some(&json!("#FFF")), Some(&json!("#1234567")), Some(&json!("FF0000")), Some(&json!("#GGGGGG")), Some(&json!(7)), Some(&json!(true)), Some(&json!([]))] {
        assert_eq!(h(v), Err("Theme colors must be #RRGGBB"), "{v:?}");
    }
}

// The WCAG 2 luminance and contrast (the reference's): black is 0, white is 1, the same color is
// 1:1, and the maximum black-on-white ratio is exactly 21.
#[test]
fn luminance_and_contrast_are_the_wcag_ratios() {
    let (black, white) = (c(0), c(0xffffff));
    assert_eq!(luminance(black), 0.);
    assert_eq!(luminance(white), 1.);
    assert_eq!(contrast(white, black), 21.);
    assert_eq!(contrast(black, white), 21., "order does not matter");
    assert_eq!(contrast(black, black), 1.);
    assert_eq!(contrast(white, white), 1.);
    assert!(contrast(c(0xf6f8fa), c(0x111317)) >= 4.5, "the dark theme's text on its background reads");
}

// `brightness` (JUCE's perceived brightness): pure black is 0, white is 1. `dark` reads a
// background as dark when that brightness is under one half.
#[test]
fn brightness_and_dark_split_backgrounds_at_one_half() {
    let (black, white) = (c(0), c(0xffffff));
    assert_eq!(brightness(black), 0.);
    assert_eq!(brightness(white), 1.);
    assert!(dark(c(0x111317)) && dark(c(0x15181d)), "the built-in dark backgrounds");
    assert!(!dark(c(0xeef1f5)), "the light theme's background");
    assert!(!dark(white) && dark(black), "white is not dark, black is");
    // A saturated red is dark by brightness (0.49) though it reads best with black: that is why
    // `apply` uses `most_readable` for the warning chip, not `contrasting`.
    assert!(brightness(c(0xff0000)) < 0.5);
    assert_eq!(contrasting(c(0xff0000)), c(0xffffff), "red is dark to `contrasting`");
    assert_eq!(most_readable(c(0xff0000)), c(0), "but black reads better on it by WCAG");
    assert_eq!(contrasting(c(0x00ffff)), c(0), "cyan is bright to `contrasting`");
    assert_eq!(contrasting(black), c(0xffffff), "black gets white");
    assert_eq!(contrasting(white), c(0), "white gets black");
}

// `white_first` (the ink for large text on a box): white wherever white on the color reads as
// large text does (3:1), black once the color is too light for that — the boundary in both
// directions, and a mid gray that is not quite light enough for white.
#[test]
fn white_first_takes_white_on_dark_and_black_on_light() {
    let white = c(0xffffff);
    assert_eq!(white_first(c(0)), white, "white on black");
    assert_eq!(white_first(c(0xffffff)), c(0), "black on white");
    assert_eq!(white_first(c(0xff0000)), white, "saturated red: white still clears 3:1");
    assert_eq!(white_first(c(0x10161e)), white, "the light theme's text color");
    assert_eq!(white_first(c(0xaaaaaa)), c(0), "a light gray too light for white on it");
}

// `most_readable` (the chip's ink, chosen by WCAG contrast, not brightness): black on each of the
// bright and saturated colors, white on the dark ones, matching the contrast rule the reference
// uses for its own text so the chip reads on any warning color a theme chooses.
#[test]
fn most_readable_is_black_or_white_by_contrast() {
    assert_eq!(most_readable(c(0)), c(0xffffff));
    assert_eq!(most_readable(c(0xffffff)), c(0));
    assert_eq!(most_readable(c(0x00ff00)), c(0));
    assert_eq!(most_readable(c(0xffff00)), c(0));
    assert_eq!(most_readable(c(0x00ffff)), c(0));
    assert_eq!(most_readable(c(0xff0000)), c(0));
    assert_eq!(most_readable(c(0x0000ff)), c(0xffffff), "pure blue is dark enough for white");
    assert_eq!(most_readable(c(0x8b0000)), c(0xffffff), "a dark red");
    for col in [c(0xffffff), c(0), c(0xff0000), c(0x00ff00), c(0x0000ff)] {
        let ink = most_readable(col);
        assert!(contrast(ink, col) >= 3., "the chip's ink reads on {col:?}: {ink:?}");
    }
}

// A font buffer's `fontContainer` check: a minimal TrueType signature with the six required
// tables and a CFF is taken, and each part of the shape — a signature, the table count, a table
// past the end of the file, or a missing required table — is what fails it.
#[test]
fn font_container_accepts_a_minimal_font_and_refuses_the_rest() {
    fn font(tags: &[&[u8; 4]], offsets: &[(usize, usize)]) -> Vec<u8> {
        let mut b = vec![0u8; 12 + 16 * tags.len()];
        b[0..4].copy_from_slice(&[0, 1, 0, 0]);
        b[4..6].copy_from_slice(&(tags.len() as u16).to_be_bytes());
        for (i, (&tag, &(off, len))) in tags.iter().zip(offsets).enumerate() {
            let at = 12 + 16 * i;
            b[at..at + 4].copy_from_slice(tag);
            b[at + 8..at + 12].copy_from_slice(&(off as u32).to_be_bytes());
            b[at + 12..at + 16].copy_from_slice(&(len as u32).to_be_bytes());
        }
        b
    }
    let req: Vec<&[u8; 4]> = vec![b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name"];
    let mut with_cff = req.clone();
    with_cff.push(b"CFF ");
    let none = vec![(0usize, 0usize); with_cff.len()];
    assert!(font_container(&font(&with_cff, &none)), "the six required tables and a CFF");
    let with_glyf_loca = vec![b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"glyf", b"loca"];
    let none = vec![(0usize, 0usize); with_glyf_loca.len()];
    assert!(font_container(&font(&with_glyf_loca, &none)), "glyf and loca in place of a CFF");
    assert!(!font_container(&[]), "no bytes");
    assert!(!font_container(&b"xy"[..]), "too short");
    let mut wrong = font(&with_cff, &none);
    wrong[0..4].copy_from_slice(b"xxxx");
    assert!(!font_container(&wrong), "a wrong signature, the rest a good font");
    assert!(!font_container(&font(&with_cff, &[(0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (200, 8)])), "a table past the end of the file");
    let mut no_cmap = with_cff.clone();
    no_cmap[0] = b"caxp";
    assert!(!font_container(&font(&no_cmap, &none)), "a required table missing");
    let no_outline = req.clone();
    assert!(!font_container(&font(&no_outline, &none)), "neither CFF nor glyf/loca");
    let mut b69 = font(&with_cff, &none);
    b69[5] = 0;
    assert!(!font_container(&b69), "no tables is not a font");
}

// The screens take their colors, corners, strokes and sizes from the `Theme` global: no .slint file
// but theme.slint has a literal color, or a literal corner radius or font size.
#[test]
fn no_screen_hardcodes_what_the_theme_sets() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("slint");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".slint") || name == "theme.slint" {
            continue;
        }
        checked += 1;
        for (n, line) in std::fs::read_to_string(&path).unwrap().lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let hex_color = code.split('#').skip(1).any(|r| r.chars().take_while(char::is_ascii_hexdigit).count() >= 3);
            let literal = |key: &str| code.trim_start().strip_prefix(key).is_some_and(|v| v.trim_start().starts_with(|c: char| c.is_ascii_digit()));
            assert!(!hex_color && !code.contains("Colors.") && !code.contains("rgb(") && !code.contains("rgba("), "{name}:{}: a literal color: {line}", n + 1);
            assert!(!literal("border-radius:") && !literal("font-size:"), "{name}:{}: a literal radius or font size: {line}", n + 1);
            if code.contains("opacity:") {
                // A dimming alpha is the theme's `dim` (0.45) or `disabled` (0.4, theme.slint), never a literal with one of
                // those values; an alpha of its own (routing.slint's 0.5 and 0.55) has no token and may stay.
                let branch = code.split('?').nth(1).unwrap_or_default();
                for word in branch.split(|c: char| !(c.is_ascii_digit() || c == '.')) {
                    assert!(word.is_empty() || !["0.45", "0.4"].contains(&word), "{name}:{}: a literal dim or disabled alpha: {line}", n + 1);
                }
            }
        }
    }
    assert!(checked >= 9, "the screens were read");
}
