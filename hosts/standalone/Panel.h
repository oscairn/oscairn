// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include "Engine.h"
#include "Launcher.h" // EngineRecovery
#include "Layout.h"
#include "Screen.h"

#include <juce_audio_utils/juce_audio_utils.h>
#include <juce_gui_basics/juce_gui_basics.h>

#include <array>
#include <bitset>
#include <memory>
#include <vector>

namespace oscairn::standalone
{
    /// A piano keyboard that plays notes into the engine's note input, as MIDI in does (zones,
    /// splits and layers apply): a panel's keys, and an input's on-screen keyboard. A click or
    /// touch plays a key, lower on it louder; a drag plays across the keys, several fingers at
    /// once. Drawn in the theme (design-language.md): a key played here filled with the accent as
    /// high as its velocity, one held by sustain tinted with it, and the input's own notes as they
    /// arrive a dot in the positive color.
    class Keys : public juce::Component, private juce::MidiKeyboardState::Listener
    {
    public:
        Keys( Engine& engine, int lowest, int count );
        ~Keys() override;
        std::function<int()> channel;    // the MIDI channel a note goes on, 0..15; unset or -1: the selected track's
        std::string input;               // the MIDI input it plays as (session status); empty: the built-in source
        void show( int lowest, int count ); // the keys shown, each as wide as fits
        void press( int note, bool on, float velocity = .8f ); // a key played from elsewhere (the computer's keys), shown held
        void letGo();                    // every key held down (mouse, fingers, the computer's keys) up; sustain keeps them sounding
        void release();                  // every note it holds or sustains, off, and sustain off
        void setSustain( bool on );      // keys let go keep sounding until it is off
        bool sustain() const { return sustain_; }
        void setIncoming( const std::vector<uint8_t>& keys ); // the input's own notes arriving now, lit
        bool incoming( int note ) const { return note >= 0 && note < 128 && incoming_[size_t( note )]; }
        bool sustained( int note ) const { return note >= 0 && note < 128 && sustained_[size_t( note )]; }
        float velocity( int note ) const { return note >= 0 && note < 128 ? velocity_[size_t( note )] : 0.f; }
        bool down( int note ) const { return state_.isNoteOnForChannels( 0xffff, note ); }
        int lowest() const;
        int highest() const;
        juce::MidiKeyboardComponent& keyboard();

        void resized() override;
        std::unique_ptr<juce::AccessibilityHandler> createAccessibilityHandler() override { return std::make_unique<juce::AccessibilityHandler>( *this, juce::AccessibilityRole::group ); }

    private:
        class Board;
        void handleNoteOn( juce::MidiKeyboardState*, int, int note, float velocity ) override;
        void handleNoteOff( juce::MidiKeyboardState*, int, int note, float ) override;
        void send( bool on, int note, float velocity );

        Engine&                 engine_;
        juce::MidiKeyboardState state_;
        std::unique_ptr<Board>  keyboard_;
        std::bitset<128>        incoming_, sustained_;
        std::array<float, 128>  velocity_{};
        bool                    sustain_ = false;
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
