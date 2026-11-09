// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Settings.h"
#include "oscairn/signing.h"

namespace oscairn::standalone
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
    }

    juce::String fingerprint(const std::string& id)
    {
        juce::String out;
        for(size_t i = 0; i < id.size(); i += 4) out << (i ? " " : "") << text(id.substr(i, 4));
        return out;
    }

    void askToMakeKey(Engine& engine, juce::Component* over, std::function<void(const std::string&)> made)
    {
        auto* w = new juce::AlertWindow("Make a signing key", "Its name, which only you see: letters, digits, '.', '-' and '_'. The private key stays on this device.", juce::MessageBoxIconType::NoIcon, over);
        w->addTextEditor("name", "", "Name");
        w->addButton("Make", 1, juce::KeyPress(juce::KeyPress::returnKey)); w->addButton("Cancel", 0, juce::KeyPress(juce::KeyPress::escapeKey));
        w->enterModalState(true, juce::ModalCallbackFunction::create([&engine, w, then = std::move(made)](int result) {
            if(result != 1) return;
            Control::SigningKey key;
            if(const auto error = engine.makeSigningKey(w->getTextEditorContents("name").trim().toStdString(), key); !error.empty())
            {
                juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "No key made", text(error));
                return;
            }
            if(then) then(key.name);
        }), true);
    }

    Settings::Settings(Engine& engine) : engine_(engine)
    {
        setTitle("Settings");
        makeKey_.setTooltip("A new key to sign exports with; its private half never leaves this device");
        makeKey_.onClick = [this] { askToMakeKey(engine_, this, [safe = juce::Component::SafePointer<Settings>(this)](const std::string&) { if(safe) safe->refresh(); }); };
        addRepository_.setTooltip("A repository by its address; its root key is trusted only once you have checked its fingerprint");
        addRepository_.onClick = [this] { addRepository(); };
        addAndMakeVisible(makeKey_); addAndMakeVisible(addRepository_);
        refresh();
    }

    RepositoryAddress::RepositoryAddress()
    {
        address.setJustification(juce::Justification::centredLeft); address.setIndents(10, 0);
        address.setTextToShowWhenEmpty("https://", ink(Ink::Muted));
        address.onReturnKey = [this] { add.triggerClick(); };
        for(auto* c : std::initializer_list<juce::Component*>{&address, &add, &folder, &cancel}) addAndMakeVisible(c);
        setSize(560, 190);
    }

    void RepositoryAddress::resized()
    {
        const int m = std::max(1, desktopMode(*this));
        const int control = m == 2 ? 44 : 34, gap = m == 2 ? 10 : 8;
        auto area = getLocalBounds().reduced(20, 16);
        area.removeFromTop(int(textSize(TextRole::Body, m) * 2.6f) + gap);
        address.setBounds(area.removeFromTop(control));
        address.setFont(juce::FontOptions(textSize(TextRole::Body, m)));
        auto bar = area.removeFromBottom(control);
        add.setBounds(bar.removeFromRight(m == 2 ? 110 : 90)); bar.removeFromRight(gap);
        cancel.setBounds(bar.removeFromRight(m == 2 ? 110 : 90));
        folder.setBounds(bar.removeFromLeft(m == 2 ? 180 : 150));
    }

    void RepositoryAddress::paint(juce::Graphics& g)
    {
        g.fillAll(ink(Ink::Background));
        const int m = std::max(1, desktopMode(*this));
        g.setColour(ink(Ink::Text));
        g.setFont(juce::FontOptions(textSize(TextRole::Body, m)));
        g.drawFittedText("Its https:// address, or the folder holding its repository.json. Its root key's fingerprint is shown before anything is trusted.",
                         getLocalBounds().reduced(20, 16).removeFromTop(int(textSize(TextRole::Body, m) * 2.6f)), juce::Justification::topLeft, 2, 1.f);
    }

    void Settings::readRepositories()
    {
        repositories_.clear(); switches_.clear(); removes_.clear();
        if(!engine_.packages([&](Control& c) { repositories_ = c.repositories(); }).empty()) repositories_.clear();
        for(const auto& r : repositories_)
        {
            auto on = std::make_unique<juce::ToggleButton>("On");
            on->setTitle("On: " + text(r.name));
            on->setToggleState(r.enabled, juce::dontSendNotification);
            on->setTooltip(r.origin == "official" ? "Oscairn's own repository; it can be turned off, never removed" : "Whether the Catalog shows what it offers");
            on->onClick = [this, address = r.address, raw = on.get()] {
                const auto error = engine_.packages([&](Control& c) { c.setRepositoryEnabled(address, raw->getToggleState()); });
                if(!error.empty()) juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "Not changed", text(error), {}, nullptr, juce::ModalCallbackFunction::create([](int) {}));
                juce::MessageManager::callAsync([safe = juce::Component::SafePointer<Settings>(this)] { if(safe) safe->refresh(); }); // not while its button calls
            };
            addAndMakeVisible(*on);
            switches_.push_back(std::move(on));
            std::unique_ptr<juce::TextButton> remove;
            if(r.origin == "user")
            {
                remove = std::make_unique<juce::TextButton>("Remove");
                remove->setTitle("Remove " + text(r.name));
                remove->onClick = [this, r] {
                    auto options = juce::MessageBoxOptions::makeOptionsOkCancel(juce::MessageBoxIconType::QuestionIcon, "Remove " + text(r.name) + "?",
                        "What you installed from it stays. Its root key is no longer trusted for repositories.", "Remove", "Cancel", this);
                    juce::AlertWindow::showAsync(options, [safe = juce::Component::SafePointer<Settings>(this), address = r.address](int chosen) {
                        if(!safe || chosen != 1) return;
                        if(const auto error = safe->engine_.packages([&](Control& c) { c.removeRepository(address); }); !error.empty())
                            juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "Not removed", text(error), {}, nullptr, juce::ModalCallbackFunction::create([](int) {}));
                        safe->refresh();
                    });
                };
                addAndMakeVisible(*remove);
            }
            removes_.push_back(std::move(remove));
        }
    }

    void Settings::addRepository()
    {
        auto* content = new RepositoryAddress();
        setDesktopMode(*content, mode());
        juce::DialogWindow::LaunchOptions options;
        options.dialogTitle = "Add a repository";
        options.content.setOwned(content); options.componentToCentreAround = this; options.useNativeTitleBar = true; options.resizable = false;
        options.dialogBackgroundColour = ink(Ink::Background);
        auto* dialog = options.launchAsync();
        content->cancel.onClick = [dialog] { dialog->exitModalState(0); };
        content->add.onClick = [safe = juce::Component::SafePointer<Settings>(this), dialog, content] {
            const auto address = content->address.getText().trim();
            dialog->exitModalState(0);
            if(safe) safe->openRepository(address);
        };
        content->folder.onClick = [safe = juce::Component::SafePointer<Settings>(this), dialog] {
            dialog->exitModalState(0);
            if(!safe) return;
            safe->chooser_ = std::make_unique<juce::FileChooser>("A package repository: the folder holding repository.json", juce::File());
            safe->chooser_->launchAsync(juce::FileBrowserComponent::openMode | juce::FileBrowserComponent::canSelectDirectories, [safe](const juce::FileChooser& fc) {
                if(safe && fc.getResult() != juce::File()) safe->openRepository(fc.getResult().getFullPathName());
            });
        };
    }

    // The engine takes http:// too, for a local test server; Settings asks for https://, so no one
    // on the way sees what is fetched (signatures already catch a changed file).
    void Settings::openRepository(const juce::String& address)
    {
        if(address.startsWith("https://") || (!address.contains("://") && juce::File::isAbsolutePath(address))) { listThenAdd(address.toStdString(), false); return; }
        juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "Not an https address",
            "A repository on the web is added by its https:// address, such as https://packages.example.org; a folder by its full path, or with Choose folder.",
            {}, nullptr, juce::ModalCallbackFunction::create([](int) {})); // a callback keeps it asynchronous where modal loops are permitted (the tests)
    }

    // Read first, so only a repository that verifies is added. A root key not trusted yet is
    // offered by its fingerprint, and trusted with the repository added only when the user says
    // so; then read again, and taken back out, key and all, if it still does not verify.
    void Settings::listThenAdd(const std::string& address, bool trusted)
    {
        auto notRead = [](const std::string& why) {
            juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "Not read", text(why), {}, nullptr, juce::ModalCallbackFunction::create([](int) {}));
        };
        uint64_t job = 0;
        if(const auto error = engine_.packages([&](Control& c) { job = c.listRepository(address); }); !error.empty()) { notRead(error); return; }
        struct Poll
        {
            static void again(juce::Component::SafePointer<Settings> safe, uint64_t job, std::function<void(const Control::PackageJob&)> then)
            {
                juce::Timer::callAfterDelay(100, [safe, job, then]() {
                    if(!safe) return;
                    Control::PackageJob state;
                    if(const auto error = safe->engine_.packages([&](Control& c) { state = c.packageJob(job); }); !error.empty()) { state.state = "failed"; state.error = error; }
                    if(state.state == "running") { again(safe, job, then); return; }
                    then(state);
                });
            }
        };
        Poll::again(this, job, [safe = juce::Component::SafePointer<Settings>(this), address, trusted, notRead](const Control::PackageJob& listing) {
            if(!safe) return;
            if(listing.state == "done")
            {
                if(const auto error = safe->engine_.packages([&](Control& c) { c.addRepository({address, listing.name, listing.rootKey, {}}); }); !error.empty()) notRead(error);
                safe->refresh();
                return;
            }
            if(trusted) safe->engine_.packages([&](Control& c) { c.removeRepository(address); }); // it does not verify: not kept, nor its key
            if(!trusted && !listing.rootKey.empty() && listing.error.find("not a repository this device trusts") != std::string::npos)
            {
                auto options = juce::MessageBoxOptions::makeOptionsOkCancel(juce::MessageBoxIconType::QuestionIcon, "Trust this repository?",
                    "Its root key's fingerprint is " + fingerprint(signing::keyId(listing.rootKey)) + ". Trust it only if its publisher gave you that fingerprint.", "Trust", "Cancel", safe.getComponent());
                juce::AlertWindow::showAsync(options, [safe, address, key = listing.rootKey](int chosen) {
                    if(!safe || chosen != 1) return;
                    if(const auto error = safe->engine_.packages([&](Control& c) { c.addRepository({address, {}, key, {}}); }); !error.empty())
                    {
                        juce::AlertWindow::showMessageBoxAsync(juce::MessageBoxIconType::WarningIcon, "Not added", text(error), {}, nullptr, juce::ModalCallbackFunction::create([](int) {}));
                        return;
                    }
                    safe->listThenAdd(address, true);
                });
                return;
            }
            safe->refresh();
            notRead(listing.error);
        });
    }

    void Settings::setControllers(ControllerPorts ports)
    {
        controllers_ = std::move(ports);
        refresh();
    }

    void Settings::readPorts()
    {
        ports_.clear(); boxes_.clear();
        if(!controllers_.inputs) return;
        const auto inputs = controllers_.inputs();
        const auto choices = controllers_.choices ? controllers_.choices() : ProfileChoices{};
        const auto& profiles = controllers_.profiles;
        for(size_t i = 0; i < inputs.size(); ++i)
        {
            PortRow row{inputs[i], profilesFor(profiles, inputs, i), profileOf(profiles, inputs, i, choices), {}};
            if(row.profiles.empty()) continue;
            row.all = row.profiles.size() > 1 && profilesOf(profiles, inputs, i, choices).size() == row.profiles.size();
            // What the profile in use does with this port: its port whose name the input carries.
            for(const auto& p : profiles[row.chosen].midi.ports)
                if(juce::String(inputs[i]).containsIgnoreCase(juce::String(p.name)))
                    row.note = p.ignore ? "Ignored: a duplicate of another port" : p.protocol == "mackie" ? "Mackie Control" : "Its controls join";
            auto box = std::make_unique<juce::ComboBox>("Profile for " + text(inputs[i]));
            for(size_t k = 0; k < row.profiles.size(); ++k) box->addItem(text(profiles[row.profiles[k]].name), int(k) + 1);
            // Identical controllers on one port name, each set to its own channels, a profile each.
            if(row.profiles.size() > 1) box->addItem("Each of these, by MIDI channel", int(row.profiles.size()) + 1);
            box->setSelectedId(row.all ? int(row.profiles.size()) + 1 : int(std::find(row.profiles.begin(), row.profiles.end(), row.chosen) - row.profiles.begin()) + 1, juce::dontSendNotification);
            box->setEnabled(row.profiles.size() > 1);
            box->setTooltip(row.profiles.size() > 1 ? "Several profiles fit this port: choose the one its firmware matches, or each for identical controllers on their own channels" : "The only profile that fits this port");
            box->onChange = [this, port = inputs[i], candidates = row.profiles, raw = box.get()] {
                const auto k = size_t(raw->getSelectedId() - 1);
                if(k > candidates.size() || !controllers_.choose) return;
                std::vector<std::string> ids;
                for(size_t c = 0; c < candidates.size(); ++c) if(c == k || k == candidates.size()) ids.push_back(controllers_.profiles[candidates[c]].id);
                controllers_.choose(port, ids);
                juce::MessageManager::callAsync([safe = juce::Component::SafePointer<Settings>(this)] { if(safe) safe->refresh(); }); // not while its box calls
            };
            addAndMakeVisible(*box);
            ports_.push_back(std::move(row)); boxes_.push_back(std::move(box));
        }
    }

    void Settings::refresh()
    {
        keys_ = engine_.signingKeys();
        readPorts();
        readRepositories();
        resized();
        repaint();
    }

    void Settings::resized()
    {
        const bool wide = getWidth() >= 720; // the repositories in the right-hand column (an 800-wide screen has the room); else they come last
        const int m = mode(), row = m == 2 ? 48 : 36, control = m == 2 ? 44 : 30, headed = int(textSize(TextRole::Title, m)) + 8 + (wide ? 2 : 1) * (int(textSize(TextRole::Small, m)) + 6) + 14;
        auto area = getLocalBounds().reduced(24);
        auto right = wide ? area.removeFromRight((area.getWidth() - 24) / 2) : juce::Rectangle<int>();
        if(wide) area.removeFromRight(24);
        keysHeading_ = area.removeFromTop(headed); // heading and its line
        keysArea_ = area.removeFromTop(row * std::max<int>(1, int(keys_.size())));
        area.removeFromTop(12);
        makeKey_.setBounds(area.removeFromTop(control).removeFromLeft(160));
        // The controllers: a heading and its line, then a row per port.
        area.removeFromTop(28);
        portsHeading_ = area.removeFromTop(int(textSize(TextRole::Title, m)) + 8 + int(textSize(TextRole::Small, m)) + 4 + 12);
        portsArea_ = area.removeFromTop(row * std::max<int>(1, int(ports_.size())));
        for(size_t i = 0; i < boxes_.size(); ++i)
        {
            auto r = portsArea_.withHeight(row).translated(0, int(i) * row).reduced(16, 0);
            r.removeFromLeft(r.getWidth() * 2 / 5);
            const int width = std::min(300, r.getWidth() * 3 / 5);
            boxes_[i]->setBounds(r.removeFromLeft(width).withSizeKeepingCentre(width, control));
        }
        // The repositories: a heading and its line, a row each (its name, then its address and
        // fingerprint), its switch and, for the user's, Remove; then Add.
        auto& column = wide ? right : area;
        if(!wide) column.removeFromTop(28);
        reposHeading_ = column.removeFromTop(headed);
        const int tall = m == 2 ? 64 : 50;
        reposArea_ = column.removeFromTop(tall * std::max<int>(1, int(repositories_.size())));
        for(size_t i = 0; i < switches_.size(); ++i)
        {
            auto r = reposArea_.withHeight(tall).translated(0, int(i) * tall).reduced(16, 0);
            switches_[i]->setBounds(r.removeFromRight(m == 2 ? 80 : 60).withSizeKeepingCentre(m == 2 ? 80 : 60, control));
            r.removeFromRight(8);
            if(removes_[i]) removes_[i]->setBounds(r.removeFromRight(m == 2 ? 110 : 90).withSizeKeepingCentre(m == 2 ? 110 : 90, control));
        }
        column.removeFromTop(12);
        addRepository_.setBounds(column.removeFromTop(control).removeFromLeft(m == 2 ? 220 : 180));
    }

    void Settings::paint(juce::Graphics& g)
    {
        g.fillAll(ink(Ink::Background));
        const int m = mode(), row = m == 2 ? 48 : 36;
        const juce::Font title(juce::FontOptions(textSize(TextRole::Title, m), juce::Font::bold)), body(juce::FontOptions(textSize(TextRole::Body, m), juce::Font::bold)),
                         small(juce::FontOptions(textSize(TextRole::Small, m)));
        // A heading's line: two lines where the columns are narrow.
        auto explain = [&](const juce::String& line, juce::Rectangle<int> r) { g.setFont(small); g.drawMultiLineText(line, r.getX(), r.getY() + int(small.getAscent()), r.getWidth()); }; // wrapped, never shrunk
        auto area = keysHeading_;
        g.setColour(ink(Ink::Text)); drawInked(g, "Signing keys", area.removeFromTop(int(title.getHeight()) + 8), title);
        g.setColour(ink(Ink::Muted)); explain("Exports signed with one of these show others they came from you. Each is shown by its fingerprint.", area);
        drawSection(g, keysArea_, ink(Ink::Surface));
        if(keys_.empty()) { g.setColour(ink(Ink::Muted)); drawInked(g, "No keys yet", keysArea_.reduced(16, 0), small); }
        for(size_t i = 0; i < keys_.size(); ++i)
        {
            auto r = keysArea_.withHeight(row).translated(0, int(i) * row);
            if(i) { g.setColour(ink(Ink::Border).withAlpha(.35f)); g.fillRect(r.removeFromTop(1).reduced(12, 0)); }
            r = r.reduced(16, 0);
            g.setColour(ink(Ink::Text)); drawInked(g, text(keys_[i].name), r.removeFromLeft(r.getWidth() / 2), body);
            g.setColour(ink(Ink::Muted)); drawInked(g, fingerprint(keys_[i].id), r, small, juce::Justification::right);
        }
        auto heading = portsHeading_;
        g.setColour(ink(Ink::Text)); drawInked(g, "MIDI controllers", heading.removeFromTop(int(title.getHeight()) + 8), title);
        g.setColour(ink(Ink::Muted)); explain("Each port a controller's profile fits. Where several fit, choose the one its firmware matches; it is kept for the port.", heading);
        drawSection(g, portsArea_, ink(Ink::Surface));
        if(ports_.empty()) { g.setColour(ink(Ink::Muted)); drawInked(g, "No controller with a profile is connected", portsArea_.reduced(16, 0), small); }
        for(size_t i = 0; i < ports_.size(); ++i)
        {
            auto r = portsArea_.withHeight(row).translated(0, int(i) * row);
            if(i) { g.setColour(ink(Ink::Border).withAlpha(.35f)); g.fillRect(r.removeFromTop(1).reduced(12, 0)); }
            r = r.reduced(16, 0);
            g.setColour(ink(Ink::Text)); drawInked(g, text(ports_[i].port), r.removeFromLeft(r.getWidth() * 2 / 5).withTrimmedRight(12), body);
            r.removeFromLeft(std::min(300, r.getWidth() * 3 / 5) + 12); // its profile's box
            g.setColour(ports_[i].note.startsWith("Ignored") ? ink(Ink::Danger) : ink(Ink::Muted)); drawInked(g, ports_[i].note, r, small, juce::Justification::right);
        }
        heading = reposHeading_;
        g.setColour(ink(Ink::Text)); drawInked(g, "Package repositories", heading.removeFromTop(int(title.getHeight()) + 8), title);
        g.setColour(ink(Ink::Muted)); explain("Where the Catalog finds packages, each shown by its root key's fingerprint. Turned off, it is not read.", heading);
        drawSection(g, reposArea_, ink(Ink::Surface));
        const int tall = m == 2 ? 64 : 50;
        for(size_t i = 0; i < repositories_.size(); ++i)
        {
            const auto& repo = repositories_[i];
            auto r = reposArea_.withHeight(tall).translated(0, int(i) * tall);
            if(i) { g.setColour(ink(Ink::Border).withAlpha(.35f)); g.fillRect(r.removeFromTop(1).reduced(12, 0)); }
            r = r.reduced(16, 4);
            if(i < switches_.size()) r.setRight(switches_[i]->getX() - 8);
            if(i < removes_.size() && removes_[i]) r.setRight(removes_[i]->getX() - 8);
            const auto origin = repo.origin == "official" ? juce::String("Oscairn's own") : repo.origin == "device" ? juce::String("This device's") : juce::String();
            auto top = r.removeFromTop(r.getHeight() / 2);
            g.setColour(repo.enabled ? ink(Ink::Text) : ink(Ink::Muted));
            drawInked(g, text(repo.name) + (origin.isEmpty() ? juce::String() : juce::String::fromUTF8("  ·  ") + origin), top, body, juce::Justification::bottomLeft);
            g.setColour(ink(Ink::Muted));
            drawInked(g, text(repo.address) + juce::String::fromUTF8("  ·  ") + fingerprint(signing::keyId(repo.key)), r, small, juce::Justification::topLeft);
        }
    }
}
