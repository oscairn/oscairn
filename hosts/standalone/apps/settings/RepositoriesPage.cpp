// SPDX-License-Identifier: AGPL-3.0-or-later
#include "RepositoriesPage.h"
#include "../../SigningKeys.h"
#include "oscairn/signing.h"
#include <chrono>
#include <thread>

namespace oscairn::standalone::settings
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        const juce::String separator = juce::String::fromUTF8("  ·  ");
        // The row's line: its address and root key fingerprint, and what it is or why it is off.
        juce::String under(const Control::Repository& r)
        {
            auto line = text(r.address) + separator + fingerprint(signing::keyId(r.key));
            if(r.origin == "official") line += separator + "Oscairn's own: it can be turned off, never removed";
            else if(r.origin == "device") line += separator + "This device's: it can be turned off, never removed";
            else if(!r.enabled) line += separator + "turned off: the Catalog reads nothing from it";
            return line;
        }
    }

    RepositoriesPage::RowControls::RowControls()
    {
        for(auto* c : std::initializer_list<juce::Component*>{&on, &remove}) addAndMakeVisible(c);
    }

    void RepositoriesPage::RowControls::resized()
    {
        auto area = getLocalBounds();
        if(remove.isVisible()) { remove.setBounds(area.removeFromRight(std::min(110, area.getWidth() / 3))); area.removeFromRight(8); }
        on.setBounds(area);
    }

    RepositoriesPage::AskControls::AskControls()
    {
        address.setJustification(juce::Justification::centredLeft); address.setIndents(10, 0);
        address.setTextToShowWhenEmpty("https://packages.example.org", ink(Ink::Muted));
        address.setTitle("The repository's https:// address");
        for(auto* c : std::initializer_list<juce::Component*>{&address, &add, &cancel}) addAndMakeVisible(c);
    }

    void RepositoriesPage::AskControls::resized()
    {
        const int control = std::max(1, desktopMode(*this)) == 2 ? 44 : 30;
        auto area = getLocalBounds().withHeight(control);
        cancel.setBounds(area.removeFromRight(control == 44 ? 110 : 90)); area.removeFromRight(8);
        add.setBounds(area.removeFromRight(control == 44 ? 110 : 90)); area.removeFromRight(8);
        address.setBounds(area);
    }

    RepositoriesPage::RepositoriesPage(Engine& engine) : engine_(engine)
    {
        addRepository.setTooltip("A repository by its https:// address; its root key's fingerprint is shown before anything is trusted");
        addRepository.onClick = [this] { asking_ = true; show(); ask.address.grabKeyboardFocus(); };
        ask.address.onReturnKey = [this] { openRepository(ask.address.getText().trim()); };
        ask.add.onClick = [this] { openRepository(ask.address.getText().trim()); };
        ask.cancel.onClick = [this] { asking_ = false; refusal_.clear(); show(); };
        list();
    }

    void RepositoriesPage::list()
    {
        busy_ = true; show();
        auto listed = std::make_shared<std::vector<Control::Repository>>();
        engine_.job([listed](Control& c) { *listed = c.repositories(); },
            [safe = juce::Component::SafePointer<RepositoriesPage>(this), listed](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false; ++safe->listings_;
                safe->listError_ = text(error);
                if(error.empty()) safe->repositories_ = *listed;
                safe->show();
            });
    }

    // Read first, so only a repository that verifies is added (RFC 0031). The listing is a package
    // job: started with packages.list and asked about with packages.job until it ends, all of it
    // off the message thread in one Engine::job, not the old dialog's poll on the message thread.
    // A root key not trusted yet is offered by its fingerprint, and trusted with the repository
    // added only when the user says so; then read again, and taken back out, key and all, if it
    // still does not verify.
    void RepositoriesPage::listThenAdd(const std::string& address, bool trusted)
    {
        busy_ = true; asking_ = true; refused_.clear(); refusal_.clear(); progress_ = "Reading " + text(address) + "..."; show();
        const auto listed = std::make_shared<Control::PackageJob>();
        engine_.job([listed, address](Control& c) {
            const auto job = c.listRepository(address);
            auto state = c.packageJob(job);
            while(state.state == "running")
            {
                std::this_thread::sleep_for(std::chrono::milliseconds(100)); // the engine's job runs elsewhere; this waits for it
                state = c.packageJob(job);
            }
            *listed = state;
        }, [safe = juce::Component::SafePointer<RepositoriesPage>(this), address, trusted, listed](const std::string& error) {
            if(!safe) return;
            auto notRead = [safe](const std::string& why) { safe->busy_ = false; safe->progress_.clear(); safe->refusal_ = "Not read: " + text(why); safe->asking_ = false; safe->list(); };
            if(!error.empty()) { notRead(error); return; }
            if(listed->state == "done")
            {
                safe->progress_ = "Adding " + text(listed->name) + "..."; safe->show();
                safe->engine_.job([address, listed](Control& control) { control.addRepository({address, listed->name, listed->rootKey, {}}); },
                    [safe](const std::string& why) {
                        if(!safe) return;
                        safe->busy_ = false; safe->progress_.clear();
                        if(why.empty()) { safe->asking_ = false; safe->refusal_.clear(); }
                        else safe->refusal_ = "Not added: " + text(why);
                        safe->list();
                    });
                return;
            }
            if(trusted) // it does not verify even with its key just trusted: not kept, nor that key
            {
                safe->engine_.job([address](Control& control) { control.removeRepository(address); }, [safe, notRead, listed](const std::string&) { if(safe) notRead(listed->error); });
                return;
            }
            if(!listed->rootKey.empty() && listed->error.find("not a repository this device trusts") != std::string::npos)
            {
                safe->progress_ = "Asking whether to trust its root key..."; safe->show();
                // Its name as its root key signed it (the engine checks the signatures before trust).
                auto options = juce::MessageBoxOptions::makeOptionsOkCancel(juce::MessageBoxIconType::QuestionIcon, "Trust this repository?",
                    "It calls itself \"" + text(listed->name) + "\", signed by the root key whose fingerprint is " + fingerprint(signing::keyId(listed->rootKey)) +
                    ". Trust it only if its publisher gave you that fingerprint.", "Trust", "Cancel", safe.getComponent());
                juce::AlertWindow::showAsync(options, [safe, address, name = listed->name, key = listed->rootKey](int chosen) {
                    if(!safe) return;
                    if(chosen != 1) { safe->busy_ = false; safe->progress_.clear(); safe->refusal_ = "Not added: its root key was not trusted."; safe->asking_ = false; safe->list(); return; }
                    safe->progress_ = "Trusting its root key..."; safe->show();
                    safe->engine_.job([address, name, key](Control& control) { control.addRepository({address, name, key, {}}); }, // trusts the key for repositories, and lists it
                        [safe, address](const std::string& why) {
                            if(!safe) return;
                            if(!why.empty()) { safe->busy_ = false; safe->progress_.clear(); safe->refusal_ = "Not added: " + text(why); safe->list(); return; }
                            safe->listThenAdd(address, true); // the key is trusted now: read it for its name, then keep it
                        });
                });
                return;
            }
            notRead(listed->error);
        });
    }

    // The engine takes http:// too, for a local test server; the app asks for https:// alone, so
    // no one on the way sees what is fetched (signatures already catch a changed file), and an
    // app may name no folder on the engine's filesystem (RFC 0022): a file goes through the
    // Library's Install... > File, which the shell runs.
    void RepositoriesPage::openRepository(const juce::String& address)
    {
        if(address.startsWith("https://")) { listThenAdd(address.toStdString(), false); return; }
        refused_.clear();
        refusal_ = "Not an https address: a repository is added by its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.";
        show();
    }

    void RepositoriesPage::show()
    {
        clearRows(); rows_.clear();
        for(size_t i = 0; i < repositories_.size(); ++i)
        {
            const auto& repo = repositories_[i];
            if(controls_.size() <= i) controls_.push_back(std::make_unique<RowControls>());
            auto& c = *controls_[i];
            const auto address = repo.address;
            c.on.setToggleState(repo.enabled, juce::dontSendNotification);
            c.on.setTitle("On: " + text(repo.name));
            c.on.setEnabled(!busy_);
            c.on.onClick = [this, address, raw = &c.on] {
                // Copied first: show() sets this handler again, which ends these captures.
                auto* self = this; const auto where = address; const bool on = raw->getToggleState();
                self->busy_ = true; self->refused_ = where; self->refusal_.clear(); self->show();
                self->engine_.job([where, on](Control& control) { control.setRepositoryEnabled(where, on); },
                    [safe = juce::Component::SafePointer<RepositoriesPage>(self), where](const std::string& error) {
                        if(!safe) return;
                        safe->busy_ = false;
                        if(!error.empty()) { safe->refused_ = where; safe->refusal_ = "Not changed: " + text(error); }
                        safe->list(); // listed again as the engine kept it, refusal or not
                    });
            };
            c.remove.setVisible(repo.origin == "user"); // the official and the device's cannot be removed
            c.remove.setTitle("Remove " + text(repo.name));
            c.remove.setEnabled(!busy_);
            c.remove.onClick = [this, address, name = text(repo.name)] {
                auto options = juce::MessageBoxOptions::makeOptionsOkCancel(juce::MessageBoxIconType::QuestionIcon, "Remove " + name + "?",
                    "What you installed from it stays. Its root key is no longer trusted for repositories.", "Remove", "Cancel", this);
                juce::AlertWindow::showAsync(options, [safe = juce::Component::SafePointer<RepositoriesPage>(this), address](int chosen) {
                    if(!safe || chosen != 1) return;
                    safe->busy_ = true; safe->show();
                    safe->engine_.job([address](Control& control) { control.removeRepository(address); },
                        [safe, address](const std::string& error) {
                            if(!safe) return;
                            safe->busy_ = false;
                            if(!error.empty()) { safe->refused_ = address; safe->refusal_ = "Not removed: " + text(error); }
                            safe->list();
                        });
                });
            };
            auto& row = addRow(text(repo.name), c, under(repo));
            if(repo.address == refused_ && refusal_.isNotEmpty()) row.setExplanation(refusal_, true);
            rows_.push_back(&row);
        }
        controls_.resize(repositories_.size());
        buttonRow_ = &addRow("Add a repository", addRepository, "A new repository is added by its https:// address alone; its root key is trusted only once you have checked its fingerprint.");
        addRepository.setEnabled(!busy_ && !asking_);
        ask.setEnabled(!busy_);
        askRow_ = &addRow("Address", ask, "Its https:// address, such as https://packages.example.org. To install from a file, use the Library's Install... > File.");
        buttonRow_->setVisible(!asking_);
        askRow_->setVisible(asking_);
        if(asking_ && (progress_.isNotEmpty() || refusal_.isNotEmpty() || listError_.isNotEmpty()))
            askRow_->setExplanation(refusal_.isNotEmpty() ? refusal_ : progress_.isNotEmpty() ? progress_ : "The repositories could not be read: " + listError_, refusal_.isNotEmpty());
        if(!asking_ && refused_.empty() && refusal_.isNotEmpty()) buttonRow_->setExplanation(refusal_, true);
        else if(!asking_ && refused_.empty() && listError_.isNotEmpty()) buttonRow_->setExplanation("The repositories could not be read: " + listError_, true);
        heightChanged();
    }
}
