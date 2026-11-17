// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Settings app (RFC 0022; hosts/standalone/apps/settings/ is the reference, ui/SETTINGS-APP.md the checklist):
//! the pages a player sets the instrument with, each of a kind (universal, per desktop OS, per hardware target) and
//! shown only where the device profile says it applies: Appearance (the theme), Top bar, MIDI controllers, Audio,
//! Package repositories, Signing keys and About.
//!
//! On the shared list screen (`browser.rs`, the Part screen's component) with the pages as the shared tab family
//! (`slint/settings.slint`, the theme's `tabPlacement` putting them in a strip or a rail), as the Library app is. A
//! page is a `Page`: it makes its rows and the panel's lines, takes a press, a menu's choice, a question's answer and
//! the keyboard's text, and asks the engine through `Ctx`. Without touch: the dial and the left knob walk the tabs
//! then the rows, a press on a row changes it (a choice opens a list, a switch turns over), the right knob scrolls
//! the panel, Cancel goes back one level.
//!
//! Nothing here waits on the engine: a request goes out and its answer comes back as `reply`; one the engine refuses
//! as busy (`-32009`) is asked again in a moment. A list changes only when the engine says so (the status's
//! `audioDeviceList`, `midiPortList`), never on a timer.

use crate::settings_audio::Audio;
use crate::browser::{Browser, Nav};
use crate::client::{Error, BUSY};
use crate::settings_controllers::Controllers;
use crate::settings_keys::{About, Keys};
use crate::settings_look::{Appearance, TopBarPage};
use crate::menu;
use crate::nav::Action;
use crate::profile::Profile;
use crate::settings_repos::Repositories;
use crate::tabs;
use crate::topbar::{MeterSettings, Tool};
use crate::{InfoLine, SettingData, SettingsState, Shell, TabLabel};
use serde_json::Value;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::time::Duration;

/// The pages, by what they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id {
    Appearance,
    TopBar,
    Controllers,
    Audio,
    Repositories,
    Keys,
    About,
}

impl Id {
    /// In the order the tabs show them: what a player changes most first.
    pub const ALL: [Id; 7] = [Id::Appearance, Id::TopBar, Id::Controllers, Id::Audio, Id::Repositories, Id::Keys, Id::About];

    pub fn title(self) -> &'static str {
        match self {
            Id::Appearance => "Appearance",
            Id::TopBar => "Top bar",
            Id::Controllers => "MIDI controllers",
            Id::Audio => "Audio",
            Id::Repositories => "Package repositories",
            Id::Keys => "Signing keys",
            Id::About => "About",
        }
    }

    /// Whether the profile has the page (ui/SETTINGS-APP.md, "The three kinds"): Audio only where a device chooses its
    /// audio device, which a profile that fixes the device does not; every other page is universal.
    pub fn applies(self, profile: &Profile) -> bool {
        match self {
            Id::Audio => !profile.audio_fixed,
            _ => true,
        }
    }
}

/// What a request of a page is for, so its answer goes where it belongs.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Audio(crate::settings_audio::Step),
    Controllers(crate::settings_controllers::Step),
    Repos(crate::settings_repos::Step),
    Keys(crate::settings_keys::Step),
    About(crate::settings_keys::AboutStep),
}

/// A request as the shell sends it, kept whole so a busy engine's refusal can be answered by asking it again.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub step: Step,
    pub method: &'static str,
    pub params: Value,
}

/// What the screen asks the engine, and when (a step refused as busy is asked again).
pub struct Send {
    pub call: Call,
    pub after: Option<Duration>,
}

/// What a page asks of the screen: requests, a menu, a question, text from the keyboard, a line to say, and what it
/// changed in the shell itself.
#[derive(Default)]
pub struct Ctx {
    pub out: Vec<Send>,
    pub menu: Option<(String, Vec<menu::Item>)>,
    pub notice: Option<(String, String, Vec<String>)>,
    pub prompt: Option<(String, String)>,
    pub message: Option<String>,
    /// The theme chosen, to show now.
    pub theme: Option<crate::theme::Loaded>,
    /// The top bar's meters now, to show.
    pub meters: Option<MeterSettings>,
}

