// SPDX-License-Identifier: AGPL-3.0-or-later
//! The theme the JUCE app would show (hosts/standalone/Theme.cpp): the user's choice, else the
//! screen's (the profile's), else the built-in default; its colors and radius from its
//! theme.json (docs/theme.schema.json) into the screens' `Theme` global. Built-in themes only.
//! shortcut: a theme the user installed (in the configuration folder's Themes/) is not read, and
//! its id falls back to the default here; read it from there once the Rust UI installs themes.

use crate::profile::{DEFAULT_THEME, THEMES};
use crate::{Shell, Theme};
use serde_json::Value;
use slint::{Color, ComponentHandle};

/// The built-in theme's theme.json for the first of `ids` that is one.
pub fn chosen<'a>(ids: impl IntoIterator<Item = &'a str>) -> Value {
    ids.into_iter()
        .chain([DEFAULT_THEME, "oscairn-dark"])
        .find_map(|id| THEMES.iter().find(|(t, _)| *t == id))
        .and_then(|(_, text)| serde_json::from_str(text).ok())
        .unwrap_or_default()
}

fn color(hex: &str) -> Option<Color> {
    let v = u32::from_str_radix(hex.strip_prefix('#').filter(|h| h.len() == 6)?, 16).ok()?;
    Some(Color::from_rgb_u8((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

/// Puts the theme's colors, radius and type scale in the window's `Theme` global; what it lacks
/// keeps the global's defaults.
pub fn apply(window: &Shell, theme: &Value) {
    let g = window.global::<Theme>();
    let c = &theme["colors"];
    let set = |key: &str, f: &dyn Fn(Color)| {
        if let Some(v) = c[key].as_str().and_then(color) {
            f(v)
        }
    };
    set("background", &|v| g.set_background(v));
    set("surface", &|v| g.set_surface(v));
    set("raised", &|v| g.set_raised(v));
    set("border", &|v| g.set_border(v));
    set("text", &|v| g.set_text(v));
    set("muted", &|v| g.set_muted(v));
    set("accent", &|v| g.set_accent(v));
    set("positive", &|v| g.set_positive(v));
    set("danger", &|v| g.set_danger(v));
    if let Some(r) = theme["radius"].as_f64() {
        g.set_radius(r.clamp(0., 64.) as f32);
    }
    if let Some(s) = theme["fontScale"].as_f64().filter(|s| s.is_finite()) {
        g.set_font_scale(s.clamp(0.5, 2.) as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_built_in_wins_then_the_default() {
        assert_eq!(chosen(["", "oscairn-block"])["id"], "oscairn-block");
        assert_eq!(chosen(["oscairn-light", "oscairn-block"])["id"], "oscairn-light");
        assert_eq!(chosen(["installed-by-user"])["id"], DEFAULT_THEME); // shortcut above
        assert_eq!((color("#FF9A45"), color("#FFF"), color("red")), (Some(Color::from_rgb_u8(255, 154, 69)), None, None));
    }
}
