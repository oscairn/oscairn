// SPDX-License-Identifier: AGPL-3.0-or-later
//! The operator colors (src/palette.rs, ui/THEME.md "Operator colors"): eight apart and readable in every
//! built-in theme and the two odd ones, the same each time, and what the window carries.

use super::*;

#[test]
fn the_operator_colors_are_eight_apart_and_read_against_the_panel_in_every_theme() {
    use crate::palette::{contrast, distance, operators};
    let s = Screen::new();
    for t in &all_themes() {
        let c = |n: &str| t.data.ink(n);
        let colors = operators(c("accent"), c("surface"), c("background"), c("text"));
        let (mut apart, mut against, mut on) = (f32::MAX, f32::MAX, f32::MAX);
        for (i, a) in colors.iter().enumerate() {
            against = against.min(contrast(a.fill, c("surface")));
            on = on.min(contrast(a.fill, a.on));
            for b in &colors[i + 1..] {
                apart = apart.min(distance(a.fill, b.fill));
            }
        }
        println!("{}: least distance {apart:.3} (OKLab), least contrast on the panel {against:.2}, least text on a fill {on:.2}", t.id);
        assert!(apart >= 0.07, "{}: two operator colors are {apart:.3} apart", t.id);
        assert!(against >= 3., "{}: an operator color has {against:.2} to 1 on the panel", t.id);
        assert!(on >= 4.5, "{}: the text on an operator's fill has {on:.2} to 1", t.id);
        assert_eq!(colors, operators(c("accent"), c("surface"), c("background"), c("text")), "the same colors each time");
        theme::apply(&s.ui, t);
        let g = s.ui.global::<crate::Theme>();
        let set: Vec<slint::Color> = g.get_operators().iter().collect();
        assert_eq!(set, colors.iter().map(|o| o.fill).collect::<Vec<_>>(), "{}: the window has them", t.id);
        let on_set: Vec<slint::Color> = g.get_operators_on().iter().collect();
        assert_eq!(on_set, colors.iter().map(|o| o.on).collect::<Vec<_>>(), "{}: and the text on them", t.id);
    }
}
