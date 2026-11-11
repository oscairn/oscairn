// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Settings.h"
#include "oscairn/signing.h"

namespace oscairn::standalone
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
    }

    Settings::Settings(Engine& engine) : engine_(engine)
    {
        setTitle("Settings");
        addRepository_.setTooltip("A repository by its address; its root key is trusted only once you have checked its fingerprint");
        addRepository_.onClick = [this] { addRepository(); };
        addAndMakeVisible(addRepository_);
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

    void Settings::refresh()
    {
        readRepositories();
        resized();
        repaint();
    }

    void Settings::resized()
    {
        const int m = mode(), control = m == 2 ? 44 : 30, headed = int(textSize(TextRole::Title, m)) + 8 + int(textSize(TextRole::Small, m)) + 6 + 14;
        auto column = getLocalBounds().reduced(24);
        // A heading and its line, a row each (its name, then its address and fingerprint), its
        // switch and, for the user's, Remove; then Add.
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
        const int m = mode();
        const juce::Font title(juce::FontOptions(textSize(TextRole::Title, m), juce::Font::bold)), body(juce::FontOptions(textSize(TextRole::Body, m), juce::Font::bold)),
                         small(juce::FontOptions(textSize(TextRole::Small, m)));
        auto explain = [&](const juce::String& line, juce::Rectangle<int> r) { g.setFont(small); g.drawMultiLineText(line, r.getX(), r.getY() + int(small.getAscent()), r.getWidth()); }; // wrapped, never shrunk
        auto heading = reposHeading_;
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
