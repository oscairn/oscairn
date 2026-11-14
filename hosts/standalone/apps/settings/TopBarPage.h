// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"

namespace oscairn::standalone::settings
{
    /// The top bar's resource section in the Rust UI (docs/rust-ui.md): CPU and RAM, each as a
    /// bar, a percent, both or neither, over what the profile's screens[].topBar says. Kept in
    /// the configuration folder's top-bar.json (`{"cpu":"bar","ram":"none"}`, a meter left out
    /// as the device has it), which the Rust UI reads when it starts. The JUCE top bar is not changed by it.
    class TopBarPage : public Page
    {
    public:
        /// `configuration`: the folder top-bar.json is in; none: the theme's (the user's configuration folder).
        explicit TopBarPage(Engine& engine, juce::File configuration = {});
        juce::ComboBox cpu, ram;
        juce::File file() const { return folder_.getChildFile("top-bar.json"); }

    private:
        juce::File folder_;
        Row* row_ = nullptr;
        void save();
    };
}
