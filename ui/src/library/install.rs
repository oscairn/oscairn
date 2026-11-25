// SPDX-License-Identifier: AGPL-3.0-or-later
//! Installing an archive (ui/LIBRARY-APP.md D2; LibraryPane.cpp `install`, `InstallChoices`, `describeSigner`): its contents
//! are read (`library.contents`), then what it holds is asked about: who signed it, how it goes in (into the user's
//! own Sounds and Performances to edit, or as a locked library), which items, the plugin builds it adds with who
//! signed each, its license and where its source is, and a warning for builds from an unknown source or a revoked
//! key (RFC 0006). Install (or "Install anyway" where unknown code would be accepted) sends `library.install`.

use super::*;

/// Who signed a build, or the archive (`library.contents`' `signer` and each `code` entry).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Signer {
    pub kind: String,
    pub name: String,
    pub version: String,
    pub platform: String,
    /// `trusted`, `unknown`, `revoked` or `unsigned`.
    pub status: String,
    pub key: String,
    pub key_name: String,
    pub public_key: String,
}

impl Signer {
    pub fn from_json(v: &Value) -> Signer {
        let text = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Signer { kind: text("kind"), name: text("name"), version: text("version"), platform: text("platform"), status: text("status"), key: text("key"), key_name: text("keyName"), public_key: text("publicKey") }
    }
}

/// A key's ID as people read it (RFC 0006's fingerprint, SigningKeys.h `fingerprint`): its digits in groups of four.
pub fn fingerprint(id: &str) -> String {
    id.chars().collect::<Vec<_>>().chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

/// Who signed an archive (LibraryPane.cpp `describeSigner`).
pub fn describe_signer(s: &Signer) -> String {
    let who = if s.key_name.is_empty() { format!("key {}", fingerprint(&s.key)) } else { format!("{} ({})", s.key_name, fingerprint(&s.key)) };
    match s.status.as_str() {
        "trusted" => format!("Signed by {who}, a key you trust"),
        "unknown" => format!("Signed by {who}, not a key you trust"),
        "revoked" => format!("Signed by {who}, which is revoked"),
        _ => "Not signed".into(),
    }
}

/// One item an archive holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub ticked: bool,
}

/// An archive's contents and the choices made so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    pub file: std::path::PathBuf,
    pub name: String,
    pub kind: String,
    pub items: Vec<Held>,
    pub code: Vec<Signer>,
    pub signer: Signer,
    pub license: String,
    pub source: Option<(String, String)>,
    /// Into the user's own Sounds and Performances (else a locked library).
    pub own: bool,
    /// Installing says yes to code from an unknown source.
    pub asks: bool,
}

impl Install {
    pub fn from_json(file: std::path::PathBuf, v: &Value) -> Install {
        let kind = v["kind"].as_str().unwrap_or_default().to_string();
        let code: Vec<Signer> = v["code"].as_array().into_iter().flatten().map(Signer::from_json).collect();
        let asks = code.iter().any(|c| c.status != "trusted" && c.status != "revoked");
        let mut items: Vec<Held> = v["items"].as_array().into_iter().flatten().map(|i| Held { kind: i["kind"].as_str().unwrap_or_default().into(), id: i["id"].as_str().unwrap_or_default().into(), name: i["name"].as_str().unwrap_or_default().into(), ticked: true }).collect();
        // Under a heading for each kind there is: Performances, then Sounds, then the rest (the types in their order).
        let rank = |k: &str| match k {
            "performance" => 0,
            "sound" => 1,
            "setlist" => 2,
            _ => 3,
        };
        items.sort_by_key(|i| rank(&i.kind));
        Install {
            file,
            name: v["name"].as_str().unwrap_or_default().into(),
            own: kind != "library",
            kind,
            items,
            code,
            signer: Signer::from_json(&v["signer"]),
            license: v["license"].as_str().unwrap_or_default().into(),
            source: v["source"].as_object().map(|s| (s.get("url").and_then(|v| v.as_str()).unwrap_or_default().to_string(), s.get("revision").and_then(|v| v.as_str()).unwrap_or_default().to_string())),
            asks,
        }
    }

