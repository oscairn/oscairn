// SPDX-License-Identifier: AGPL-3.0-or-later
//! The eight operator colors of the FM view (ui/THEME.md "Operator colors"): derived from the theme, so every
//! theme has its own and the same operator always has the same color in it.
//!
//! The rule, in the OKLCH space (lightness, chroma and hue as the eye ranks them): operator 1 has the
//! accent's hue; operator n has it turned by 45 degrees times n - 1, so the eight hues are evenly spaced
//! around the wheel. Chroma is the accent's, at least 0.12 (a gray accent still gives colors) and at most what
//! sRGB holds at that lightness. Lightness is the accent's, moved for every second operator by 0.07 toward
//! the text's side of the panel (away from it when the accent is already at the end of the range), so neighbors on the wheel also differ in lightness; then moved further
//! toward the text until the color has a 3 to 1 contrast against the panel (the surface) and 4.5 to 1 against the
//! text on it, as far as the range (lightness 0.3 to 0.86, where hues still differ) allows. A fill's text is the theme's text or background color, whichever contrasts more.

use slint::Color;

/// One operator's color and the text color that reads on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OperatorColor {
    pub fill: Color,
    pub on: Color,
}

const LIGHTNESS_STEP: f32 = 0.07;
const LEAST_CHROMA: f32 = 0.12;
/// Past these sRGB has too little chroma left for hues to differ.
const LEAST_LIGHTNESS: f32 = 0.3;
const MOST_LIGHTNESS: f32 = 0.86;
/// Against the panel: the least for graphics (WCAG 1.4.11); against the text on a fill: the least for text.
const AGAINST_PANEL: f32 = 3.;
const AGAINST_TEXT: f32 = 4.5;
const HUE_STEP: f32 = 45.;

fn linear(c: u8) -> f32 {
    let v = f32::from(c) / 255.;
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0., 1.);
    let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1. / 2.4) - 0.055 };
    (s * 255.).round() as u8
}

/// WCAG relative luminance.
fn luminance(c: Color) -> f32 {
    0.2126 * linear(c.red()) + 0.7152 * linear(c.green()) + 0.0722 * linear(c.blue())
}

/// WCAG contrast ratio, 1 to 21.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// OKLab of a color: lightness, then the two color axes.
pub fn oklab(c: Color) -> (f32, f32, f32) {
    let (r, g, b) = (linear(c.red()), linear(c.green()), linear(c.blue()));
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    (0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s)
}

#[cfg(test)]
/// The distance between two colors in OKLab (about 0.02 is the least a person sees; 0.1 is plainly apart).
pub fn distance(a: Color, b: Color) -> f32 {
    let (p, q) = (oklab(a), oklab(b));
    ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2) + (p.2 - q.2).powi(2)).sqrt()
}

/// The sRGB color of OKLCH (lightness, chroma, hue in degrees), and whether it lay inside sRGB.
fn from_oklch(l: f32, c: f32, h: f32) -> (Color, bool) {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let r = 4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_93 * s_;
    let g = -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_4 * s_;
    let bl = -0.004_196_086 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_;
    let inside = [r, g, bl].iter().all(|v| (-0.000_5..=1.000_5).contains(v));
    (Color::from_rgb_u8(encode(r), encode(g), encode(bl)), inside)
}

/// The color at this lightness and hue with as much chroma as sRGB holds up to `chroma`.
fn fitted(l: f32, chroma: f32, hue: f32) -> Color {
    let mut c = chroma;
    loop {
        let (color, inside) = from_oklch(l, c, hue);
        if inside || c < 0.005 {
            return color;
        }
        c -= 0.005;
    }
}

fn oklch(c: Color) -> (f32, f32, f32) {
    let (l, a, b) = oklab(c);
    (l, a.hypot(b), b.atan2(a).to_degrees())
}

