// SPDX-License-Identifier: AGPL-3.0-or-later
#include "TextKeyboard.h"
#include "Profile.h"
#include "Theme.h"

#include <algorithm>
#include <memory>

namespace oscairn::standalone
{
    namespace
    {
        constexpr int touch = Profile::Screen::touchTarget; // a key is never smaller

        // What a screen reader says for a key's character.
        juce::String spoken(const juce::String& c)
        {
            static const std::pair<const char*, const char*> names[] = {
                {" ", "space"}, {".", "period"}, {",", "comma"}, {"/", "slash"}, {":", "colon"}, {";", "semicolon"}, {"-", "hyphen"}, {"_", "underscore"},
                {"@", "at sign"}, {"#", "number sign"}, {"&", "ampersand"}, {"'", "apostrophe"}, {"\"", "quotation mark"}, {"?", "question mark"},
                {"!", "exclamation mark"}, {"(", "left parenthesis"}, {")", "right parenthesis"}, {"%", "percent"}, {"+", "plus"}};
            for(const auto& [symbol, name] : names) if(c == symbol) return name;
            return c;
        }

        // The editor's text and caret, drawn: the editor itself may be under the keys.
        class Line final : public juce::Component
        {
        public:
            Line() { setInterceptsMouseClicks(false, false); }
            void show(juce::TextEditor* e) { editor_ = e; repaint(); }
            void paint(juce::Graphics& g) override
            {
                g.setColour(ink(Ink::Background)); g.fillRoundedRectangle(getLocalBounds().toFloat(), 4);
                g.setColour(ink(Ink::Border)); g.drawRoundedRectangle(getLocalBounds().toFloat().reduced(.5f), 4, 1);
                if(!editor_) return;
                auto text = editor_->getText();
                if(const auto mask = editor_->getPasswordCharacter(); mask != 0) text = juce::String::repeatedString(juce::String::charToString(mask), text.length());
                const auto font = juce::FontOptions(textSize(TextRole::Body, 2));
                auto width = [&](int upTo) { return float(juce::GlyphArrangement::getStringWidthInt(juce::Font(font), text.substring(0, upTo))); };
                const int pad = 8, room = getWidth() - 2 * pad;
                const float caret = width(editor_->getCaretPosition());
                const float shift = std::max(0.f, caret - float(room) + 2); // the caret stays in view
                juce::Graphics::ScopedSaveState keep(g);
                g.reduceClipRegion(getLocalBounds().reduced(pad, 0));
                g.setFont(juce::Font(font));
                if(text.isEmpty()) { g.setColour(ink(Ink::Muted)); g.drawText(editor_->getTextToShowWhenEmpty(), pad, 0, room, getHeight(), juce::Justification::centredLeft); }
                const auto range = editor_->getHighlightedRegion();
                if(!range.isEmpty()) { g.setColour(ink(Ink::Accent).withAlpha(.35f)); g.fillRect(juce::Rectangle<float>(pad + width(range.getStart()) - shift, 4.f, width(range.getEnd()) - width(range.getStart()), float(getHeight() - 8))); }
                g.setColour(ink(Ink::Text));
                g.drawText(text, juce::Rectangle<float>(float(pad) - shift, 0, width(text.length()) + 4, float(getHeight())), juce::Justification::centredLeft, false);
                g.setColour(ink(Ink::Accent)); g.fillRect(juce::Rectangle<float>(pad + caret - shift, 6.f, 2.f, float(getHeight() - 12)));
            }

        private:
            juce::Component::SafePointer<juce::TextEditor> editor_;
        };

        enum class Kind { Character, Shift, Caps, Layer, Space, Backspace, Left, Right, Clear, Cancel, Done };

        class Key final : public juce::TextButton
        {
        public:
            Key(Kind k, int r, float w, juce::String l = {}, juce::String s = {}) : kind(k), row(r), weight(w), letter(std::move(l)), symbol(std::move(s))
            {
                setWantsKeyboardFocus(false); setMouseClickGrabsKeyboardFocus(false); // the editor keeps the focus
                setTriggeredOnMouseDown(true);                                         // a tap types at once
                if(k == Kind::Backspace || k == Kind::Left || k == Kind::Right) setRepeatSpeed(450, 70); // held, it repeats
            }
            const Kind kind; const int row; const float weight; const juce::String letter, symbol;
        };

