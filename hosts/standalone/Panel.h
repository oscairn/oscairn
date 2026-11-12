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
    /// A piano keyboard that plays notes into the engine's note input, as MIDI in does (zones,
    /// splits and layers apply): a panel's keys, and the desktop's under Play. A click or touch
    /// plays a key, lower on it louder; a drag plays across the keys, several fingers at once.
    /// Drawn in the theme.
    class Keys : public juce::Component, private juce::MidiKeyboardState::Listener
    {
    public:
        Keys( Engine& engine, int lowest, int count );
        ~Keys() override;
        std::function<int()> channel;    // the MIDI channel a note goes on, 0..15; unset or -1: the selected track's
        void show( int lowest, int count ); // the keys shown, each as wide as fits
        void press( int note, bool on, float velocity = .8f ); // a key played from elsewhere (the computer's keys), shown held
        void release();                  // every note it holds, off
        int lowest() const { return keyboard_.getRangeStart(); }
        int highest() const { return keyboard_.getRangeEnd(); }
        juce::MidiKeyboardComponent& keyboard() { return keyboard_; }

        void resized() override;
        void lookAndFeelChanged() override;
        std::unique_ptr<juce::AccessibilityHandler> createAccessibilityHandler() override { return std::make_unique<juce::AccessibilityHandler>( *this, juce::AccessibilityRole::group ); }

    private:
        void handleNoteOn( juce::MidiKeyboardState*, int, int note, float velocity ) override;
        void handleNoteOff( juce::MidiKeyboardState*, int, int note, float ) override;

        Engine&                     engine_;
        juce::MidiKeyboardState     state_;
        juce::MidiKeyboardComponent keyboard_{ state_, juce::MidiKeyboardComponent::horizontalKeyboard };
    };

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

        void timerCallback() override;

        Layout                 layout_;
        int                    designWidth_ = layout_.width, designHeight_ = layout_.height;
        juce::Component*       overlay_ = nullptr;
        Engine&                engine_;
        std::unique_ptr<Board> board_;
        EngineRecovery recovery_{engine_}; // the engine's notice and Restart, when it is gone
    };
}