/// The eight operators' colors in a theme of this accent, panel (surface), background and text.
pub fn operators(accent: Color, surface: Color, background: Color, text: Color) -> [OperatorColor; 8] {
    let (l0, c0, h0) = oklch(accent);
    let chroma = c0.max(LEAST_CHROMA);
    // Toward the text's side of the panel: lighter on a dark panel, darker on a light one.
    let toward = if luminance(text) > luminance(surface) { 1. } else { -1. };
    let mut out = [OperatorColor { fill: accent, on: text }; 8];
    for (k, slot) in out.iter_mut().enumerate() {
        let hue = h0 + HUE_STEP * k as f32;
        // Every second operator is a step toward the text; when the accent is already at the end of the range, a step away.
        let base = l0.clamp(LEAST_LIGHTNESS, MOST_LIGHTNESS);
        let stepped = base + toward * LIGHTNESS_STEP;
        let other = if (LEAST_LIGHTNESS..=MOST_LIGHTNESS).contains(&stepped) { stepped } else { base - toward * LIGHTNESS_STEP };
        let mut l = if k % 2 == 1 { other } else { base };
        let on_of = |fill: Color| if contrast(fill, text) >= contrast(fill, background) { text } else { background };
        let mut fill = fitted(l, chroma, hue);
        while (contrast(fill, surface) < AGAINST_PANEL || contrast(fill, on_of(fill)) < AGAINST_TEXT) && (LEAST_LIGHTNESS..MOST_LIGHTNESS).contains(&(l + toward * 0.01)) {
            l += toward * 0.01;
            fill = fitted(l, chroma, hue);
        }
        let on = on_of(fill);
        *slot = OperatorColor { fill, on };
    }
    out
}

/// The colors of a selected tab and the text on it, and of an unselected tab and its text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabColors {
    pub on: Color,
    pub on_ink: Color,
    pub off: Color,
    pub off_ink: Color,
}

/// The least contrast between a selected tab's fill and an unselected one's, in every theme (WCAG 1.4.11's
/// 3 to 1 for graphics): the fill, not an outline, is what tells them apart.
pub const TAB_CONTRAST: f32 = 3.;