    /// A plugin or asset package holds no items, so offers no choice of how: its builds go into the store either way.
    pub fn package(&self) -> bool {
        self.kind == "plugin" || self.kind == "asset"
    }

    pub fn rows(&self) -> Vec<SettingData> {
        let mut rows = Vec::new();
        let radio = |label: &str, on: bool| SettingData { label: label.into(), value: if on { "Chosen" } else { "" }.into(), kind: 1, on, ..Default::default() };
        if !self.package() {
            rows.push(radio("Into my own, to edit", self.own));
            rows.push(radio("As a locked library", !self.own));
        }
        for i in &self.items {
            let what = super::item::Kind::from_key(&i.kind).map_or(i.kind.clone(), |k| k.singular().to_string());
            rows.push(SettingData { label: if i.name.is_empty() { "Untitled".into() } else { i.name.as_str().into() }, value: what.into(), kind: 1, on: i.ticked, switch: true, ..Default::default() });
        }
        let row = |label: &str| SettingData { label: label.into(), value: String::new().into(), kind: 2, ..Default::default() };
        rows.push(row(if self.asks { "Install anyway" } else { "Install" }));
        rows.push(row("Cancel"));
        rows
    }

    pub fn info(&self) -> Vec<InfoLine> {
        let head = |t: &str| InfoLine { head: true, text: t.into(), ..Default::default() };
        let line = |t: String| InfoLine { head: false, text: t.into(), ..Default::default() };
        let warn = |t: String| InfoLine { text: t.into(), failed: true, ..Default::default() };
        let mut lines = vec![head(&format!("Install {}", if self.name.is_empty() { "an archive" } else { &self.name }))];
        lines.extend(wrap(&describe_signer(&self.signer), INFO_WIDTH).into_iter().map(line));
        if !self.package() {
            lines.extend(wrap("Into my own: your own Sounds and Performances, to edit. As a locked library: a library of its own, which Save does not change.", INFO_WIDTH).into_iter().map(line));
        }
        let mut unknown = Vec::new();
        let mut revoked = Vec::new();
        for c in &self.code {
            let what = format!("{} {} ({})", c.name, c.version, c.platform);
            if c.status == "revoked" {
                revoked.push(what);
            } else if c.status != "trusted" {
                unknown.push(format!("{what}{}", if c.status == "unsigned" { ", unsigned".to_string() } else { format!(", signed by unknown key {}", c.key) }));
            }
        }
        // Each plugin a bullet, wrapped (the reference indents its lines by two spaces).
        let bullets = |list: Vec<String>| -> Vec<InfoLine> { list.iter().flat_map(|u| wrap(u, INFO_WIDTH - 2).into_iter().enumerate().map(|(i, l)| warn(if i == 0 { format!("• {l}") } else { l }))).collect() };
        if !unknown.is_empty() {
            lines.extend(wrap("These plugins run code from an unknown source:", INFO_WIDTH).into_iter().map(warn));
            lines.extend(bullets(unknown));
        }
        if !revoked.is_empty() {
            lines.extend(wrap("Signed by a revoked key, so not installed:", INFO_WIDTH).into_iter().map(warn));
            lines.extend(bullets(revoked));
        }
        if !self.code.is_empty() {
            lines.push(head("Plugins"));
            for c in &self.code {
                let s = format!("{} {} ({}): {}{}", c.name, c.version, c.platform, describe_signer(c), if c.status == "revoked" { ", not installed" } else { "" });
                lines.extend(wrap(&s, INFO_WIDTH).into_iter().map(line));
            }
        }
        if !self.license.is_empty() || self.source.is_some() {
            lines.push(head("License"));
            if !self.license.is_empty() {
                lines.extend(wrap(&self.license, INFO_WIDTH).into_iter().map(line));
            }
            if let Some((url, revision)) = &self.source {
                lines.extend(wrap(&format!("Source: {url} at {revision}"), INFO_WIDTH).into_iter().map(line));
            }
        }
        lines
    }