impl Ctx {
    pub fn ask(&mut self, step: Step, method: &'static str, params: Value) {
        self.out.push(Send { call: Call { step, method, params }, after: None });
    }

    pub fn ask_after(&mut self, after: Duration, step: Step, method: &'static str, params: Value) {
        self.out.push(Send { call: Call { step, method, params }, after: Some(after) });
    }
}

/// What a page is: its rows, the lines of the panel, and what a press and the answers do.
pub trait Page {
    /// The page is shown (or shown again): it reads what it shows.
    fn open(&mut self, c: &mut Ctx);
    fn rows(&self) -> Vec<SettingData>;
    /// The panel: what the row under the cursor (`at`) is and says.
    fn info(&self, at: Option<usize>) -> Vec<InfoLine>;
    fn press(&mut self, at: usize, c: &mut Ctx);
    /// The page's own actions on the top bar.
    fn tools(&self, _at: Option<usize>) -> Vec<Tool> {
        vec![]
    }
    fn tool(&mut self, _i: usize, _at: Option<usize>, _c: &mut Ctx) {}
    /// A choice of the menu the page opened.
    fn chosen(&mut self, _id: usize, _c: &mut Ctx) {}
    /// The answer to a question the page asked: the choice, or none.
    fn noticed(&mut self, _choice: Option<usize>, _c: &mut Ctx) {}
    /// The keyboard's text.
    fn entered(&mut self, _text: &str, _c: &mut Ctx) {}
    fn not_entered(&mut self, _c: &mut Ctx) {}
    fn status(&mut self, _status: &Value, _c: &mut Ctx) {}
    /// Back out of a level of the page's own: whether there was one.
    fn back(&mut self, _c: &mut Ctx) -> bool {
        false
    }
    /// The levels of the page's own below its name, for the bar's title.
    fn path(&self) -> Vec<String> {
        vec![]
    }
    /// What the footer counts.
    fn noun(&self) -> &'static str {
        "Settings"
    }
    /// The line under the list.
    fn message(&self) -> String {
        String::new()
    }
}

// ---- what a page makes its rows and its panel of ----

pub fn row(label: impl Into<String>, value: impl Into<String>) -> SettingData {
    SettingData { label: label.into().into(), value: value.into().into(), kind: 2, ..Default::default() }
}

/// A row that opens a list of choices or a question: its value ends in the mark that says so.
pub fn choice(label: impl Into<String>, value: impl AsRef<str>) -> SettingData {
    row(label, format!("{} ›", value.as_ref()))
}

pub fn switch(label: impl Into<String>, on: bool) -> SettingData {
    SettingData { label: label.into().into(), value: if on { "On" } else { "Off" }.into(), kind: 1, on, switch: true, ..Default::default() }
}

/// A row the page does not let be changed now: faint.
pub fn faint(mut r: SettingData) -> SettingData {
    r.off = true;
    r
}

pub fn head(text: &str) -> InfoLine {
    InfoLine { head: true, text: text.into(), ..Default::default() }
}

pub fn line(text: &str) -> InfoLine {
    InfoLine { text: text.into(), ..Default::default() }
}

/// A sentence on the panel's lines.
pub fn said(text: &str) -> Vec<InfoLine> {
    crate::library::wrap(text, crate::library::INFO_WIDTH).iter().map(|l| line(l)).collect()
}

/// A failure, on lines in the danger color.
pub fn failed(text: &str) -> Vec<InfoLine> {
    crate::library::wrap(text, crate::library::INFO_WIDTH).iter().map(|l| InfoLine { failed: true, text: l.as_str().into(), ..Default::default() }).collect()
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Stay,
    Back,
    /// Text asked for on the keyboard: its title and what it starts with; `entered` takes it.
    Prompt(String, String),
}