/// A selected tab's fill is the accent where the accent holds 3 to 1 against the unselected tab (the raised
/// color), the surface and the background (a strip sits on either) and 4.5 to 1 against its text; else the
/// theme's text color, which a theme file must keep 4.5 to 1 from all three, so the guarantee holds in any
/// theme the loader accepts. The text on it is the text or background color, whichever contrasts more; an
/// unselected tab is the raised color with the muted text.
pub fn tabs(accent: Color, text: Color, surface: Color, background: Color, raised: Color, muted: Color) -> TabColors {
    let ink_on = |fill: Color| if contrast(fill, text) >= contrast(fill, background) { text } else { background };
    let fits = |fill: Color| {
        [raised, surface, background].iter().all(|&against| contrast(fill, against) >= TAB_CONTRAST) && contrast(fill, ink_on(fill)) >= AGAINST_TEXT
    };
    let on = if fits(accent) { accent } else { text };
    TabColors { on, on_ink: ink_on(on), off: raised, off_ink: muted }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_srgb_transfer_keeps_black_and_white_and_clamps() {
        assert_eq!(linear(0), 0.);
        assert_eq!(linear(255), 1.);
        // A middle gray is brighter than black and between the two; the transfer rises monotonically.
        let mid = linear(128);
        assert!(0. < mid && mid < 1.);
        assert!(linear(10) < linear(20));
        // encode rounds back at the ends and clamps a value outside 0..1 to the nearer end.
        assert_eq!(encode(0.), 0);
        assert_eq!(encode(1.), 255);
        assert_eq!(encode(-0.5), 0);
        assert_eq!(encode(1.5), 255);
    }

    #[test]
    fn contrast_is_one_against_itself_and_twenty_one_black_to_white() {
        let (white, black) = (Color::from_rgb_u8(255, 255, 255), Color::from_rgb_u8(0, 0, 0));
        assert_eq!(contrast(white, white), 1.);
        // Black to white is the sheet's whole range, 21 to 1 (float rounding, not the exact value here).
        let bw = contrast(black, white);
        assert!(bw > 20.99 && bw <= 21., "black to white is {bw}");
        assert_eq!(contrast(white, black), contrast(black, white));
        assert_eq!(luminance(black), 0.);
        assert_eq!(luminance(white), 1.);
    }

    #[test]
    fn black_is_oklab_origin_and_an_accent_keeps_its_hue() {
        assert_eq!(oklab(Color::from_rgb_u8(0, 0, 0)), (0., 0., 0.));
        let accent = Color::from_rgb_u8(128, 128, 128);
        let surface = Color::from_rgb_u8(0, 0, 0);
        let text = Color::from_rgb_u8(255, 255, 255);
        let eight = operators(accent, surface, surface, text);
        assert_eq!(eight.len(), 8);
        // A gray accent still gives eight colors, and none repeats (the lightness steps apart of the
        // 0.07 band also separate neighbors whose same-chroma fills the gray hue would leave near one another).
        for (i, a) in eight.iter().enumerate() {
            for b in &eight[i + 1..] {
                assert!(distance(a.fill, b.fill) > 0.02, "two operators are not apart in OKLab");
            }
        }
        // The first operator keeps a colored accent's hue, whatever the filling does to lightness
        // (a gray accent's hue is undefined, so the check needs a chroma-full accent).
        let colored = Color::from_rgb_u8(0, 122, 204);
        let (_, _, fill_hue) = oklch(operators(colored, surface, surface, text)[0].fill);
        let (_, _, accent_hue) = oklch(colored);
        assert!((fill_hue - accent_hue).abs() <= 2., "operator 1 left the accent's hue");
    }

    #[test]
    fn from_oklch_and_fitted_stay_inside_srgb_or_say_so() {
        // No chroma is a gray, always inside sRGB, with the three channels equal.
        let (gray, inside) = from_oklch(0.5, 0., 0.);
        assert!(inside);
        assert_eq!(gray.red(), gray.blue());
        // Chroma 0.8 is more than sRGB can hold at any hue: the flag says so, and fitted comes back inside.
        assert!(!from_oklch(0.5, 0.8, 0.).1);
        assert!(!from_oklch(0.5, 0.8, 45.).1);
        let fit = fitted(0.5, 0.8, 0.);
        assert!(oklch(fit).1 < 0.8, "fitted did not reduce the chroma to fit sRGB");
        // A tiny chroma is already inside and is not reduced below what was asked.
        assert!(from_oklch(0.5, 0.005, 0.).1);
    }

    #[test]
    fn a_tab_takes_the_accent_where_it_fits_and_the_text_color_otherwise() {
        let (white, black) = (Color::from_rgb_u8(255, 255, 255), Color::from_rgb_u8(0, 0, 0));
        let muted = Color::from_rgb_u8(200, 200, 200);
        // A white accent against black everywhere holds 3 to 1 against every panel and 4.5 to 1 against
        // its ink, so the selectable fill is the accent and its ink the text.
        let t = tabs(white, black, black, black, black, muted);
        assert_eq!(t.on, white);
        assert_eq!(t.on_ink, black);
        assert_eq!((t.off, t.off_ink), (black, muted));
        // An accent too close to the panels to hold 3 to 1 (gray on white) falls back to the theme's text
        // color; the text is distinct from the accent, so `on` proves the fallback branch ran, not the accent's.
        let accent = Color::from_rgb_u8(128, 128, 128);
        let text = Color::from_rgb_u8(30, 30, 30);
        let t = tabs(accent, text, white, white, white, muted);
        assert_eq!(t.on, text, "the fill is the theme's text, not the accent");
        assert_eq!(t.on_ink, white);
        assert_eq!((t.off, t.off_ink), (white, muted));
    }

    #[test]
    fn the_operator_fills_are_deterministic_and_their_ink_is_only_text_or_background() {
        let accent = Color::from_rgb_u8(0, 122, 204);
        let surface = Color::from_rgb_u8(238, 238, 238);
        let background = Color::from_rgb_u8(255, 255, 255);
        let text = Color::from_rgb_u8(30, 30, 30);
        let a = operators(accent, surface, background, text);
        assert_eq!(a, operators(accent, surface, background, text), "the same theme gives the same colors");
        for o in &a {
            assert!(o.on == text || o.on == background, "a fill's ink is only the theme's text or background");
        }
    }
}
