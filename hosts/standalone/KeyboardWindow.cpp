// SPDX-License-Identifier: AGPL-3.0-or-later
#include "KeyboardWindow.h"
#include "PartsModel.h"
#include "PlayView.h"
#include "Theme.h"

#include <algorithm>

namespace oscairn::standalone
{
    namespace
    {
        const juce::String typingKeys = "awsedftgyhujkolp;"; // the computer's keys from C up, as JUCE's keyboard has them
        constexpr int bandRows = 6, bandHeight = 4, bandGap = 2; // a band per part, at most six rows

        juce::String noteName(int note) { return juce::MidiMessage::getMidiNoteName(note, true, true, 4); }

        /// A keyboard's own window: over the app's windows while the app is in front, not over
        /// other apps' (JUCE has no owned windows, so it is on top only while Oscairn is the
        /// foreground app). Going behind lets go of the keys held down.
        class KeyboardWindow final : public juce::DocumentWindow, private juce::Timer
        {
        public:
            explicit KeyboardWindow(std::unique_ptr<KeyboardPanel> panel)
                : juce::DocumentWindow("Oscairn - " + panel->title(), ink(Ink::Background), juce::DocumentWindow::closeButton | juce::DocumentWindow::minimiseButton), panel_(*panel)
            {
                setUsingNativeTitleBar(true);
                setContentOwned(panel.release(), false);
                setResizable(true, false);
                setResizeLimits(360, 150, 4000, 640);
                timerCallback();
                startTimer(250);
            }
            KeyboardPanel& panel() { return panel_; }
            std::function<void()> onClose;
            void closeButtonPressed() override { if(onClose) onClose(); }
            void activeWindowStatusChanged() override
            {
                if(isActiveWindow()) panel_.grabKeyboardFocus();
                else panel_.letGo(); // its key-ups go to the window in front now
            }

        private:
            void timerCallback() override
            {
                const bool front = juce::Process::isForegroundProcess();
                if(front != isAlwaysOnTop()) setAlwaysOnTop(front);
            }
            KeyboardPanel& panel_;
        };
    }

    KeyboardPanel::KeyboardPanel(Engine& engine, std::string input) : engine_(engine), input_(std::move(input)), keys_(engine, 48, 25)
    {
        keys_.input = input_;
        keys_.keyboard().clearKeyMappings(); // the panel plays the computer's keys (keyPressed)
        keys_.keyboard().setWantsKeyboardFocus(false);
        keys_.setDescription("Plays as " + title() + ". The computer's keys play it too while it has focus: A W S E D F T G Y H U J K O L P and semicolon from C, Z and X an octave down and up");
        addAndMakeVisible(keys_);
        down_.setTooltip("An octave down (Z)");
        up_.setTooltip("An octave up (X)");
        down_.setTitle("Octave down"); up_.setTitle("Octave up");
        down_.onClick = [this] { shift(-1); };
        up_.onClick = [this] { shift(1); };
        fixed_.setTooltip("Every note at the same velocity; off, lower on a key is louder");
        fixed_.setClickingTogglesState(true);
        fixed_.onClick = [this] { const bool fixed = fixed_.getToggleState(); keys_.keyboard().setVelocity(fixed ? 100 / 127.f : 1.f, !fixed); };
        sustain_.setTooltip("Keys let go keep sounding until Sustain is off");
        sustain_.setClickingTogglesState(true);
        sustain_.onClick = [this] { keys_.setSustain(sustain_.getToggleState()); };
        close_.setTooltip("Close the keyboard (Esc)");
        close_.onClick = [this] { if(onClose) onClose(); };
        for(auto* b : {&down_, &up_, &fixed_, &sustain_, &close_})
        {
            b->setWantsKeyboardFocus(false); // the panel keeps focus, so the computer's keys play
            addChildComponent(*b);
            b->setVisible(b != &close_);
        }
        setWantsKeyboardFocus(true);
        setTitle(title());
        refresh();
        startTimer(33); // the input's own notes, a frame or two after the engine's status says
    }

    KeyboardPanel::~KeyboardPanel() { stopTimer(); release(); }

    juce::String KeyboardPanel::title() const { return input_.empty() ? juce::String("On-screen keyboard") : juce::String::fromUTF8(input_.c_str()); }

    void KeyboardPanel::setSheet(bool sheet) { close_.setVisible(sheet); resized(); }