        class Board final : public juce::Component
        {
        public:
            std::function<void()> dismiss; // Done and Cancel: hides, and the editor lets go of the focus

            Board()
            {
                setTitle("On-screen keyboard"); setWantsKeyboardFocus(false); setMouseClickGrabsKeyboardFocus(false);
                addAndMakeVisible(line_);
                const char* const top[] = {"q", "w", "e", "r", "t", "y", "u", "i", "o", "p"}, *const topSymbols[] = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "0"};
                const char* const middle[] = {"a", "s", "d", "f", "g", "h", "j", "k", "l"}, *const middleSymbols[] = {"@", "#", "&", "-", "_", "/", ":", "'", "\""};
                const char* const bottom[] = {"z", "x", "c", "v", "b", "n", "m", "."}, *const bottomSymbols[] = {"(", ")", ",", "?", "!", "%", "+", "."};
                add(Kind::Left, 0, 1.2f); add(Kind::Right, 0, 1.2f); add(Kind::Clear, 0, 1.6f); add(Kind::Cancel, 0, 2.f); add(Kind::Done, 0, 2.5f);
                for(int i = 0; i < 10; ++i) add(Kind::Character, 1, 1, top[i], topSymbols[i]);
                add(Kind::Backspace, 1, 1.5f);
                for(int i = 0; i < 9; ++i) add(Kind::Character, 2, 1, middle[i], middleSymbols[i]);
                add(Kind::Shift, 3, 1.5f);
                for(int i = 0; i < 8; ++i) add(Kind::Character, 3, 1, bottom[i], bottomSymbols[i]);
                add(Kind::Layer, 4, 1.5f); add(Kind::Caps, 4, 1.5f); add(Kind::Space, 4, 4.f);
                for(const char* c : {",", "-", "/", "@"}) add(Kind::Character, 4, 1, c, c);
                relabel();
            }
            // The editor typed into, and the text it has now, for Cancel.
            void attach(juce::TextEditor& e)
            {
                editor_ = &e; original_ = e.getText(); shift_ = caps_ = symbols_ = false; line_.show(&e); relabel();
            }
            juce::TextEditor* editor() const { return editor_.getComponent(); }
            void refresh() { line_.repaint(); }
            static int heightFor() { return 5 * touch + 4 * denseGap + 2 * denseMargin; }
            bool canModalEventBeSentToComponent(const juce::Component*) override { return true; } // over a modal dialog, it and the dialog both work

            void paint(juce::Graphics& g) override
            {
                g.fillAll(ink(Ink::Surface));
                g.setColour(ink(Ink::Border)); g.fillRect(0, 0, getWidth(), 1);
            }
            void resized() override
            {
                const auto area = getLocalBounds().reduced(denseMargin);
                for(int r = 0; r < 5; ++r)
                {
                    auto row = area.withTrimmedTop(r * (touch + denseGap)).withHeight(touch);
                    std::vector<Key*> in; float total = 0;
                    for(auto* k : keys_) if(k->row == r) { in.push_back(k); total += k->weight; }
                    if(r == 0)
                    {
                        // Cursor, Clear, Cancel and Done at the right; the text takes the rest.
                        float fixed = 0; for(auto* k : in) fixed += k->weight * touch + denseGap;
                        line_.setBounds(row.removeFromLeft(std::max(touch, row.getWidth() - int(fixed))));
                        for(auto* k : in) { row.removeFromLeft(denseGap); k->setBounds(row.removeFromLeft(int(k->weight * touch))); }
                        continue;
                    }
                    // The middle row is as wide as the top row's letters, centered, as printed.
                    const float unit = (float(row.getWidth()) - denseGap * float(in.size() - 1)) / total;
                    const float scale = r == 2 ? (float(row.getWidth()) - denseGap * 10.f) / 11.5f : unit;
                    if(r == 2) row.reduce(int((float(row.getWidth()) - (scale * 9 + denseGap * 8.f)) / 2), 0);
                    float x = float(row.getX());
                    for(auto* k : in) { const float w = k->weight * scale; k->setBounds(juce::Rectangle<float>(x, float(row.getY()), w, float(touch)).toNearestInt()); x += w + denseGap; }
                }
            }

