// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include "Engine.h"
#include "Launcher.h" // EngineRecovery
#include "Layout.h"
#include "Screen.h"

#include <juce_audio_utils/juce_audio_utils.h>
#include <juce_gui_basics/juce_gui_basics.h>

#include <memory>
#include <vector>

namespace oscairn::standalone
{
    /// A device's front panel built from a Layout: its screen and controls, bound to the
    /// engine, drawn at the layout's design size and scaled to fit the component.
    class Panel : public juce::Component, private juce::Timer
    {
    public:
        /// Throws std::runtime_error if a control is bound to something the engine lacks.
        Panel( Engine& engine, const Layout& layout );
        ~Panel() override;

        const Layout& layout() const { return layout_; }
        /// Puts a view over the screen (the Parts views), scaled with the panel like the screen,
        /// so the panel's knobs and buttons stay in reach; null removes it.
        void overlay( juce::Component* view );
        /// A stretching layout's size in design units on its screen (RFC 0019): the screen's pixels
        /// over its profile's scale, so the panel reflows to that size and scales to fill the
        /// component, with nothing cut off and no bars. The layout's own size until it is set.
        void setDesignSize( int width, int height );

        void paint( juce::Graphics& ) override;
        void resized() override;

    private:
        class Board;
        class Knob;
        class Pads;
        class Keys;

        void timerCallback() override;

        Layout                 layout_;
        int                    designWidth_ = layout_.width, designHeight_ = layout_.height;
        juce::Component*       overlay_ = nullptr;
        Engine&                engine_;
        std::unique_ptr<Board> board_;
        EngineRecovery recovery_{engine_}; // the engine's notice and Restart, when it is gone
    };
}