    void KeyboardPanel::timerCallback() { refresh(); }

    void KeyboardPanel::refresh()
    {
        if(!engine_.connected()) return;
        const auto status = engine_.sessionStatus();
        int source = input_.empty() ? int(builtInSource) : -1;
        for(const auto& e : status.midiInputs) if(e.name == input_) source = e.source;
        if(source < 0) // unplugged: its notes go, and so does its keyboard
        {
            release();
            if(onClose) onClose();
            return;
        }
        std::vector<uint8_t> lit;
        for(const auto& h : status.heldNotes) if(h.source == source) lit = h.keys;
        keys_.setIncoming(lit);
        if(ticks_++ % 15) return; // the parts' zones, twice a second
        std::vector<Zone> zones;
        try
        {
            for(const auto& p : readParts(engine_.graph()).parts)
                if(p.notes && (resolveDevices(p.devices, status.midiInputs) >> source & 1)) zones.push_back({p.name, p.lowKey, p.highKey});
        }
        catch(const std::exception&) { return; }
        if(zones == zones_) return;
        const bool rows = std::min(zones.size(), size_t(bandRows)) != std::min(zones_.size(), size_t(bandRows));
        zones_ = std::move(zones);
        juce::String described;
        for(const auto& z : zones_) described << (described.isEmpty() ? "" : "; ") << juce::String::fromUTF8(z.part.c_str()) << ": " << noteName(z.low) << " to " << noteName(z.high);
        setDescription(zones_.empty() ? juce::String("Plays no part") : "Plays " + described);
        if(rows) resized();
        repaint();
    }

    void KeyboardPanel::letGo()
    {
        typed_.clear();
        keys_.letGo();
    }

    void KeyboardPanel::release()
    {
        typed_.clear();
        keys_.release();
        sustain_.setToggleState(false, juce::dontSendNotification);
    }

    void KeyboardPanel::shift(int by)
    {
        for(const auto& [code, note] : typed_) keys_.press(note, false); // played an octave over, released in it
        typed_.clear();
        octave_ = std::clamp(octave_ + by, -4, 4);
        resized();
        repaint();
    }

    bool KeyboardPanel::keyPressed(const juce::KeyPress& key)
    {
        if(key == juce::KeyPress::escapeKey) { if(onClose) onClose(); return true; }
        if(key.getModifiers().isAnyModifierKeyDown() || dynamic_cast<juce::TextEditor*>(juce::Component::getCurrentlyFocusedComponent())) return false; // the menus' shortcuts have a modifier
        const auto code = key.getKeyCode();
        const auto c = juce::CharacterFunctions::toLowerCase(juce::juce_wchar(code));
        if(c == 'z' || c == 'x') { shift(c == 'x' ? 1 : -1); return true; }
        const int at = typingKeys.indexOfChar(c);
        if(at < 0) return false;
        if(!typed_.count(code)) // held: its repeats pass
        {
            typed_[code] = std::clamp(60 + 12 * octave_ + at, 0, 127);
            keys_.press(typed_[code], true, fixed_.getToggleState() ? 100 / 127.f : .8f);
        }
        return true;
    }

    bool KeyboardPanel::keyStateChanged(bool)
    {
        bool used = false;
        for(auto it = typed_.begin(); it != typed_.end();)
            if(juce::KeyPress::isKeyCurrentlyDown(it->first)) ++it;
            else { keys_.press(it->second, false); it = typed_.erase(it); used = true; }
        return used;
    }

    void KeyboardPanel::focusLost(FocusChangeType) { if(!hasKeyboardFocus(true)) letGo(); }