pub struct Screen {
    ui: slint::Weak<Shell>,
    b: Browser,
    /// The pages the profile has, in order.
    pages: Vec<Id>,
    at: usize,
    /// The tab the cursor is on, when it is on the tabs.
    tab_at: Option<usize>,
    appearance: Appearance,
    top_bar: TopBarPage,
    controllers: Controllers,
    audio: Audio,
    repos: Repositories,
    keys: Keys,
    about: About,
    ctx: Ctx,
    /// Whether this screen is the one shown: the list and the tabs are written only then.
    active: bool,
    /// The tabs as last written: how many, which, the cursor.
    tabs_shown: Option<(usize, usize, Option<usize>)>,
    pub out: Vec<Send>,
}

impl Screen {
    pub fn new(ui: &Shell, profile: &Profile) -> Screen {
        let config = crate::settings::config_dir();
        Screen {
            ui: ui.as_weak(),
            b: Browser::new(ui),
            pages: Id::ALL.into_iter().filter(|p| p.applies(profile)).collect(),
            at: 0,
            tab_at: None,
            appearance: Appearance::new(crate::theme::Places::found(), profile.theme.clone(), config.clone()),
            top_bar: TopBarPage::new(profile.top_bar, config),
            controllers: Controllers::default(),
            audio: Audio::default(),
            repos: Repositories::default(),
            keys: Keys::default(),
            about: About::new(),
            ctx: Ctx::default(),
            active: false,
            tabs_shown: None,
            out: vec![],
        }
    }

    /// Where the theme and the top bar's choices are kept, and where the themes are (tests use folders of their own).
    #[cfg(test)]
    pub fn set_folders(&mut self, config: Option<std::path::PathBuf>, places: crate::theme::Places) {
        self.appearance.set_folders(config.clone(), places);
        self.top_bar.set_folder(config);
    }

    /// Tests: where the theme chooser starts.
    #[cfg(test)]
    pub fn set_files_dir(&mut self, dir: std::path::PathBuf) {
        self.appearance.set_files_dir(dir);
    }

    /// Tests: a theme chosen in the chooser, by its path.
    #[cfg(test)]
    pub fn choose_file(&mut self, path: &std::path::Path) {
        self.appearance.install(path, &mut self.ctx);
        self.take_up();
        self.show();
    }

    /// Another program chose a theme and it is shown: the Appearance page marks it.
    pub fn theme_shown(&mut self) {
        if let Some(ui) = self.ui.upgrade() {
            let id = ui.global::<crate::Theme>().get_id().to_string();
            self.appearance.shows(&id);
        }
        self.show();
    }

    /// The pages shown.
    #[cfg(test)]
    pub fn pages(&self) -> &[Id] {
        &self.pages
    }

    fn page(&self) -> &dyn Page {
        match self.pages[self.at] {
            Id::Appearance => &self.appearance,
            Id::TopBar => &self.top_bar,
            Id::Controllers => &self.controllers,
            Id::Audio => &self.audio,
            Id::Repositories => &self.repos,
            Id::Keys => &self.keys,
            Id::About => &self.about,
        }
    }

    /// Something for the page shown, then what it asked for taken up.
    fn with_page<R>(&mut self, f: impl FnOnce(&mut dyn Page, &mut Ctx) -> R) -> R {
        let r = match self.pages[self.at] {
            Id::Appearance => f(&mut self.appearance, &mut self.ctx),
            Id::TopBar => f(&mut self.top_bar, &mut self.ctx),
            Id::Controllers => f(&mut self.controllers, &mut self.ctx),
            Id::Audio => f(&mut self.audio, &mut self.ctx),
            Id::Repositories => f(&mut self.repos, &mut self.ctx),
            Id::Keys => f(&mut self.keys, &mut self.ctx),
            Id::About => f(&mut self.about, &mut self.ctx),
        };
        self.take_up();
        r
    }

    /// What the pages asked of the shell: the requests out. The theme and the bar's meters are taken by `take_look`.
    fn take_up(&mut self) {
        self.out.append(&mut self.ctx.out);
        if let Some(text) = self.ctx.message.take() {
            self.b.say(&text);
        }
    }

