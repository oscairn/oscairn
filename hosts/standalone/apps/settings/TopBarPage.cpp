// SPDX-License-Identifier: AGPL-3.0-or-later
#include "TopBarPage.h"
#include "../../Theme.h"
#include <array>

namespace oscairn::standalone::settings
{
    namespace
    {
        // Each choice as top-bar.json writes it (empty: left out, as the device has it) and as the list shows it.
        constexpr std::array<const char*, 5> keys{"", "both", "bar", "percent", "none"};
        constexpr std::array<const char*, 5> names{"As the device has it", "Bar and percent", "Bar", "Percent", "Hidden"};

        juce::File themeConfiguration()
        {
            auto* theme = dynamic_cast<Theme*>(&juce::LookAndFeel::getDefaultLookAndFeel());
            return theme ? theme->configuration() : juce::File();
        }
    }

    TopBarPage::TopBarPage(Engine&, juce::File configuration) : folder_(configuration == juce::File() ? themeConfiguration() : configuration)
    {
        const auto saved = juce::JSON::parse(file().loadFileAsString());
        for(auto [box, key] : {std::pair{&cpu, "cpu"}, {&ram, "ram"}})
        {
            for(int i = 0; i < int(names.size()); ++i) box->addItem(names[size_t(i)], i + 1);
            int chosen = 1;
            for(int i = 1; i < int(keys.size()); ++i) if(saved.getProperty(key, {}).toString() == keys[size_t(i)]) chosen = i + 1;
            box->setSelectedId(chosen, juce::dontSendNotification);
            box->onChange = [this] { save(); };
        }
        addRow("CPU", cpu, "How full the CPU is: the share of each audio block's time the engine takes.");
        row_ = &addRow("RAM", ram, "How much of the device's memory is in use. The Rust UI's top bar shows these from the next time its Home opens.");
    }

    void TopBarPage::save()
    {
        auto* chosen = new juce::DynamicObject();
        for(auto [box, key] : {std::pair{&cpu, "cpu"}, {&ram, "ram"}})
            if(const auto i = size_t(box->getSelectedId() - 1); i > 0 && i < keys.size()) chosen->setProperty(key, keys[i]);
        const bool saved = folder_ != juce::File() && folder_.createDirectory().wasOk() && file().replaceWithText(juce::JSON::toString(juce::var(chosen), true) + "\n");
        if(!saved) { row_->setExplanation("Not saved: " + file().getFullPathName() + " cannot be written.", true); heightChanged(); }
    }
}