    void KeyboardPanel::resized()
    {
        const bool touch = desktopMode(*this) == 2;
        auto area = getLocalBounds().reduced(touch ? 12 : 8);
        header_ = area.removeFromTop(touch ? 44 : 30);
        auto controls = header_;
        if(close_.isVisible()) { close_.setBounds(controls.removeFromRight(touch ? 90 : 70)); controls.removeFromRight(8); }
        sustain_.setBounds(controls.removeFromRight(touch ? 100 : 80)); controls.removeFromRight(6);
        fixed_.setBounds(controls.removeFromRight(touch ? 140 : 116)); controls.removeFromRight(12);
        up_.setBounds(controls.removeFromRight(touch ? 44 : 30));
        controls.removeFromRight(touch ? 86 : 72); // the keys shown, written between
        down_.setBounds(controls.removeFromRight(touch ? 44 : 30));
        area.removeFromTop(touch ? 10 : 8);
        const int rows = int(std::min(zones_.size(), size_t(bandRows)));
        bands_ = area.removeFromTop(std::max(1, rows) * (bandHeight + bandGap));
        area.removeFromTop(2);
        // As many octaves (up to seven) as fit keys a finger or a pointer can hit, around middle C and the octave chosen.
        const int whiteKey = touch ? 36 : 22, octaves = std::clamp((area.getWidth() / whiteKey - 1) / 7, 1, 7);
        keys_.show(std::clamp(60 - 12 * (octaves / 2) + 12 * octave_, 0, 127 - 12 * octaves), 12 * octaves + 1);
        keys_.setBounds(area);
        keys_.resized();
    }

    void KeyboardPanel::paint(juce::Graphics& g)
    {
        g.fillAll(ink(Ink::Background));
        const int m = std::max(1, desktopMode(*this));
        g.setColour(ink(Ink::Text));
        drawInked(g, title(), header_.withRight(down_.getX() - 12), juce::Font(juce::FontOptions(textSize(TextRole::Body, m), juce::Font::bold)));
        g.setColour(ink(Ink::Muted));
        drawInked(g, noteName(keys_.lowest()) + juce::String::fromUTF8("–") + noteName(keys_.highest()), {down_.getRight(), header_.getY(), up_.getX() - down_.getRight(), header_.getHeight()},
                  juce::Font(juce::FontOptions(textSize(TextRole::Small, m))), juce::Justification::horizontallyCentred);
        // A band per part this input plays, over its keys, above them: splits side by side, layers stacked.
        auto& board = keys_.keyboard();
        for(size_t i = 0; i < zones_.size() && i < size_t(bandRows); ++i)
        {
            const auto& z = zones_[i];
            const int low = std::max(z.low, keys_.lowest()), high = std::min(z.high, keys_.highest());
            if(low > high) continue;
            const float x = float(keys_.getX()) + board.getRectangleForKey(low).getX(), right = float(keys_.getX()) + board.getRectangleForKey(high).getRight();
            g.setColour(ink(Ink::Accent).withAlpha(i % 2 ? .65f : 1.f));
            g.fillRoundedRectangle({x + 1, float(bands_.getY() + int(i) * (bandHeight + bandGap)), std::max(2.f, right - x - 2), float(bandHeight)}, bandHeight / 2.f);
        }
    }

    Keyboards::Keyboards(Engine& engine, juce::File configuration, juce::Component* sheetHost) : engine_(engine), file_(configuration.getChildFile("keyboard.txt")), sheetHost_(sheetHost)
    {
        // keyboard.txt: "hint dismissed", and a line per input's window, "x y width height name".
        // Lines it does not know (the old docked keyboard's "shown" or "hidden") are dropped.
        juce::StringArray lines;
        lines.addLines(file_.loadFileAsString());
        for(auto line : lines)
        {
            if(line.trim() == "hint dismissed") { hintDismissed_ = true; continue; }
            std::array<int, 4> v{};
            bool ok = true;
            for(auto& n : v)
            {
                const auto token = line.upToFirstOccurrenceOf(" ", false, false);
                ok = ok && token.isNotEmpty() && token.containsOnly("-0123456789") && line.contains(" ");
                n = token.getIntValue();
                line = line.fromFirstOccurrenceOf(" ", false, false);
            }
            if(ok && v[2] > 0 && v[3] > 0) places_[line.toStdString()] = {v[0], v[1], v[2], v[3]};
        }
        if(sheetHost) sheetHost->addComponentListener(this);
    }

    Keyboards::~Keyboards()
    {
        for(auto& [input, c] : open_)
        {
            if(auto* p = find(input)) p->release();
            if(!sheetHost_ && c->isVisible()) places_[input] = c->getBounds();
        }
        save();
        if(sheetHost_) sheetHost_->removeComponentListener(this);
        open_.clear();
    }

    KeyboardPanel* Keyboards::find(const std::string& input) const
    {
        const auto it = open_.find(input);
        if(it == open_.end()) return nullptr;
        if(auto* w = dynamic_cast<KeyboardWindow*>(it->second.get())) return &w->panel();
        return dynamic_cast<KeyboardPanel*>(it->second.get());
    }