    /// The request that installs it, or why it does not: at least one item must be ticked.
    pub fn request(&self) -> Result<Value, &'static str> {
        let ticked: Vec<&Held> = self.items.iter().filter(|i| i.ticked).collect();
        if ticked.is_empty() && !self.package() {
            return Err("Choose at least one item.");
        }
        // All of it is an empty list.
        let items: Vec<&str> = if ticked.len() == self.items.len() { vec![] } else { ticked.iter().map(|i| i.id.as_str()).collect() };
        Ok(json!({"file": self.file.display().to_string(), "asLibrary": !self.own, "items": items, "acceptUnsignedCode": self.asks}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_read_in_groups_of_four() {
        assert_eq!(fingerprint("0123456789abcdef"), "0123 4567 89ab cdef");
        assert_eq!(fingerprint(""), "");
    }

    #[test]
    fn the_signer_is_described_as_the_reference_words_it() {
        let s = |status: &str, name: &str| Signer { status: status.into(), key: "0123456789abcdef".into(), key_name: name.into(), ..Default::default() };
        assert_eq!(describe_signer(&s("trusted", "Quill")), "Signed by Quill (0123 4567 89ab cdef), a key you trust");
        assert_eq!(describe_signer(&s("unknown", "")), "Signed by key 0123 4567 89ab cdef, not a key you trust");
        assert_eq!(describe_signer(&s("revoked", "Quill")), "Signed by Quill (0123 4567 89ab cdef), which is revoked");
        assert_eq!(describe_signer(&s("unsigned", "")), "Not signed");
    }

    #[test]
    fn a_key_is_shown_short_or_whole_and_chars_never_split() {
        assert_eq!(fingerprint("abcd"), "abcd");
        assert_eq!(fingerprint("abc"), "abc");
        assert_eq!(fingerprint("a"), "a");
        assert_eq!(fingerprint("é章"), "é章");
        assert_eq!(fingerprint("0123456789abc"), "0123 4567 89ab c");
    }

    #[test]
    fn an_empty_contents_answer_defaults_to_every_field_absent() {
        let i = Install::from_json("a.olib".into(), &json!({}));
        assert_eq!(i.kind, "");
        assert_eq!(i.name, "");
        assert!(i.own, "no kind is not a library, so it goes into the user's own");
        assert!(i.items.is_empty());
        assert!(i.code.is_empty());
        assert_eq!(i.signer.status, "");
        assert_eq!(i.license, "");
        assert!(i.source.is_none());
        assert!(!i.asks, "no code means no unknown code");
    }

    #[test]
    fn a_key_or_field_of_the_wrong_shape_reads_as_absent() {
        // A field that is an object where a string is expected, and arrays that are not arrays.
        let i = Install::from_json("a.olib".into(), &json!({
            "kind": {"nested": true}, "name": 7, "license": ["MIT"], "items": "not-a-list", "code": 3,
            "signer": "not-an-object", "source": "not-an-object",
        }));
        assert_eq!(i.kind, "");
        assert_eq!(i.name, "");
        assert_eq!(i.license, "");
        assert!(i.items.is_empty());
        assert!(i.code.is_empty());
        assert_eq!(i.signer.status, "");
        assert!(i.source.is_none());
    }

    #[test]
    fn a_library_kind_goes_locked_and_a_package_stays_own() {
        let library = Install::from_json("l".into(), &json!({"kind": "library"}));
        assert!(!library.own, "a library goes in as a locked library");
        assert!(!library.package());
        let installable = Install::from_json("s".into(), &json!({"kind": "sound"}));
        assert!(installable.own, "anything not a library is the user's own");
        assert!(!installable.package());
    }

    #[test]
    fn package_is_plugin_or_asset_alone() {
        for kind in ["plugin", "asset"] {
            assert!(Install::from_json("f".into(), &json!({"kind": kind})).package(), "{kind}");
        }
        for kind in ["", "library", "sound", "performance", "setlist"] {
            assert!(!Install::from_json("f".into(), &json!({"kind": kind})).package(), "{kind}");
        }
    }

    #[test]
    fn source_is_none_unless_a_source_object_is_there() {
        let none = Install::from_json("f".into(), &json!({"source": "https://x/y"})).source;
        assert!(none.is_none(), "a source as a string, not an object: none");
        let yes = Install::from_json("f".into(), &json!({"source": {"url": "https://x/y", "revision": "abc"}})).source;
        assert_eq!(yes, Some(("https://x/y".into(), "abc".into())));
        let partial = Install::from_json("f".into(), &json!({"source": {"url": "u"}})).source;
        assert_eq!(partial, Some(("u".into(), "".into())), "a missing revision reads empty, not missing");
    }

    #[test]
    fn install_asks_when_any_code_is_neither_trusted_nor_revoked() {
        let with = |status: &str| Install::from_json("f".into(), &json!({"kind": "plugin", "code": [{"status": status}]}));
        assert!(!with("trusted").asks);
        assert!(!with("revoked").asks);
        for status in ["unknown", "unsigned", "", "missing"] {
            assert!(with(status).asks, "{status}");
        }
    }

    #[test]
    fn items_are_ordered_performances_then_sounds_then_setlists_then_the_rest() {
        let i = Install::from_json("f".into(), &json!({"items": [
            {"kind": "setlist", "id": "sl"}, {"kind": "sound", "id": "s"},
            {"kind": "other", "id": "o"}, {"kind": "performance", "id": "p"},
        ]}));
        let kinds: Vec<&str> = i.items.iter().map(|h| h.kind.as_str()).collect();
        assert_eq!(kinds, ["performance", "sound", "setlist", "other"]);
        assert!(i.items.iter().all(|h| h.ticked), "everything is ticked at first, the user unticks what they do not want");
        assert_eq!(i.items[0].id, "p", "the performance, though last in the reply, is listed first");
    }

    #[test]
    fn rows_offer_the_choice_of_where_outside_a_package_and_name_the_untitled() {
        let i = Install::from_json("f".into(), &json!({"kind": "sound", "name": "Pro", "items": [
            {"kind": "sound", "name": ""}, {"kind": "sound", "name": "Kick"},
        ]}));
        let labels: Vec<String> = i.rows().iter().map(|r| r.label.to_string()).collect();
        assert_eq!(labels, ["Into my own, to edit", "As a locked library", "Untitled", "Kick", "Install", "Cancel"]);
        assert!(i.rows()[0].on, "the user's own is chosen for a non-library");
        assert!(!i.rows()[1].on);
        assert!(i.rows()[3].on, "an item is ticked until the user unticks it");
    }

    #[test]
    fn a_package_has_no_choice_of_where_and_unknown_code_renames_install() {
        let p = Install::from_json("f".into(), &json!({"kind": "plugin", "code": [{"status": "unknown", "key": "aa11", "name": "One", "version": "1.0", "platform": "F"}]}));
        let labels: Vec<String> = p.rows().iter().map(|r| r.label.to_string()).collect();
        assert_eq!(labels, ["Install anyway", "Cancel"], "no radio row for a package; unknown code says Install anyway");
        let known = Install::from_json("f".into(), &json!({"kind": "plugin", "code": [{"status": "trusted"}]}));
        let known_labels: Vec<String> = known.rows().iter().map(|r| r.label.to_string()).collect();
        assert_eq!(known_labels, ["Install", "Cancel"]);
    }

    #[test]
    fn info_says_not_signed_and_names_the_archive_when_it_has_one() {
        let anon = Install::from_json("a.olib".into(), &json!({}));
        let anon_info = anon.info();
        assert_eq!(anon_info[0].text, "Install an archive");
        assert!(anon_info.iter().any(|l| l.text.as_str() == "Not signed"), "no signer says Not signed");
        assert!(!anon_info.iter().any(|l| l.text.as_str() == "Plugins"), "no code, no Plugins head");
        let named = Install::from_json("a.olib".into(), &json!({"name": "Hero Pack"}));
        assert_eq!(named.info()[0].text, "Install Hero Pack");
    }

    #[test]
    fn info_warns_on_unknown_and_revoked_code_and_lists_the_plugins() {
        let i = Install::from_json("a.olib".into(), &json!({
            "signer": {"status": "trusted", "key_name": "Quill", "key": "0123456789abcdef"},
            "code": [
                {"status": "unknown", "key": "aa11", "name": "One", "version": "1.0", "platform": "FANTOM"},
                {"status": "revoked", "key": "bb22", "name": "Two", "version": "2.0", "platform": "FANTOM"},
                {"status": "unsigned", "key": "", "name": "Three", "version": "3.0", "platform": "FANTOM"},
            ],
            "license": "MIT",
        }));
        let info = i.info();
        let texts: Vec<&str> = info.iter().map(|l| l.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.contains("a key you trust")), "the signer is described as trusted");
        assert!(texts.iter().any(|t| t.starts_with("These plugins")), "the unknown source is warned");
        assert!(texts.iter().any(|t| t.contains("One 1.0 (FANTOM)")), "the unknown build is named");
        assert!(texts.iter().any(|t| t.contains("unknown key aa11")), "the unknown build names its key");
        assert!(texts.iter().any(|t| t.contains("Three 3.0 (FANTOM)")), "the unsigned build is named");
        assert!(texts.iter().any(|t| t.contains("unsigned")), "an unsigned build says so");
        assert!(texts.iter().any(|t| t.starts_with("Signed by a revoked key")), "the revoked key is warned");
        assert!(texts.iter().any(|t| t.contains("Two 2.0 (FANTOM)")), "the revoked build is singled out");
        assert!(texts.contains(&"Plugins"), "the code is listed under a Plugins head");
        assert!(texts.contains(&"License"), "a license is headed");
    }

    #[test]
    fn request_sends_all_of_it_as_an_empty_list_when_everything_is_chosen() {
        let i = Install::from_json("my.oins".into(), &json!({"kind": "sound", "items": [{"id": "a"}, {"id": "b"}]}));
        let r = i.request().unwrap();
        assert_eq!(r["file"], "my.oins");
        assert_eq!(r["asLibrary"], json!(false));
        assert_eq!(r["items"], json!([]), "all items chosen is sent as the empty list the engine takes as everything");
        assert_eq!(r["acceptUnsignedCode"], json!(false));
    }

    #[test]
    fn request_refuses_when_nothing_is_ticked_but_a_package_never_trips_it() {
        let mut i = Install::from_json("f".into(), &json!({"kind": "sound", "items": [{"id": "a"}, {"id": "b"}]}));
        i.items[0].ticked = false;
        i.items[1].ticked = false;
        assert_eq!(i.request(), Err("Choose at least one item."));
        let p = Install::from_json("f".into(), &json!({"kind": "plugin"}));
        assert!(p.request().is_ok(), "a package holds no items, so nothing to choose is not a refusal");
    }

    #[test]
    fn request_sends_the_ticked_items_only() {
        let mut i = Install::from_json("f".into(), &json!({"kind": "sound", "items": [{"id": "a"}, {"id": "b"}, {"id": "c"}]}));
        i.items[1].ticked = false;
        let r = i.request().unwrap();
        assert_eq!(r["items"], json!(["a", "c"]), "only the ticked ids, in their order");
        assert_eq!(r["asLibrary"], json!(false));
    }

    #[test]
    fn request_sends_as_library_for_a_library_and_accept_unsigned_when_asked() {
        let lib = Install::from_json("f".into(), &json!({"kind": "library", "items": [{"id": "a"}]}));
        let r = lib.request().unwrap();
        assert_eq!(r["asLibrary"], json!(true));
        assert_eq!(r["acceptUnsignedCode"], json!(false));
        let asks = Install::from_json("f".into(), &json!({"kind": "sound", "items": [{"id": "a"}], "code": [{"status": "unknown", "key": "aa11", "name": "One", "version": "1", "platform": "F"}]}));
        assert_eq!(asks.request().unwrap()["acceptUnsignedCode"], json!(true));
    }
}
