// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Catalog.h"
#include "SigningKeys.h"
#include "oscairn/discovery.h"
#include "oscairn/repository.h"

namespace oscairn::standalone
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        const juce::String separator = juce::String::fromUTF8("  ·  ");
        juce::String bytes(uint64_t n) { return juce::File::descriptionOfSizeInBytes(int64_t(n)); }
        // Why a version cannot be installed here: this device, or a revocation list withdrawing it.
        juce::String whyNot(const Control::RepositoryVersion& v)
        {
            if(!v.incompatible.empty()) return text(v.incompatible);
            if(!v.withdrawn.reason.empty()) return "withdrawn by " + text(v.withdrawn.repository) + ": " + text(v.withdrawn.reason);
            return "no version it can install";
        }
    }

    juce::String withdrawnSaying(const Offer& o)
    {
        const auto& w = o.record.withdrawn;
        if(w.reason.empty()) return {};
        return text(o.package.name.empty() ? o.package.id : o.package.name) + " " + text(o.record.version) + " was withdrawn by " + text(w.repository) + ": " + text(w.reason);
    }

    bool Offer::holds(const std::string& kind) const
    {
        for(const auto& [type, n] : version.types)
            if(n && (kind == "plugin" ? type == "clap" || type == "lv2" : type == "oscairn." + kind)) return true;
        return false;
    }

    std::vector<Offer> offers(const std::vector<std::pair<std::string, Control::PackageJob>>& listings, const std::map<std::string, Control::InstalledPackage>& installed,
                              const std::map<std::string, std::string>& bundled)
    {
        // The plugins a package holds: its ID's, or those named here for a package holding several.
        // shortcut: the index lists no plugin IDs, so a package of several is named here; when it lists them, use those.
        const std::map<std::string, std::vector<std::string>> several{{"org.oscairn.splitters", {"org.oscairn.split", "org.oscairn.join"}}};
        std::vector<Offer> out;
        for(const auto& [source, listing] : listings)
            for(const auto& p : listing.packages)
            {
                if(p.versions.empty()) continue;
                // The version the engine would install; else the newest, to say why not.
                const Control::RepositoryVersion *pick = nullptr, *newest = nullptr;
                for(const auto& v : p.versions)
                {
                    if(!newest || repository::older(newest->version, v.version)) newest = &v;
                    if(v.incompatible.empty() && v.yanked.empty() && v.withdrawn.reason.empty() && (v.channel.empty() || v.channel == "stable") && (!pick || repository::older(pick->version, v.version))) pick = &v;
                }
                Offer o{source, listing.name, p, pick ? *pick : *newest, pick != nullptr};
                // Installed at a version not known (a library installed before installs were recorded): no update can be told.
                if(const auto at = installed.find(p.id); at != installed.end())
                {
                    const auto& r = at->second;
                    o.installed = r.version; o.record = r;
                    o.update = pick && !r.version.empty() && repository::older(r.version, pick->version);
                    // "Installed" only as this repository builds it (decided 2026-11-11).
                    for(const auto& v : p.versions)
                        if(!o.update && !r.version.empty() && v.version == r.version && (r.otherBuild || (!r.sha256.empty() && !v.sha256.empty() && r.sha256 != v.sha256)))
                        {
                            o.otherBuild = true; o.version = v; o.installable = v.incompatible.empty() && v.withdrawn.reason.empty();
                        }
                }
                else if(o.holds("plugin"))
                {
                    const auto named = several.find(p.id);
                    std::optional<std::string> oldest;
                    bool all = true;
                    for(const auto& id : named == several.end() ? std::vector<std::string>{p.id} : named->second)
                        if(const auto b = bundled.find(id); b == bundled.end()) all = false;
                        else if(!oldest || repository::older(b->second, *oldest)) oldest = b->second;
                    if(all)
                    {
                        o.builtIn = true; o.installed = oldest;
                        o.update = pick && repository::older(*oldest, pick->version);
                    }
                }
                out.push_back(std::move(o));
            }
        std::stable_sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return text(a.package.name).compareIgnoreCase(text(b.package.name)) < 0; });
        return out;
    }

    juce::String describe(const Offer& o)
    {
        const auto about = text(o.package.publisher) + (o.package.summary.empty() ? juce::String() : separator + text(o.package.summary));
        // What is installed and withdrawn since says so first, in place of the rest: nothing else is done about it.
        if(o.installed && !o.record.withdrawn.reason.empty())
            return "Withdrawn by " + text(o.record.withdrawn.repository) + ": " + text(o.record.withdrawn.reason) + (o.update ? separator + "Update to " + text(o.version.version) : juce::String());
        return o.update ? "Update to " + text(o.version.version) + separator + about
             : o.builtIn ? "Built in" + separator + about
             : o.otherBuild ? "Installed (a different build)" + separator + about
             : o.installed ? "Installed" + separator + about
             : o.installable ? about
                             : "Cannot be installed here: " + whyNot(o.version);
    }

    juce::String replaceQuestion(const Offer& o)
    {
        auto archive = [](const std::string& sha) { return "archive " + text(sha.substr(0, 16)); };
        juce::StringArray signers;
        for(const auto& k : o.record.signers) signers.add(fingerprint(k));
        const auto name = text(o.package.name.empty() ? o.package.id : o.package.name) + " " + text(o.version.version);
        const auto installed = o.record.sha256.empty() ? juce::String("a build installed some other way, its archive not known")
                                                       : archive(o.record.sha256) + (signers.isEmpty() ? juce::String(", unsigned") : ", signed by key " + signers.joinIntoString(", "));
        return name + " is installed here, but it is not the build " + text(o.repository) + " has.\n\nInstalled: " + installed + "\n" + text(o.repository) + "'s: " +
               archive(o.version.sha256) + ", signed by " + text(o.package.publisher) + ", key " + fingerprint(o.package.publisherKey) +
               "\n\nInstalling the repository's build replaces the installed one.";
    }

    CodeQuestion codeQuestion(const Control::PackageJob& refused, const std::string& publisher, const std::string& publisherKey)
    {
        juce::StringArray all, unsigned_, strangers; // every build refused; those unsigned; the keys no one vouches for
        std::vector<std::string> keys;
        for(const auto& c : refused.installed.code)
        {
            if(c.status != "unknown" && c.status != "unsigned") continue; // trusted, or revoked (never installed)
            all.addIfNotAlreadyThere(text(c.name) + " " + text(c.version));
            if(c.status == "unsigned") unsigned_.addIfNotAlreadyThere(text(c.name) + " " + text(c.version));
            else if(publisherKey.empty() || c.key != publisherKey) strangers.addIfNotAlreadyThere("key " + fingerprint(c.key));
            if(c.status == "unknown" && !c.publicKey.empty() && std::find(keys.begin(), keys.end(), c.publicKey) == keys.end()) keys.push_back(c.publicKey);
        }
        const auto who = text(publisher.empty() ? std::string("this publisher") : publisher), repository = text(refused.name.empty() ? std::string("this repository") : refused.name);
        const auto code = "The plugin code in this package (" + all.joinIntoString(", ") + ")";
        const juce::String risk = "\n\nA plugin runs as a program on this device, with your access to its files and network. Install it only if you know where it came from.";
        CodeQuestion q;
        if(!keys.empty() && unsigned_.isEmpty() && strangers.isEmpty())
        {
            q.title = "Trust " + who + " to install plugins?";
            q.message = code + " is signed by " + who + ". The repository " + repository + " lists this key as " + who + "'s.\n\nKey fingerprint: " + fingerprint(publisherKey) +
                        "\n\nOnce trusted, plugins signed by this key install without asking.";
            q.buttons = {"Trust and install", "Cancel"}; q.trust = 1; q.keys = keys;
        }
        else if(unsigned_.isEmpty())
        {
            q.title = "Install code from an unknown key?";
            q.message = code + " is signed by " + strangers.joinIntoString(", ") + ", which " + repository + " does not list as " + who + "'s and this device does not trust. Anyone could have made it." + risk;
            q.buttons = {keys.size() > 1 ? "Trust these keys" : "Trust this key", "Install anyway", "Cancel"}; q.trust = 1; q.anyway = 2; q.keys = keys;
        }
        else
        {
            q.title = "Install unsigned code?";
            q.message = unsigned_.joinIntoString(", ") + " in this package is not signed: nothing says who made it, or that it is unchanged." +
                        (strangers.isEmpty() ? juce::String() : " The rest is signed by " + strangers.joinIntoString(", ") + ", which no one here vouches for.") + risk;
            q.buttons = {"Install anyway", "Cancel"}; q.anyway = 1;
        }
        return q;
    }

    Catalog::Catalog(Engine& engine, const std::string& kind, std::map<std::string, Control::InstalledPackage> installed, std::string scanner)
        : engine_(engine), kind_(kind), installed_(std::move(installed)), scanner_(std::move(scanner))
    {
        setTitle("Catalog");
        for(auto* b : {&performances, &sounds, &plugins}) { b->setClickingTogglesState(true); b->setRadioGroupId(1); }
        performances.onClick = [this] { showKind("performance"); };
        sounds.onClick = [this] { showKind("sound"); };
        plugins.onClick = [this] { showKind("plugin"); };
        search.setJustification(juce::Justification::centredLeft); search.setIndents(10, 0);
        search.setTextToShowWhenEmpty("Search by name, tag or publisher", ink(Ink::Muted));
        search.onTextChange = [this] { filter(); };
        backButton.onClick = [this] { back(); };
        install.onClick = [this] { if(opened_ && opened_->offered() && onInstall) onInstall(*opened_); };
        list.setOutlineThickness(0);
        for(auto* c : std::initializer_list<juce::Component*>{&performances, &sounds, &plugins, &search, &list, &close}) addAndMakeVisible(c);
        addChildComponent(backButton); addChildComponent(install);
        showKind(kind);
        setSize(820, 560);
    }

    void Catalog::showKind(const std::string& kind)
    {
        kind_ = kind;
        performances.setToggleState(kind == "performance", juce::dontSendNotification);
        sounds.setToggleState(kind == "sound", juce::dontSendNotification);
        plugins.setToggleState(kind == "plugin", juce::dontSendNotification);
        filter();
    }

    void Catalog::filter()
    {
        const auto words = juce::StringArray::fromTokens(search.getText(), true);
        shown_.clear();
        for(const auto& o : all_)
        {
            if(!o.holds(kind_)) continue;
            juce::String haystack = text(o.package.name + " " + o.package.id + " " + o.package.summary + " " + o.package.publisher);
            for(const auto& t : o.package.tags) haystack << " " << text(t);
            if(std::all_of(words.begin(), words.end(), [&](const auto& w) { return haystack.containsIgnoreCase(w); })) shown_.push_back(o);
        }
        list.updateContent(); list.repaint();
        repaint();
    }

    void Catalog::read()
    {
        std::vector<Control::Repository> on;
        if(const auto error = engine_.packages([&](Control& c) { for(const auto& r : c.repositories()) if(r.enabled) on.push_back(r); }); !error.empty())
        {
            status_ = text(error); repaint(); return;
        }
        listings_.clear(); read_.clear(); failed_.clear();
        if(on.empty()) { status_ = "No repository is on. Settings lists them."; repaint(); return; }
        reading_ = true;
        // The app's plugins, by the isolated scanner, off this thread; a scan that fails finds none.
        if(!scanner_.empty() && !scan_.valid())
            scan_ = std::async(std::launch::async, [dir = engine_.pluginDir(), helper = scanner_, canceled = &canceled_] {
                std::map<std::string, std::string> found;
                try { for(const auto& d : scanPlugins({dir}, helper, canceled).plugins) found[d.id] = d.version; } catch(const std::exception&) {}
                return found;
            });
        readNext(std::move(on));
    }

    void Catalog::readNext(std::vector<Control::Repository> left)
    {
        if(left.empty())
        {
            if(scan_.valid())
            {
                if(scan_.wait_for(std::chrono::seconds(0)) != std::future_status::ready)
                {
                    juce::Timer::callAfterDelay(100, [safe = juce::Component::SafePointer<Catalog>(this)] { if(safe) safe->readNext({}); });
                    return;
                }
                bundled_ = scan_.get();
            }
            reading_ = false;
            all_ = offers(listings_, installed_, bundled_);
            status_ = read_.isEmpty() ? juce::String() : "From " + read_.joinIntoString(", ") + ".";
            if(!failed_.isEmpty()) status_ << (status_.isEmpty() ? "" : " ") << "Not read: " << failed_.joinIntoString("; ") << ".";
            filter();
            return;
        }
        const auto r = left.front();
        left.erase(left.begin());
        status_ = "Reading " + text(r.name) + "...";
        repaint();
        uint64_t job = 0;
        if(const auto error = engine_.packages([&](Control& c) { job = c.listRepository(r.address); }); !error.empty())
        {
            failed_.add(text(r.name) + " (" + text(error) + ")");
            readNext(std::move(left));
            return;
        }
        // Asked about every 100 ms until it ends, off the engine's thread as every package job is.
        struct Poll
        {
            static void again(juce::Component::SafePointer<Catalog> safe, uint64_t job, Control::Repository r, std::vector<Control::Repository> left)
            {
                juce::Timer::callAfterDelay(100, [safe, job, r, left]() mutable {
                    if(!safe) return;
                    Control::PackageJob state;
                    if(const auto error = safe->engine_.packages([&](Control& c) { state = c.packageJob(job); }); !error.empty()) { state.state = "failed"; state.error = error; }
                    if(state.state == "running") { again(safe, job, r, std::move(left)); return; }
                    if(state.state == "done") { state.name = r.name; safe->listings_.push_back({r.address, state}); safe->read_.add(text(r.name)); } // this device's name for it, never the one its root claims
                    else safe->failed_.add(text(r.name) + " (" + text(state.error) + ")");
                    safe->readNext(std::move(left));
                });
            }
        };
        Poll::again(this, job, r, std::move(left));
    }

    void Catalog::open(int row)
    {
        if(row < 0 || size_t(row) >= shown_.size()) return;
        opened_ = shown_[size_t(row)];
        const auto& o = *opened_;
        install.setButtonText(o.update ? "Update to " + text(o.version.version) : o.otherBuild ? juce::String("Install the repository's build") : juce::String("Install"));
        install.setEnabled(o.installable);
        install.setTooltip(o.installable ? juce::String() : whyNot(o.version));
        for(auto* c : std::initializer_list<juce::Component*>{&performances, &sounds, &plugins, &search, &list}) c->setVisible(false);
        backButton.setVisible(true); install.setVisible(!o.installed || o.update || o.otherBuild); // installed and current: nothing to install
        resized(); repaint();
    }

    void Catalog::back()
    {
        opened_.reset();
        for(auto* c : std::initializer_list<juce::Component*>{&performances, &sounds, &plugins, &search, &list}) c->setVisible(true);
        backButton.setVisible(false); install.setVisible(false);
        resized(); repaint();
    }

    std::vector<std::pair<juce::String, juce::String>> Catalog::facts() const
    {
        if(!opened_) return {};
        const auto& o = *opened_;
        const auto& v = o.version;
        juce::StringArray needs;
        for(const auto& [id, least] : v.dependencies) needs.add(text(id) + " " + text(least) + " or later");
        std::vector<std::pair<juce::String, juce::String>> out{
            {"Summary", text(o.package.summary)},
            {"Version", text(v.version) + (v.channel.empty() || v.channel == "stable" ? juce::String() : " (" + text(v.channel) + ")") + (v.yanked.empty() ? juce::String() : ", yanked: " + text(v.yanked))},
            {"License", o.package.license.empty() ? juce::String("Not stated") : text(o.package.license)},
            {"Publisher", text(o.package.publisher) + ", key " + fingerprint(o.package.publisherKey)},
            {"Repository", text(o.repository)},
            {"Size", bytes(v.size) + " to download, " + bytes(v.installed) + " installed"},
            {"Needs", needs.isEmpty() ? juce::String("Nothing else") : needs.joinIntoString(", ")}};
        if(o.installed)
            out.insert(out.begin() + 2, {"Installed", o.installed->empty() ? juce::String("Its version not known")
                                                      : text(*o.installed) + (o.builtIn ? ", built in: it comes with the app"
                                                                              : o.otherBuild ? ", a different build: it does not match this repository's" : "")});
        if(!o.installable) out.push_back({"On this device", whyNot(v)});
        // Said, and what the user can do about it; never removed or disabled for them.
        if(const auto said = withdrawnSaying(o); said.isNotEmpty())
            out.insert(out.begin(), {"Withdrawn", said + (o.update ? ". Update to " + text(v.version) + " replaces it" : juce::String()) +
                                                      ". To remove it, use the Library's list of " + (o.record.kind == "plugin" ? "plugins" : o.record.kind == "asset" ? "asset libraries" : "libraries") + "."});
        return out;
    }

    void Catalog::paintListBoxItem(int row, juce::Graphics& g, int width, int height, bool selected)
    {
        if(row < 0 || size_t(row) >= shown_.size()) return;
        const auto& o = shown_[size_t(row)];
        const int m = mode();
        drawListRow(g, {0, 0, width, height}, row, juce::Colours::transparentBlack);
        if(selected) { g.setColour(ink(Ink::Accent).withAlpha(.18f)); g.fillRect(0, 0, width, height); }
        auto area = juce::Rectangle<int>(0, 0, width, height).reduced(16, 4);
        auto top = area.removeFromTop(area.getHeight() / 2 + 2);
        // What this device cannot install is grayed, and says why; what is installed says so, with its version.
        g.setColour(o.installable || o.installed ? ink(Ink::Text) : ink(Ink::Muted));
        g.setFont(juce::FontOptions(textSize(TextRole::Body, m), juce::Font::bold));
        g.drawText(text(o.package.name.empty() ? o.package.id : o.package.name) + "  " + text(o.installed ? *o.installed : o.version.version), top, juce::Justification::bottomLeft, true);
        g.setColour(ink(Ink::Muted)); g.setFont(juce::FontOptions(textSize(TextRole::Small, m)));
        g.drawText(describe(o), area, juce::Justification::topLeft, true);
    }

    void Catalog::resized()
    {
        const bool touch = mode() == 2;
        const int control = touch ? 44 : 30, gap = touch ? 10 : 8;
        list.setRowHeight(touch ? 64 : 48);
        auto area = getLocalBounds().reduced(16, 12);
        auto bar = area.removeFromTop(control);
        headingArea_ = {};
        if(opened_)
        {
            backButton.setBounds(bar.removeFromLeft(touch ? 110 : 80)); bar.removeFromLeft(gap);
            headingArea_ = bar;
        }
        else
        {
            plugins.setBounds(bar.removeFromRight(touch ? 110 : 80)); bar.removeFromRight(gap / 2);
            sounds.setBounds(bar.removeFromRight(touch ? 110 : 80)); bar.removeFromRight(gap / 2);
            performances.setBounds(bar.removeFromRight(touch ? 150 : 120)); bar.removeFromRight(gap);
            search.setBounds(bar);
            search.setFont(juce::FontOptions(textSize(TextRole::Body, mode())));
        }
        area.removeFromTop(gap);
        auto bottom = area.removeFromBottom(control);
        area.removeFromBottom(gap);
        close.setBounds(bottom.removeFromRight(touch ? 120 : 100)); bottom.removeFromRight(gap);
        if(opened_) { install.setBounds(bottom.removeFromRight(std::max(touch ? 130 : 110, install.getBestWidthForHeight(control)))); bottom.removeFromRight(gap); }
        statusArea_ = bottom;
        list.setBounds(area);
    }

    void Catalog::paint(juce::Graphics& g)
    {
        g.fillAll(ink(Ink::Background));
        list.setColour(juce::ListBox::backgroundColourId, ink(Ink::Surface));
        const int m = mode();
        const juce::Font title(juce::FontOptions(textSize(TextRole::Title, m), juce::Font::bold)), body(juce::FontOptions(textSize(TextRole::Body, m))),
                         label(juce::FontOptions(textSize(TextRole::Small, m), juce::Font::bold)), small(juce::FontOptions(textSize(TextRole::Small, m)));
        g.setColour(ink(Ink::Muted));
        drawInked(g, status_, statusArea_, small);
        if(!opened_)
        {
            if(shown_.empty() && !reading_) drawInked(g, "Nothing to show here.", list.getBounds().reduced(16, 12).removeFromTop(30), small);
            return;
        }
        g.setColour(ink(Ink::Text));
        drawInked(g, text(opened_->package.name.empty() ? opened_->package.id : opened_->package.name), headingArea_, title);
        // Each fact a row: its name in a column, the value beside it.
        auto area = list.getBounds();
        drawSection(g, area, ink(Ink::Surface));
        area = area.reduced(16, 8);
        const int row = m == 2 ? 44 : 32, column = m == 2 ? 170 : 130;
        for(const auto& [name, value] : facts())
        {
            auto r = area.removeFromTop(row);
            g.setColour(ink(Ink::Muted)); drawInked(g, name, r.removeFromLeft(column), label);
            g.setColour(ink(Ink::Text)); drawInked(g, value, r, body);
        }
    }
}