    juce::Component* Keyboards::window(const std::string& input) const
    {
        const auto it = open_.find(input);
        return it == open_.end() ? nullptr : it->second.get();
    }

    void Keyboards::open(const std::string& input)
    {
        if(auto* shown = window(input)) // open already, or closed a moment ago and not yet gone
        {
            shown->setVisible(true); shown->toFront(true);
            if(auto* p = find(input)) p->grabKeyboardFocus();
            return;
        }
        if(sheetHost_) for(auto& [other, c] : open_) close(other); // a device's screen: one sheet at a time
        auto panel = std::make_unique<KeyboardPanel>(engine_, input);
        panel->keys().channel = [this] { return channel ? channel() : -1; };
        // Closed: its notes let go at once, and it goes after the click or key that closed it.
        panel->onClose = [this, alive = std::weak_ptr<bool>(alive_), input] {
            auto* shown = window(input);
            if(!shown || !shown->isVisible()) return;
            if(auto* p = find(input)) p->release();
            if(!sheetHost_) places_[input] = shown->getBounds();
            shown->setVisible(false);
            save();
            juce::MessageManager::callAsync([this, alive, input] {
                if(alive.expired()) return;
                if(const auto it = open_.find(input); it != open_.end() && !it->second->isVisible()) open_.erase(it);
            });
        };
        if(sheetHost_)
        {
            panel->setSheet(true);
            setDesktopMode(*panel, 2);
            sheetHost_->addAndMakeVisible(*panel);
            place(*panel);
            panel->grabKeyboardFocus();
            open_[input] = std::move(panel);
            return;
        }
        setDesktopMode(*panel, mode ? mode() : 1);
        auto* p = panel.get();
        auto w = std::make_unique<KeyboardWindow>(std::move(panel));
        w->onClose = [p] { if(p->onClose) p->onClose(); };
        // Where it was, if that is still on a display; else along the bottom of the main display, each new one a step over.
        const auto& displays = juce::Desktop::getInstance().getDisplays();
        const auto kept = places_.find(input);
        if(kept != places_.end() && displays.getDisplayForRect(kept->second) && displays.getDisplayForRect(kept->second)->userArea.intersects(kept->second)) w->setBounds(kept->second);
        else
        {
            const auto area = displays.getPrimaryDisplay() ? displays.getPrimaryDisplay()->userArea : juce::Rectangle<int>(0, 0, 1280, 800);
            const int width = std::min(900, area.getWidth() - 80), height = desktopMode(*p) == 2 ? 300 : 230, step = 30 * int(open_.size());
            w->setBounds(area.getCentreX() - width / 2 + step, area.getBottom() - height - 60 - step, width, height);
        }
        w->setVisible(true);
        w->toFront(true);
        open_[input] = std::move(w);
    }

    void Keyboards::close(const std::string& input)
    {
        if(auto* p = find(input); p && p->onClose) p->onClose();
    }

    void Keyboards::releaseAll()
    {
        for(const auto& [input, c] : open_) if(auto* p = find(input)) p->release();
    }

    void Keyboards::offerHint(PlayView& play)
    {
        if(hintDismissed_) { play.offerKeyboard({}, {}); return; }
        const std::weak_ptr<bool> alive = alive_;
        play.offerKeyboard([this, alive] { if(!alive.expired()) open({}); },
                           [this, alive] { if(!alive.expired()) { hintDismissed_ = true; save(); } });
    }

    void Keyboards::componentMovedOrResized(juce::Component&, bool, bool)
    {
        for(auto& [input, c] : open_) place(*c);
    }

    // Along the bottom of the screen, as tall as keys a finger can play, the app above it.
    void Keyboards::place(juce::Component& sheet) const
    {
        if(!sheetHost_) return;
        auto area = sheetHost_->getLocalBounds();
        sheet.setBounds(area.removeFromBottom(std::clamp(area.getHeight() * 45 / 100, std::min(area.getHeight(), 200), 340)));
    }

    void Keyboards::save() const
    {
        juce::String text;
        if(hintDismissed_) text << "hint dismissed\n";
        for(const auto& [input, r] : places_) text << r.getX() << ' ' << r.getY() << ' ' << r.getWidth() << ' ' << r.getHeight() << ' ' << juce::String::fromUTF8(input.c_str()) << '\n';
        file_.getParentDirectory().createDirectory();
        file_.replaceWithText(text);
    }
}
