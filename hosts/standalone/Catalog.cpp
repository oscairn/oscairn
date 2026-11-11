// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Catalog.h"
#include "Settings.h"
#include "oscairn/repository.h"

namespace oscairn::standalone
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        const juce::String separator = juce::String::fromUTF8("  ·  ");
        juce::String bytes(uint64_t n) { return juce::File::descriptionOfSizeInBytes(int64_t(n)); }
    }

    bool Offer::holds(const std::string& kind) const
    {
        for(const auto& [type, n] : version.types)
            if(n && (kind == "plugin" ? type == "clap" || type == "lv2" : type == "oscairn." + kind)) return true;
        return false;
    }

    std::vector<Offer> offers(const std::vector<std::pair<std::string, Control::PackageJob>>& listings, const std::map<std::string, std::string>& installed)
    {
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
                    if(v.incompatible.empty() && v.yanked.empty() && (v.channel.empty() || v.channel == "stable") && (!pick || repository::older(pick->version, v.version))) pick = &v;
                }
                Offer o{source, listing.name, p, pick ? *pick : *newest, pick != nullptr};
                // Installed at a version not known (a library installed before installs were recorded): no update can be told.
                if(const auto at = installed.find(p.id); at != installed.end())
                {
                    o.installed = at->second;
                    o.update = pick && !at->second.empty() && repository::older(at->second, pick->version);
                }
                out.push_back(std::move(o));
            }
        std::stable_sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return text(a.package.name).compareIgnoreCase(text(b.package.name)) < 0; });
        return out;
    }

    Catalog::Catalog(Engine& engine, const std::string& kind, std::map<std::string, std::string> installed) : engine_(engine), kind_(kind), installed_(std::move(installed))
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
        readNext(std::move(on));
    }

    void Catalog::readNext(std::vector<Control::Repository> left)
    {
        if(left.empty())
        {
            reading_ = false;
            all_ = offers(listings_, installed_);
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
                    if(state.state == "done") { safe->listings_.push_back({r.address, state}); safe->read_.add(text(state.name.empty() ? r.name : state.name)); }
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
        install.setButtonText(o.update ? "Update to " + text(o.version.version) : juce::String("Install"));
        install.setEnabled(o.installable);
        install.setTooltip(o.installable ? juce::String() : text(o.version.incompatible.empty() ? "No version this device can install" : o.version.incompatible));
        for(auto* c : std::initializer_list<juce::Component*>{&performances, &sounds, &plugins, &search, &list}) c->setVisible(false);
        backButton.setVisible(true); install.setVisible(!o.installed || o.update); // installed and current: nothing to install
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
        if(o.installed) out.insert(out.begin() + 2, {"Installed", o.installed->empty() ? juce::String("Its version not known") : text(*o.installed)});
        if(!o.installable) out.push_back({"On this device", text(v.incompatible.empty() ? "no version it can install" : v.incompatible)});
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
        const auto about = text(o.package.publisher) + (o.package.summary.empty() ? juce::String() : separator + text(o.package.summary));
        const auto line = o.update ? "Update to " + text(o.version.version) + separator + about
                        : o.installed ? "Installed" + separator + about
                        : o.installable ? about
                                        : "Cannot be installed here: " + text(o.version.incompatible.empty() ? "no version it can install" : o.version.incompatible);
        g.drawText(line, area, juce::Justification::topLeft, true);
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