        private:
            void add(Kind kind, int row, float weight, const juce::String& letter = {}, const juce::String& symbol = {})
            {
                auto* key = keys_.add(new Key(kind, row, weight, letter, symbol));
                key->onClick = [this, key] { press(*key); };
                addAndMakeVisible(key);
            }
            bool upper() const { return shift_ || caps_; }
            void relabel()
            {
                for(auto* k : keys_)
                {
                    juce::String text, name;
                    switch(k->kind)
                    {
                        case Kind::Character: text = symbols_ ? k->symbol : upper() ? k->letter.toUpperCase() : k->letter; name = spoken(text); break;
                        case Kind::Shift: text = "Shift"; name = "Shift"; break;
                        case Kind::Caps: text = "Caps"; name = "Caps lock"; break;
                        case Kind::Layer: text = symbols_ ? "abc" : "123"; name = symbols_ ? "Letters" : "Numbers and symbols"; break;
                        case Kind::Space: text = "space"; name = "Space"; break;
                        case Kind::Backspace: text = "Del"; name = "Backspace"; break;
                        case Kind::Left: text = "<"; name = "Cursor left"; break;
                        case Kind::Right: text = ">"; name = "Cursor right"; break;
                        case Kind::Clear: text = "Clear"; name = "Clear"; break;
                        case Kind::Cancel: text = "Cancel"; name = "Cancel"; break;
                        case Kind::Done: text = "Done"; name = "Done"; break;
                    }
                    k->setButtonText(text); k->setName(name); k->setTitle(name);
                    k->setClickingTogglesState(false);
                    if(k->kind == Kind::Shift) k->setToggleState(shift_, juce::dontSendNotification);
                    if(k->kind == Kind::Caps) k->setToggleState(caps_, juce::dontSendNotification);
                    if(k->kind == Kind::Shift || k->kind == Kind::Caps) k->setEnabled(!symbols_);
                    k->setColour(juce::TextButton::buttonColourId, ink(k->kind == Kind::Done ? Ink::Accent : Ink::Raised));
                    if(k->kind == Kind::Done) k->setColour(juce::TextButton::textColourOffId, ink(Ink::Accent).contrasting());
                }
            }
            void type(const juce::KeyPress& key) { if(auto* e = editor()) e->keyPressed(key); refresh(); }
            void press(Key& key)
            {
                auto* e = editor();
                switch(key.kind)
                {
                    case Kind::Character:
                    {
                        const auto text = key.getButtonText();
                        type(juce::KeyPress(int(text[0]), juce::ModifierKeys(), text[0]));
                        if(shift_) { shift_ = false; relabel(); } // one-shot
                        break;
                    }
                    case Kind::Space: type(juce::KeyPress(juce::KeyPress::spaceKey, juce::ModifierKeys(), ' ')); if(shift_) { shift_ = false; relabel(); } break;
                    case Kind::Shift: shift_ = !shift_; relabel(); break;
                    case Kind::Caps: caps_ = !caps_; relabel(); break;
                    case Kind::Layer: symbols_ = !symbols_; shift_ = false; relabel(); break;
                    case Kind::Backspace: type(juce::KeyPress(juce::KeyPress::backspaceKey)); break;
                    case Kind::Left: type(juce::KeyPress(juce::KeyPress::leftKey)); break;
                    case Kind::Right: type(juce::KeyPress(juce::KeyPress::rightKey)); break;
                    case Kind::Clear: if(e) e->setText({}, true); refresh(); break;
                    case Kind::Cancel: if(e) e->setText(original_, true); if(dismiss) dismiss(); break;
                    case Kind::Done:
                    {
                        // As Return does: the editor, then what it is in (an alert's default button).
                        const juce::KeyPress ret(juce::KeyPress::returnKey);
                        bool used = e && !e->isMultiLine() && e->keyPressed(ret);
                        if(e && !e->isMultiLine())
                            for(auto* c = e->getParentComponent(); c && !used;)
                            {
                                auto* parent = c->getParentComponent();
                                if((used = c->keyPressed(ret))) break; // an alert may be gone soon
                                c = parent;
                            }
                        if(dismiss) dismiss();
                        break;
                    }
                }
            }

            Line line_;
            juce::OwnedArray<Key> keys_;
            juce::Component::SafePointer<juce::TextEditor> editor_;
            juce::String original_;
            bool shift_ = false, caps_ = false, symbols_ = false;
        };

        struct Controller final : juce::DeletedAtShutdown, private juce::FocusChangeListener, private juce::MouseListener, private juce::Timer
        {
            static inline Controller* instance = nullptr;

