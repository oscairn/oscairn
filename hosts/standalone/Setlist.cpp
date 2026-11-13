// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Setlist.h"
#include "PerformanceColors.h"
#include "oscairn/library.h"

namespace oscairn::standalone
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        constexpr int maxSlots = 1000, maxSide = 8, colorMenuBase = 100;
        const char* const modeNames[] = {"list", "scroll", "grid"};
    }

    std::vector<Control::SetlistSlot> slotsOf(const Control::Setlist& setlist)
    {
        std::vector<Control::SetlistSlot> out;
        for(const auto& page : setlist.pages) out.insert(out.end(), page.slots.begin(), page.slots.end());
        return out;
    }

    /// The grid: the page shown as columns × rows cells, a screen at a time, the screen holding
    /// the slot playing. A tap opens one; a right click gives its menu.
    class SetlistPane::Grid : public juce::Component
    {
    public:
        explicit Grid(SetlistPane& pane) : pane_(pane) {}
        int perScreen() const { const auto& s = pane_.setlist_; return int(s.columns * s.rows); }
        int screen() const { const int at = pane_.current_ - pane_.first(); return at >= 0 && at < pane_.getNumRows() ? at / perScreen() : 0; }
        juce::Rectangle<int> cell(int n) const
        {
            const int columns = int(pane_.setlist_.columns), rows = int(pane_.setlist_.rows);
            const int gap = dense(pane_) ? denseGap : pane_.mode() == 2 ? 10 : 8;
            const int w = (getWidth() - gap * (columns - 1)) / columns, h = (getHeight() - gap * (rows - 1)) / rows;
            return {(n % columns) * (w + gap), (n / columns) * (h + gap), w, h};
        }
        int at(juce::Point<int> p) const
        {
            for(int n = 0; n < perScreen(); ++n)
                if(cell(n).contains(p)) { const int row = screen() * perScreen() + n; return row < pane_.getNumRows() ? pane_.first() + row : -1; }
            return -1;
        }
        void paint(juce::Graphics& g) override
        {
            const int m = pane_.mode();
            for(int n = 0; n < perScreen(); ++n)
            {
                const int row = screen() * perScreen() + n, index = pane_.first() + row;
                const auto r = cell(n).toFloat();
                const float radius = std::min(themeData().radius, 10.f);
                const auto* slot = row < pane_.getNumRows() ? pane_.slotAt(index) : nullptr;
                if(!slot || slot->empty) { g.setColour(ink(Ink::Surface).withAlpha(.5f)); g.fillRoundedRectangle(r, radius); continue; } // past the page's end, or an empty slot: its place kept
                const bool playing = index == pane_.current_;
                const auto color = pane_.colorAt(index);
                g.setColour(color.isTransparent() ? ink(Ink::Raised) : color.interpolatedWith(ink(Ink::Surface), playing ? .15f : .55f));
                g.fillRoundedRectangle(r, radius);
                if(playing) { g.setColour(ink(Ink::Accent)); g.drawRoundedRectangle(r.reduced(1.5f), radius, 3); }
                auto area = r.toNearestInt().reduced(10, 8);
                g.setColour(ink(Ink::Text).withAlpha(.7f)); g.setFont(juce::FontOptions(textSize(TextRole::Small, m), juce::Font::bold));
                g.drawText(juce::String(index + 1), area.removeFromTop(int(textSize(TextRole::Small, m)) + 4), juce::Justification::topLeft);
                g.setColour(pane_.opens(index) ? ink(Ink::Text) : ink(Ink::Muted));
                g.setFont(juce::FontOptions(textSize(TextRole::Title, m), juce::Font::bold));
                g.drawFittedText(pane_.nameAt(index), area, juce::Justification::centred, 3);
            }
        }
        void mouseUp(const juce::MouseEvent& e) override
        {
            const int index = at(e.getPosition());
            if(index < 0) return;
            if(e.mods.isPopupMenu()) pane_.slotMenu(index); else pane_.go(index);
        }
    private:
        SetlistPane& pane_;
    };

    SetlistPane::~SetlistPane() = default;

    SetlistPane::SetlistPane(Engine& engine, Actions& actions) : engine_(engine), actions_(actions)
    {
        chooser_.setTextWhenNothingSelected("No setlist");
        chooser_.onChange = [this] { const int i = chooser_.getSelectedItemIndex(); if(i >= 0 && size_t(i) < setlists_.size()) open(setlists_[size_t(i)].id); };
        pages_.onChange = [this] { if(pages_.getSelectedItemIndex() >= 0) showPage(pages_.getSelectedItemIndex()); };
        new_.onClick = [this] { ask("New setlist", {}, [this](std::string name) { create(name); }); };
        pageButton_.onClick = [this] { pageMenu(); };
        export_.onClick = [this] { exportSetlist(); };
        export_.setTooltip("Export this setlist with its Performances and everything they need, to take to another device");
        previous_.onClick = [this] { step(-1); };
        next_.onClick = [this] { step(1); };
        add_.onClick = [this] { addFromLibrary(); };
        gap_.onClick = [this] { addEmpty(); };
        gap_.setTooltip("An empty slot, to keep a place in the grid");
        remove_.onClick = [this] { remove(selected()); };
        up_.onClick = [this] { move(selected(), -1); };
        down_.onClick = [this] { move(selected(), 1); };
        view_.addItemList({"List", "Scrolling list", "Grid"}, 1);
        view_.onChange = [this] { auto d = display(); d.mode = SetlistDisplay::Mode(view_.getSelectedItemIndex()); setDisplay(d); };
        for(int i = 1; i <= maxSide; ++i) { columns_.addItem(juce::String(i) + (i == 1 ? " column" : " columns"), i); rows_.addItem(juce::String(i) + (i == 1 ? " row" : " rows"), i); }
        columns_.onChange = [this] { auto d = display(); d.columns = columns_.getSelectedId(); setDisplay(d); };
        rows_.onChange = [this] { auto d = display(); d.rows = rows_.getSelectedId(); setDisplay(d); };
        grid_ = std::make_unique<Grid>(*this);
        addChildComponent(*grid_);
        list_.setColour(juce::ListBox::backgroundColourId, ink(Ink::Surface));
        list_.setOutlineThickness(0);
        status_.setColour(juce::Label::textColourId, ink(Ink::Muted));
        for(auto* c : std::initializer_list<juce::Component*>{&chooser_, &pages_, &pageButton_, &export_, &view_, &columns_, &rows_, &new_, &previous_, &next_, &add_, &gap_, &remove_, &up_, &down_, &list_, &status_}) addAndMakeVisible(c);
        setWantsKeyboardFocus(true);
        refreshChooser();
        if(!setlists_.empty()) open(setlists_.front().id);
        showDisplay();
        updateButtons();
    }

    SetlistDisplay SetlistPane::display() const
    {
        const auto at = std::find(std::begin(modeNames), std::end(modeNames), setlist_.mode);
        return {SetlistDisplay::Mode(at == std::end(modeNames) ? 0 : at - std::begin(modeNames)), int(setlist_.columns), int(setlist_.rows)};
    }

    int SetlistPane::count() const { int n = 0; for(const auto& p : setlist_.pages) n += int(p.slots.size()); return n; }
    int SetlistPane::first() const { int n = 0; for(int p = 0; p < page_ && p < int(setlist_.pages.size()); ++p) n += int(setlist_.pages[size_t(p)].slots.size()); return n; }
    int SetlistPane::getNumRows() { return page_ < int(setlist_.pages.size()) ? int(setlist_.pages[size_t(page_)].slots.size()) : 0; }
    int SetlistPane::pageOf(int index) const
    {
        for(int p = 0; p < int(setlist_.pages.size()); ++p) { if(index < int(setlist_.pages[size_t(p)].slots.size())) return p; index -= int(setlist_.pages[size_t(p)].slots.size()); }
        return -1;
    }
    Control::SetlistSlot* SetlistPane::slotAt(int index)
    {
        if(index < 0) return nullptr;
        for(auto& p : setlist_.pages) { if(index < int(p.slots.size())) return &p.slots[size_t(index)]; index -= int(p.slots.size()); }
        return nullptr;
    }
    const Control::SetlistSlot* SetlistPane::slotAt(int index) const { return const_cast<SetlistPane*>(this)->slotAt(index); }
    bool SetlistPane::opens(int index) const
    {
        const auto* slot = slotAt(index);
        return slot && !slot->empty && !slot->id.empty() && size_t(index) < shown_.size() && !shown_[size_t(index)].missing;
    }

    juce::String SetlistPane::nameAt(int index) const
    {
        const auto* slot = slotAt(index);
        if(!slot || slot->empty) return {};
        if(!slot->label.empty()) return text(slot->label);
        const auto& name = size_t(index) < shown_.size() && !shown_[size_t(index)].name.empty() ? shown_[size_t(index)].name : slot->name;
        return text(name.empty() ? "Untitled" : name);
    }

    void SetlistPane::refreshChooser()
    {
        Control::LibraryQuery all; all.limit = 1000; all.types = {"oscairn.setlist"};
        if(const auto found = engine_.library(all)) setlists_ = found->items;
        chooser_.clear(juce::dontSendNotification);
        for(size_t i = 0; i < setlists_.size(); ++i) chooser_.addItem(text(setlists_[i].name.empty() ? "Untitled" : setlists_[i].name), int(i) + 1);
        for(size_t i = 0; i < setlists_.size(); ++i) if(setlists_[i].id == setlist_.info.id) chooser_.setSelectedItemIndex(int(i), juce::dontSendNotification);
        pages_.clear(juce::dontSendNotification);
        for(size_t p = 0; p < setlist_.pages.size(); ++p) pages_.addItem(text(setlist_.pages[p].name.empty() ? "Page " + std::to_string(p + 1) : setlist_.pages[p].name), int(p) + 1);
        pages_.setSelectedItemIndex(page_, juce::dontSendNotification);
    }

    void SetlistPane::refreshShown()
    {
        const auto slots = slotsOf(setlist_);
        shown_.assign(slots.size(), {});
        // Slots are Performances: asking for every type would spend the 1000-item read on others.
        Control::LibraryQuery all; all.limit = 1000; all.kind = "performance";
        const auto found = engine_.library(all);
        // A library larger than the read: what is not in it is unknown, not missing.
        const bool whole = found && found->total <= found->items.size();
        for(size_t i = 0; i < slots.size(); ++i)
        {
            if(slots[i].empty || slots[i].id.empty()) { shown_[i].missing = true; continue; } // an empty slot or a placeholder: nothing to open
            const auto at = found ? std::find_if(found->items.begin(), found->items.end(), [&](const auto& item) { return item.id == slots[i].id; }) : std::vector<Control::LibraryItem>::const_iterator();
            if(!found || at == found->items.end()) shown_[i].missing = whole;
            else { shown_[i].name = at->name; shown_[i].color = at->color; }
        }
    }

    void SetlistPane::open(const std::string& id)
    {
        Control::Setlist s;
        if(const auto error = engine_.setlist(id, s); !error.empty()) { say(text(error)); return; }
        setlist_ = std::move(s);
        current_ = -1; page_ = 0;
        refreshShown();
        list_.updateContent(); list_.deselectAllRows();
        refreshChooser();
        const int n = int(std::count_if(shown_.begin(), shown_.end(), [](const Shown& x) { return !x.missing; }));
        say(juce::String(n) + (n == 1 ? " Performance" : " Performances"));
        showDisplay(); updateButtons(); repaint();
    }

    void SetlistPane::create(const std::string& name)
    {
        for(const auto& s : setlists_) if(s.name == name && s.library.empty()) { open(s.id); return; }
        setlist_ = {};
        setlist_.info.name = name;
        setlist_.pages.push_back({"Page 1", {}});
        shown_.clear(); current_ = -1; page_ = 0;
        save();
        list_.updateContent();
        refreshChooser(); showDisplay(); updateButtons(); repaint();
    }

    void SetlistPane::save()
    {
        // While the engine opens a Performance, a change to a saved setlist waits its turn rather than being lost.
        if(!setlist_.info.id.empty() && engine_.connected() && engine_.soundBusy()) { engine_.edit([s = setlist_](Control& c) { c.saveSetlist(s); }); return; }
        std::string id;
        if(const auto error = engine_.saveSetlist(setlist_, id); !error.empty()) { say(text(error)); return; }
        const bool renewed = id != setlist_.info.id; // new, or a locked library's saved as the user's own
        setlist_.info.id = id;
        if(renewed) refreshChooser();
    }

    juce::String SetlistPane::add(const std::string& id)
    {
        const auto item = engine_.libraryItem(id);
        if(!item) { say("The engine is busy; add it again in a moment."); return {}; }
        if(item->id.empty() || item->kind != "performance") { say("That Performance is not in the library."); return {}; }
        if(count() >= maxSlots) { say("A setlist holds at most 1000 slots."); return {}; }
        if(setlist_.pages.empty()) create("Setlist");
        page_ = std::clamp(page_, 0, int(setlist_.pages.size()) - 1);
        setlist_.pages[size_t(page_)].slots.push_back({item->id, item->name, {}, {}});
        const int index = first() + getNumRows() - 1;
        if(current_ >= index) ++current_;
        refreshShown();
        save();
        list_.updateContent(); list_.selectRow(getNumRows() - 1);
        const auto done = "Added to " + text(setlist_.info.name);
        say(done); updateButtons(); repaint();
        return done;
    }

    void SetlistPane::addEmpty()
    {
        if(setlist_.pages.empty() || count() >= maxSlots) return;
        setlist_.pages[size_t(page_)].slots.push_back({{}, {}, {}, {}, true});
        if(current_ >= first() + getNumRows() - 1) ++current_;
        refreshShown(); save();
        list_.updateContent(); updateButtons(); repaint();
    }

    void SetlistPane::addFromLibrary()
    {
        const bool asked = actions_.request(pickPerformance, {}, [safe = juce::Component::SafePointer<SetlistPane>(this)](const std::string& id) {
            if(safe && !id.empty()) safe->add(id);
        });
        if(!asked) say("No app is installed that picks Performances.");
    }

    void SetlistPane::remove(int index)
    {
        const int p = pageOf(index);
        if(p < 0) return;
        auto& slots = setlist_.pages[size_t(p)].slots;
        int start = 0; for(int q = 0; q < p; ++q) start += int(setlist_.pages[size_t(q)].slots.size());
        slots.erase(slots.begin() + (index - start));
        if(current_ == index) current_ = -1; else if(current_ > index) --current_;
        refreshShown(); save();
        list_.updateContent();
        if(getNumRows() > 0) list_.selectRow(std::min(index - first(), getNumRows() - 1));
        updateButtons(); repaint();
    }

    void SetlistPane::move(int index, int by)
    {
        const int p = pageOf(index), to = index + by;
        if(p < 0 || pageOf(to) != p) return; // within its page
        auto* a = slotAt(index); auto* b = slotAt(to);
        std::swap(*a, *b);
        std::swap(shown_[size_t(index)], shown_[size_t(to)]);
        if(current_ == index) current_ = to; else if(current_ == to) current_ = index;
        save();
        list_.updateContent(); list_.selectRow(to - first());
        repaint();
    }

    void SetlistPane::go(int index)
    {
        const auto* slot = slotAt(index);
        if(!slot || slot->empty) return;
        if(!opens(index)) { say(nameAt(index) + (slot->id.empty() ? " is a placeholder: it has no Performance." : " is not in the library.")); return; }
        if(engine_.soundBusy()) { say("Still opening the last one."); return; }
        if(!engine_.connected()) { say("The engine is not answering; the slot did not open."); return; }
        SoundAction open; open.kind = SoundAction::LoadFromLibrary; open.libraryId = slot->id;
        engine_.soundAction(std::move(open));
        // The next one is prepared while this one plays, so going to it only switches.
        for(int next = index + 1; next < count(); ++next)
            if(opens(next)) { SoundAction prepare; prepare.kind = SoundAction::PrepareFromLibrary; prepare.libraryId = slotAt(next)->id; engine_.soundAction(std::move(prepare)); break; }
            else if(!slotAt(next)->empty) break; // a placeholder or a missing one comes first: nothing to prepare
        current_ = index;
        if(const int p = pageOf(index); p != page_) showPage(p);
        list_.selectRow(index - first());
        say(juce::String(index + 1) + ". " + nameAt(index));
        updateButtons(); repaint(); grid_->repaint();
        if(onCurrent) onCurrent();
    }

    void SetlistPane::step(int by)
    {
        // Empty slots are passed over; a placeholder or a missing Performance is stopped at, and says so.
        for(int i = (current_ < 0 ? (by > 0 ? -1 : count()) : current_) + by; i >= 0 && i < count(); i += by)
            if(!slotAt(i)->empty) { go(i); return; }
    }

    void SetlistPane::mark(int index)
    {
        const auto* slot = slotAt(index);
        if(!slot || slot->empty) return;
        current_ = index;
        if(const int p = pageOf(index); p != page_) showPage(p);
        list_.selectRow(index - first());
        say(juce::String(index + 1) + ". " + nameAt(index));
        updateButtons(); repaint(); grid_->repaint();
    }

    void SetlistPane::showPage(int page)
    {
        if(page < 0 || page >= int(setlist_.pages.size())) return;
        page_ = page;
        pages_.setSelectedItemIndex(page, juce::dontSendNotification);
        list_.updateContent(); list_.deselectAllRows();
        if(current_ >= first() && current_ < first() + getNumRows()) list_.selectRow(current_ - first());
        updateButtons(); repaint(); grid_->repaint();
    }

    void SetlistPane::addPage(const std::string& name)
    {
        if(setlist_.pages.size() >= 100) { say("A setlist holds at most 100 pages."); return; }
        setlist_.pages.push_back({name, {}});
        save(); refreshChooser(); showPage(int(setlist_.pages.size()) - 1);
    }

    void SetlistPane::renamePage(int page, const std::string& name)
    {
        if(page < 0 || page >= int(setlist_.pages.size())) return;
        setlist_.pages[size_t(page)].name = name;
        save(); refreshChooser();
    }

    void SetlistPane::removePage(int page)
    {
        if(page < 0 || page >= int(setlist_.pages.size()) || setlist_.pages.size() < 2) return;
        int start = 0; for(int q = 0; q < page; ++q) start += int(setlist_.pages[size_t(q)].slots.size());
        const int n = int(setlist_.pages[size_t(page)].slots.size());
        setlist_.pages.erase(setlist_.pages.begin() + page);
        if(current_ >= start && current_ < start + n) current_ = -1; else if(current_ >= start + n) current_ -= n;
        refreshShown(); save(); refreshChooser();
        showPage(std::min(page, int(setlist_.pages.size()) - 1));
    }

    void SetlistPane::exportSetlist()
    {
        if(setlist_.info.id.empty()) return;
        const auto name = text(setlist_.info.name.empty() ? "Setlist" : setlist_.info.name);
        fileChooser_ = std::make_unique<juce::FileChooser>("Export " + name, juce::File::getSpecialLocation(juce::File::userDocumentsDirectory).getChildFile(name + ".olib"), "*.olib");
        fileChooser_->launchAsync(juce::FileBrowserComponent::saveMode | juce::FileBrowserComponent::canSelectFiles | juce::FileBrowserComponent::warnAboutOverwriting,
            [safe = juce::Component::SafePointer<SetlistPane>(this), id = setlist_.info.id](const juce::FileChooser& fc) {
                if(!safe || fc.getResult() == juce::File()) return;
                auto file = fc.getResult(); if(!file.hasFileExtension("olib")) file = file.withFileExtension("olib");
                SoundAction out; out.kind = SoundAction::ExportPerformance; out.libraryId = id; out.directory = file.getFullPathName().toStdString();
                out.done = [safe](const std::string& error) { if(safe) safe->say(error.empty() ? juce::String("Exported.") : text(error)); };
                safe->engine_.soundAction(std::move(out));
                safe->say("Exporting...");
            });
    }

    bool SetlistPane::keyPressed(const juce::KeyPress& key)
    {
        // Page turners and foot switches send arrows or page keys.
        if(key == juce::KeyPress::rightKey || key == juce::KeyPress::pageDownKey || key == juce::KeyPress::spaceKey) { step(1); return true; }
        if(key == juce::KeyPress::leftKey || key == juce::KeyPress::pageUpKey) { step(-1); return true; }
        return false;
    }

    void SetlistPane::say(const juce::String& t) { message_ = t; status_.setText(t, juce::dontSendNotification); }

    void SetlistPane::updateButtons()
    {
        const int row = selected(), n = count();
        bool before = false, after = false;
        for(int i = 0; i < n; ++i) if(!slotAt(i)->empty) { before = before || (current_ >= 0 && i < current_); after = after || i > current_; }
        previous_.setEnabled(before); next_.setEnabled(after);
        remove_.setEnabled(row >= 0); up_.setEnabled(row >= 0 && pageOf(row - 1) == pageOf(row)); down_.setEnabled(row >= 0 && pageOf(row + 1) == pageOf(row));
        add_.setEnabled(actions_.offered(pickPerformance));
        export_.setEnabled(!setlist_.info.id.empty());
    }

    void SetlistPane::paintListBoxItem(int row, juce::Graphics& g, int width, int height, bool selected)
    {
        const int index = first() + row;
        const auto* slot = slotAt(index);
        if(!slot) return;
        const bool playing = index == current_;
        drawListRow(g, {0, 0, width, height}, row, colorAt(index));
        if(playing || selected) { g.setColour(playing ? ink(Ink::Accent).withAlpha(.22f) : ink(Ink::Raised).withAlpha(.6f)); g.fillRect(0, 0, width, height); }
        if(playing) { g.setColour(ink(Ink::Accent)); g.fillRect(0, 0, 4, height); }
        const float body = textSize(TextRole::Body, mode()), title = textSize(TextRole::Title, mode());
        auto area = juce::Rectangle<int>(0, 0, width, height).reduced(24, 0);
        g.setColour(playing ? ink(Ink::Accent) : ink(Ink::Muted)); g.setFont(juce::FontOptions(body, juce::Font::bold));
        g.drawText(juce::String(index + 1), area.removeFromLeft(int(body * 2.4f)), juce::Justification::centredLeft);
        g.setColour(opens(index) ? ink(Ink::Text) : ink(Ink::Muted)); g.setFont(juce::FontOptions(title, playing ? juce::Font::bold : juce::Font::plain));
        const juce::String note = slot->empty ? "(empty)" : slot->id.empty() ? "  (placeholder)" : !opens(index) ? "  (not in the library)" : "";
        g.drawText(slot->empty ? note : nameAt(index) + note, area, juce::Justification::centredLeft, true);
    }

    juce::Colour SetlistPane::colorAt(int index) const
    {
        const auto* slot = slotAt(index);
        if(!slot || slot->empty) return juce::Colours::transparentBlack;
        return colorOf(slot->color.empty() && size_t(index) < shown_.size() ? shown_[size_t(index)].color : slot->color);
    }

    void SetlistPane::setColor(int index, const std::string& color)
    {
        auto* slot = slotAt(index);
        if(!slot || slot->empty || !validColor(color)) return;
        slot->color = color;
        save(); repaint(); list_.repaint(); grid_->repaint();
    }

    void SetlistPane::setLabel(int index, const std::string& label)
    {
        auto* slot = slotAt(index);
        if(!slot || slot->empty) return;
        slot->label = label.substr(0, 256);
        save(); repaint(); list_.repaint(); grid_->repaint();
    }

    void SetlistPane::ask(const juce::String& title, const juce::String& initial, std::function<void(std::string)> then)
    {
        auto* w = new juce::AlertWindow(title, {}, juce::MessageBoxIconType::NoIcon, this);
        w->addTextEditor("name", initial, "Name");
        w->addButton("OK", 1, juce::KeyPress(juce::KeyPress::returnKey)); w->addButton("Cancel", 0, juce::KeyPress(juce::KeyPress::escapeKey));
        keepInScreen(*w);
        w->enterModalState(true, juce::ModalCallbackFunction::create([safe = juce::Component::SafePointer<SetlistPane>(this), w, then = std::move(then)](int result) {
            if(safe && result == 1) then(w->getTextEditorContents("name").trim().toStdString());
        }), true);
    }

    void SetlistPane::slotMenu(int index)
    {
        const auto* slot = slotAt(index);
        if(!slot) return;
        juce::PopupMenu menu, colors;
        if(!slot->empty)
        {
            addColors(colors, slot->color, colorMenuBase, "The Performance's color");
            menu.addSubMenu("Color", colors);
            menu.addItem(1, "Label...");
        }
        menu.addItem(2, "Remove");
        menu.showMenuAsync(juce::PopupMenu::Options(), [safe = juce::Component::SafePointer<SetlistPane>(this), index](int result) {
            if(!safe) return;
            std::string color;
            if(result == 1) safe->ask("Label", safe->slotAt(index) ? text(safe->slotAt(index)->label) : juce::String(), [safe, index](std::string label) { if(safe) safe->setLabel(index, label); });
            else if(result == 2) safe->remove(index);
            else if(colorChosen(result, colorMenuBase, color)) safe->setColor(index, color);
        });
    }

    void SetlistPane::pageMenu()
    {
        juce::PopupMenu menu;
        menu.addItem(1, "New page...");
        menu.addItem(2, "Rename page...", !setlist_.pages.empty());
        menu.addItem(3, "Remove page", setlist_.pages.size() > 1);
        menu.showMenuAsync(juce::PopupMenu::Options().withTargetComponent(&pageButton_), [safe = juce::Component::SafePointer<SetlistPane>(this)](int result) {
            if(!safe || safe->setlist_.pages.empty()) return;
            const int page = safe->page_;
            if(result == 1) safe->ask("New page", "Page " + juce::String(safe->setlist_.pages.size() + 1), [safe](std::string name) { if(safe) safe->addPage(name); });
            else if(result == 2) safe->ask("Rename page", text(safe->setlist_.pages[size_t(page)].name), [safe, page](std::string name) { if(safe) safe->renamePage(page, name); });
            else if(result == 3) safe->removePage(page);
        });
    }

    void SetlistPane::listBoxItemClicked(int row, const juce::MouseEvent& e)
    {
        if(e.mods.isPopupMenu()) slotMenu(first() + row);
        else if(setlist_.mode == "scroll") go(first() + row); // a list to tap
    }

    int SetlistPane::selected() const
    {
        if(setlist_.mode == "grid") return current_;
        const int row = list_.getSelectedRow();
        return row < 0 ? -1 : first() + row;
    }

    void SetlistPane::setDisplay(SetlistDisplay display)
    {
        setlist_.mode = modeNames[int(display.mode)];
        setlist_.columns = uint32_t(std::clamp(display.columns, 1, maxSide)); setlist_.rows = uint32_t(std::clamp(display.rows, 1, maxSide));
        if(!setlist_.pages.empty()) save();
        showDisplay();
    }

    void SetlistPane::showDisplay()
    {
        const auto d = display();
        const bool grid = d.mode == SetlistDisplay::Mode::Grid;
        view_.setSelectedItemIndex(int(d.mode), juce::dontSendNotification);
        columns_.setSelectedId(d.columns, juce::dontSendNotification); rows_.setSelectedId(d.rows, juce::dontSendNotification);
        columns_.setVisible(grid); rows_.setVisible(grid);
        grid_->setVisible(grid); list_.setVisible(!grid);
        previous_.setVisible(d.mode != SetlistDisplay::Mode::Scroll); next_.setVisible(d.mode != SetlistDisplay::Mode::Scroll);
        updateButtons(); resized(); repaint();
    }

    void SetlistPane::paint(juce::Graphics& g) { g.fillAll(ink(Ink::Background)); }

    void SetlistPane::resized()
    {
        const bool touch = mode() == 2;
        const bool tight = dense(*this); // a device's screen (RFC 0019)
        const int control = touch ? 44 : 30, gap = tight ? denseGap : touch ? 10 : 8, big = tight ? control : touch ? 72 : 56; // dense: Previous and Next a touch target tall, not more
        list_.setRowHeight(tight ? (touch ? 44 : 28) : touch ? 64 : 48);
        auto area = tight ? getLocalBounds().reduced(denseMargin) : getLocalBounds().reduced(16, 12);
        auto bar = area.removeFromTop(control);
        new_.setBounds(bar.removeFromRight(touch ? 90 : 70)); bar.removeFromRight(gap);
        export_.setBounds(bar.removeFromRight(touch ? 110 : 90)); bar.removeFromRight(gap);
        // A device's screen has the setlist, its page and the view on one line when they fit with a chooser worth reading.
        const bool oneLine = tight && bar.getWidth() - (columns_.isVisible() ? 570 : 360) - gap * (columns_.isVisible() ? 5 : 3) >= 160;
        if(oneLine)
        {
            rows_.setBounds(bar.removeFromRight(columns_.isVisible() ? 100 : 0)); if(columns_.isVisible()) bar.removeFromRight(gap);
            columns_.setBounds(bar.removeFromRight(columns_.isVisible() ? 110 : 0)); if(columns_.isVisible()) bar.removeFromRight(gap);
            view_.setBounds(bar.removeFromRight(140)); bar.removeFromRight(gap);
            pageButton_.setBounds(bar.removeFromRight(90)); bar.removeFromRight(gap);
            pages_.setBounds(bar.removeFromRight(130)); bar.removeFromRight(gap);
            chooser_.setBounds(bar);
        }
        else
        {
            chooser_.setBounds(bar.removeFromLeft(std::min(bar.getWidth() / 2, 280)));
            bar.removeFromLeft(gap);
            view_.setBounds(bar.removeFromLeft(touch ? 170 : 140));
            bar.removeFromLeft(gap);
            columns_.setBounds(bar.removeFromLeft(touch ? 140 : 110)); bar.removeFromLeft(gap);
            rows_.setBounds(bar.removeFromLeft(touch ? 120 : 90));
            area.removeFromTop(gap);
            auto pages = area.removeFromTop(control);
            pageButton_.setBounds(pages.removeFromRight(touch ? 110 : 90)); pages.removeFromRight(gap);
            pages_.setBounds(pages.removeFromLeft(std::min(pages.getWidth(), 280)));
        }
        area.removeFromTop(gap);
        // Previous and Next across the bottom, as big as the stage needs.
        if(previous_.isVisible())
        {
            auto stage = area.removeFromBottom(big);
            previous_.setBounds(stage.removeFromLeft(stage.getWidth() / 2 - gap / 2)); stage.removeFromLeft(gap);
            next_.setBounds(stage);
            area.removeFromBottom(gap);
        }
        auto edit = area.removeFromBottom(control);
        area.removeFromBottom(gap);
        for(auto* b : {&down_, &up_, &remove_, &gap_, &add_}) { b->setBounds(edit.removeFromRight(b == &gap_ ? (touch ? 130 : 100) : (touch ? 100 : 80))); edit.removeFromRight(gap); }
        status_.setFont(juce::FontOptions(textSize(TextRole::Small, mode())));
        status_.setBounds(edit);
        list_.setBounds(area);
        grid_->setBounds(area);
    }
}
