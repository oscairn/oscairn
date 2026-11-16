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
            source: v["source"].as_object().map(|s| (s["url"].as_str().unwrap_or_default().to_string(), s["revision"].as_str().unwrap_or_default().to_string())),
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
            rows.push(radio("Into my Sounds and Performances, to edit", self.own));
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
        lines.extend(wrap(&describe_signer(&self.signer), 38).into_iter().map(line));
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
        let bullets = |list: Vec<String>| -> Vec<InfoLine> { list.iter().flat_map(|u| wrap(u, 36).into_iter().enumerate().map(|(i, l)| warn(if i == 0 { format!("• {l}") } else { l }))).collect() };
        if !unknown.is_empty() {
            lines.extend(wrap("These plugins run code from an unknown source:", 38).into_iter().map(warn));
            lines.extend(bullets(unknown));
        }
        if !revoked.is_empty() {
            lines.extend(wrap("Signed by a revoked key, so not installed:", 38).into_iter().map(warn));
            lines.extend(bullets(revoked));
        }
        if !self.code.is_empty() {
            lines.push(head("Plugins"));
            for c in &self.code {
                let s = format!("{} {} ({}): {}{}", c.name, c.version, c.platform, describe_signer(c), if c.status == "revoked" { ", not installed" } else { "" });
                lines.extend(wrap(&s, 38).into_iter().map(line));
            }
        }
        if !self.license.is_empty() || self.source.is_some() {
            lines.push(head("License"));
            if !self.license.is_empty() {
                lines.extend(wrap(&self.license, 38).into_iter().map(line));
            }
            if let Some((url, revision)) = &self.source {
                lines.extend(wrap(&format!("Source: {url} at {revision}"), 38).into_iter().map(line));
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
        assert_eq!(describe_signer(&s("trusted", "Ada")), "Signed by Ada (0123 4567 89ab cdef), a key you trust");
        assert_eq!(describe_signer(&s("unknown", "")), "Signed by key 0123 4567 89ab cdef, not a key you trust");
        assert_eq!(describe_signer(&s("revoked", "Ada")), "Signed by Ada (0123 4567 89ab cdef), which is revoked");
        assert_eq!(describe_signer(&s("unsigned", "")), "Not signed");
    }
}