            Controller()
            {
                instance = this;
                juce::Desktop::getInstance().addFocusChangeListener(this);
                juce::Desktop::getInstance().addGlobalMouseListener(this);
                startTimerHz(10);
            }
            ~Controller() override
            {
                juce::Desktop::getInstance().removeFocusChangeListener(this);
                juce::Desktop::getInstance().removeGlobalMouseListener(this);
                hide(); instance = nullptr;
            }
            Board* shown() const { return board_ && board_->getParentComponent() ? board_.get() : nullptr; }

            void globalFocusChanged(juce::Component* focused) override
            {
                auto* e = dynamic_cast<juce::TextEditor*>(focused);
                if(e && !e->isReadOnly() && e->isEnabled()) show(*e);
                else if(shown() && !(focused && shown()->isParentOf(focused))) hide();
            }
            // A tap on anything but a text field or the keyboard puts the keyboard away: the editor keeps the focus otherwise.
            void mouseDown(const juce::MouseEvent& event) override
            {
                auto* in = event.eventComponent;
                if(!shown() || !in || shown()->isParentOf(in) || in == shown() || in->findParentComponentOfClass<juce::TextEditor>() || dynamic_cast<juce::TextEditor*>(in)) return;
                dismiss();
            }
            void timerCallback() override
            {
                if(!shown()) return;
                auto* e = shown()->editor();
                if(!e || !showing(*e)) { hide(); return; }
                place(*shown()->getParentComponent());
                shown()->refresh();
            }

            void dismiss()
            {
                auto* e = shown() ? shown()->editor() : nullptr;
                juce::Component::SafePointer<juce::TextEditor> editor(e);
                hide();
                if(editor && editor->hasKeyboardFocus(false)) editor->giveAwayKeyboardFocus(); // so tapping it again asks for the keyboard again
            }
            void show(juce::TextEditor& e)
            {
                auto* host = e.getTopLevelComponent();
                if(!host) return;
                if(!shown() || shown()->getParentComponent() != host)
                {
                    hide();
                    board_ = std::make_unique<Board>();
                    board_->dismiss = [this] { dismiss(); };
                    host->addChildComponent(*board_);
                }
                shown()->attach(e);
                place(*host);
                // A dialog under the keys moves up, to where it is above them.
                if(!moved_)
                    for(juce::Component* at = &e; at; at = at->getParentComponent())
                        if(at->getParentComponent() == host)
                        {
                            if(dynamic_cast<juce::TopLevelWindow*>(at) && at->getBottom() > shown()->getY())
                            {
                                moved_ = at; movedFrom_ = at->getBounds();
                                at->setTopLeftPosition(at->getX(), std::max(0, shown()->getY() - at->getHeight()));
                            }
                            break;
                        }
            }
            void place(juce::Component& host)
            {
                auto* b = shown(); if(!b) return;
                b->setBounds(host.getLocalBounds().removeFromBottom(std::min(Board::heightFor(), host.getHeight())));
                if(!b->isVisible()) b->setVisible(true);
                b->toFront(false);
                // Over a modal dialog, which blocks what is not in it, the keyboard is modal too and lets everything through.
                if(juce::Component::getCurrentlyModalComponent() != nullptr && !b->isCurrentlyModal()) b->enterModalState(false);
            }
            void hide()
            {
                if(moved_) moved_->setBounds(movedFrom_);
                moved_ = nullptr;
                if(!board_) return;
                if(board_->isCurrentlyModal()) board_->exitModalState(0);
                board_->setVisible(false);
                if(auto* parent = board_->getParentComponent()) parent->removeChildComponent(board_.get());
                // Not deleted here: this may run from one of its own keys.
                juce::MessageManager::callAsync([gone = std::shared_ptr<Board>(std::move(board_))] {});
            }

            std::unique_ptr<Board> board_;
            juce::Component::SafePointer<juce::Component> moved_;
            juce::Rectangle<int> movedFrom_;
        };
    }

    void setTextEntryOnScreen(bool on)
    {
        if(on && !Controller::instance) new Controller();
        else if(!on && Controller::instance) delete Controller::instance;
    }
    bool textEntryOnScreen() { return Controller::instance != nullptr; }
    juce::Component* textKeyboard() { return Controller::instance ? Controller::instance->shown() : nullptr; }
}
