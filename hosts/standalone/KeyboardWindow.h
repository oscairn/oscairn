// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Engine.h"
#include "Panel.h" // Keys

#include <juce_gui_basics/juce_gui_basics.h>
#include <map>
#include <memory>
#include <string>
#include <vector>

namespace oscairn::standalone
{
    class PlayView;

    /// A MIDI input's on-screen keyboard (design-language.md): its keys play as that input, so
    /// its parts' device selectors, channels and zones apply exactly as to its own notes (the
    /// built-in input, empty, is the on-screen keyboard proper). Its keys light as the input's own
    /// notes arrive. Over the keys, a thin band per part the input plays, over that part's keys.
    /// The header holds the octave, the velocity (where on the key, or fixed) and sustain. The
    /// computer's keys play it while it has focus, and no modifier is held: A W S E D F T G Y H U J
    /// K O L P ; from the octave's C, Z and X an octave down and up. Esc closes it.
    class KeyboardPanel : public juce::Component, private juce::Timer
    {
    public:
        KeyboardPanel(Engine& engine, std::string input);
        ~KeyboardPanel() override;
        const std::string& input() const { return input_; }
        juce::String title() const; // the input's name; "On-screen keyboard" for the built-in one
        Keys& keys() { return keys_; }
        // A part this input plays, by name, over its keys: drawn as a band.
        struct Zone { std::string part; int low = 0, high = 127; bool operator==(const Zone&) const = default; };
        const std::vector<Zone>& zones() const { return zones_; }
        void refresh();   // the zones, the input's own notes, the input still connected (the timer's)
        void letGo();     // every key held down up (the window went behind, focus left); sustain keeps them
        void release();   // every note it holds or sustains, off
        void setSheet(bool sheet); // a Close button in the header, for a sheet over a device's screen
        std::function<void()> onClose; // Esc, Close, or its input disconnected (after release)
        int octave() const { return octave_; } // from the middle C octave
        juce::Button& octaveDown() { return down_; }
        juce::Button& octaveUp() { return up_; }
        juce::Button& fixedVelocity() { return fixed_; }
        juce::Button& sustainButton() { return sustain_; }

        bool keyPressed(const juce::KeyPress&) override;
        bool keyStateChanged(bool down) override;
        void focusLost(FocusChangeType) override;
        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        void timerCallback() override;
        void shift(int by); // octaves
        Engine& engine_;
        std::string input_;
        Keys keys_;
        std::vector<Zone> zones_;
        juce::Rectangle<int> header_, bands_;
        juce::TextButton down_{"-"}, up_{"+"}, fixed_{"Fixed velocity"}, sustain_{"Sustain"}, close_{"Close"};
        int octave_ = 0, ticks_ = 0;
        std::map<int, int> typed_; // the computer's keys held, by key code, and their notes
    };

    /// The on-screen keyboards open, one per MIDI input, each independent: on the desktop a
    /// floating window, over the app's windows while it is in front but not over other apps', its
    /// size and place kept per input; on a device's screen (no window manager) a sheet over the
    /// app, one at a time, closed with Close or Esc. Closing one, its input going and Panic let go
    /// of every note it holds.
    class Keyboards : private juce::ComponentListener
    {
    public:
        // `configuration`: the folder keyboard.txt keeps the places and the hint's dismissal in.
        // `sheetHost`: a device's screen, which sheets cover; none on the desktop.
        Keyboards(Engine& engine, juce::File configuration, juce::Component* sheetHost = nullptr);
        ~Keyboards() override;
        void open(const std::string& input); // or brought to the front
        void close(const std::string& input);
        void releaseAll();
        KeyboardPanel* find(const std::string& input) const;
        juce::Component* window(const std::string& input) const; // its window, or its sheet
        size_t size() const { return open_.size(); }
        std::function<int()> channel; // the channel the keys play on (Keys::channel)
        std::function<int()> mode;    // the desktop mode a new one takes (1, or 2 for touch)
        // Play's line for a musician with no MIDI device opens the built-in one, until dismissed.
        void offerHint(PlayView&);

    private:
        void componentMovedOrResized(juce::Component&, bool, bool) override; // the sheet host
        void place(juce::Component& sheet) const;
        void save() const;
        Engine& engine_;
        juce::File file_;
        juce::Component::SafePointer<juce::Component> sheetHost_;
        std::map<std::string, std::unique_ptr<juce::Component>> open_;
        std::map<std::string, juce::Rectangle<int>> places_;
        bool hintDismissed_ = false;
        std::shared_ptr<bool> alive_ = std::make_shared<bool>(true);
    };
}