    /// The theme chosen and the top bar's meters changed, to show: not here, as putting a theme in the window runs the
    /// screens' handlers, which the shell cannot be inside of.
    pub fn take_look(&mut self) -> (Option<crate::theme::Loaded>, Option<MeterSettings>) {
        (self.ctx.theme.take(), self.ctx.meters.take())
    }

    /// The app opens: the first page, read.
    pub fn open(&mut self) {
        self.at = 0;
        self.tab_at = None;
        self.b.reset();
        self.with_page(|p, c| p.open(c));
        self.show();
    }

    pub fn shown(&mut self) {
        self.active = true;
        self.b.install();
        self.show();
    }

    pub fn hidden(&mut self) {
        self.active = false;
        self.b.hidden();
        self.tab_at = None;
        self.tabs_shown = None;
    }

    /// The page shown, as a tab names it.
    pub fn show_page(&mut self, page: usize) {
        self.tab_at = None;
        if page < self.pages.len() && page != self.at {
            self.at = page;
            self.b.reset_cursor();
            self.with_page(|p, c| p.open(c));
        }
        self.show();
    }

    pub fn tap_page(&mut self, page: usize) {
        self.show_page(page);
    }

    pub fn at_top(&self) -> bool {
        // The pages are always shown: the rail is above the list, so the top is the rail's first tab,
        // as the Library's is while its tabs show.
        self.tab_at == Some(0)
    }

    /// The bar's title: Settings, the page, and the level of its own.
    pub fn path(&self) -> Vec<String> {
        let mut path = vec!["Settings".to_string(), self.pages[self.at].title().to_string()];
        path.extend(self.page().path());
        path
    }

    fn rows(&self) -> Vec<SettingData> {
        self.page().rows()
    }

    fn show_tabs(&mut self) {
        let Some(ui) = self.ui.upgrade().filter(|_| self.active) else { return };
        let state = (self.pages.len(), self.at, self.tab_at);
        if self.tabs_shown == Some(state) {
            return;
        }
        let was = self.tabs_shown.replace(state);
        let g = ui.global::<SettingsState>();
        if was.map(|w| w.0) != Some(state.0) {
            let names: Vec<String> = self.pages.iter().map(|p| p.title().to_string()).collect();
            let tabs::Abbreviated { heading, shorts } = tabs::abbreviate(&names);
            let labels: Vec<TabLabel> = shorts
                .into_iter()
                .map(|short| {
                    let (top, bottom) = tabs::two_lines(&short);
                    TabLabel { short: short.into(), top: top.into(), bottom: bottom.into() }
                })
                .collect();
            g.set_pages(ModelRc::new(VecModel::from(names.iter().map(|n| n.as_str().into()).collect::<Vec<slint::SharedString>>())));
            g.set_page_labels(ModelRc::new(VecModel::from(labels)));
            g.set_page_heading(heading.into());
        }
        g.set_page(state.1 as i32);
        g.set_page_cursor(state.2.map_or(-1, |c| c as i32));
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
        let rows = self.rows();
        let info = self.page().info(self.b.cursor.at);
        let title = self.path().last().cloned().unwrap_or_default();
        let noun = self.page().noun();
        let message = self.page().message();
        self.b.show("", &title, noun, rows, info);
        self.b.say(&message);
    }

    // ---- the top bar ----

    pub fn bar_tools(&self) -> Vec<Tool> {
        self.page().tools(self.b.cursor.at)
    }

    pub fn tool(&mut self, i: usize) -> Outcome {
        let at = self.b.cursor.at;
        self.with_page(|p, c| p.tool(i, at, c));
        self.finish()
    }

    // ---- the keys, a press and a tap ----

    /// What the page asked that the shell must open: the keyboard.
    fn finish(&mut self) -> Outcome {
        self.show();
        match self.ctx.prompt.take() {
            Some((title, text)) => Outcome::Prompt(title, text),
            None => Outcome::Stay,
        }
    }

    fn handle(&mut self, nav: Nav) -> Outcome {
        match nav {
            Nav::Press(at) => self.with_page(|p, c| p.press(at, c)),
            Nav::Pane(by) => self.b.scroll_info(by.signum()),
            Nav::Back => return self.back_one(),
            Nav::Dial(..) | Nav::None => {}
        }
        self.finish()
    }

    /// Back from anywhere on the screen: up a level of the page, else out of the app.
    pub fn back(&mut self) -> Outcome {
        self.back_one()
    }

    fn back_one(&mut self) -> Outcome {
        self.tab_at = None;
        if self.with_page(|p, c| p.back(c)) {
            self.b.reset_cursor();
            return self.finish();
        }
        self.show();
        Outcome::Back
    }

    /// The tabs have the cursor: the dial walks them and leaves for the list past the last; left and right along them.
    fn act_tabs(&mut self, action: Action, at: usize) -> Outcome {
        let last = self.pages.len().saturating_sub(1);
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
            Action::Cancel(_) => return self.back_one(),
            _ => {}
        }
        self.show();
        Outcome::Stay
    }

    pub fn act(&mut self, action: Action) -> Outcome {
        if let Some(at) = self.tab_at {
            return self.act_tabs(action, at);
        }
        // Moving up out of the first row reaches the tabs.
        let up = matches!(action, Action::Move { pane: None | Some(0), by } if by < 0) || matches!(action, Action::Arrow { dy, .. } if dy < 0);
        if up && matches!(self.b.cursor.at, Some(0)) {
            self.b.cursor = self.b.cursor.clear();
            self.tab_at = Some(self.at);
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

    pub fn scroll_to(&mut self, first: i32) {
        self.b.scroll_to(first.max(0) as usize);
        self.show();
    }

    pub fn scroll_info(&mut self, by: i32) {
        self.b.scroll_info(by as i64);
        self.show();
    }

    pub fn step_row(&mut self, _row: i32, _by: i32) {}

    // ---- what a menu, a question and the keyboard answer ----

    pub fn take_menu(&mut self) -> Option<(String, Vec<menu::Item>)> {
        self.ctx.menu.take()
    }

    pub fn take_notice(&mut self) -> Option<(String, String, Vec<String>)> {
        self.ctx.notice.take()
    }

    pub fn chosen(&mut self, id: usize) {
        self.with_page(|p, c| p.chosen(id, c));
        self.show();
    }

    pub fn noticed(&mut self, choice: Option<usize>) {
        self.with_page(|p, c| p.noticed(choice, c));
        self.show();
    }

    pub fn entered(&mut self, text: &str) {
        self.with_page(|p, c| p.entered(text, c));
        self.show();
    }

    pub fn not_entered(&mut self) {
        self.with_page(|p, c| p.not_entered(c));
        self.show();
    }

    /// A status (a reply or the notification): what a page lists again when it says that list changed.
    pub fn status(&mut self, status: &Value) {
        self.with_page(|p, c| p.status(status, c));
        self.show();
    }

    /// Whether a request is a long one: the engine may take seconds, and the player waits on it.
    pub fn waits(call: &Call) -> bool {
        matches!(call.step, Step::Audio(crate::settings_audio::Step::Set(_)))
    }

    /// An answer. A step the engine refused as busy is asked again in a moment; any other goes to its page.
    pub fn reply(&mut self, call: Call, result: Result<Value, Error>) {
        if let Err(e) = &result {
            if e.code == BUSY {
                self.ctx.ask_after(Duration::from_millis(300), call.step, call.method, call.params);
                self.take_up();
                return;
            }
        }
        match call.step {
            Step::Audio(s) => self.audio.reply(s, result, &mut self.ctx),
            Step::Controllers(s) => self.controllers.reply(s, result, &mut self.ctx),
            Step::Repos(s) => self.repos.reply(s, result, &mut self.ctx),
            Step::Keys(s) => self.keys.reply(s, result, &mut self.ctx),
            Step::About(s) => self.about.reply(s, result, &mut self.ctx),
        }
        self.take_up();
        self.show();
    }
}
