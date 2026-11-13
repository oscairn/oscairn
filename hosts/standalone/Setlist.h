// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Apps.h"
#include "Engine.h"
#include "Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone
{
    /// A setlist is a library item (RFC 0023, docs/setlist-format.md): named pages of slots, each
    /// a Performance by its library ID with the name it had when last seen, shown if it has left
    /// the library, a color and a label of its own in this setlist, or empty, or a placeholder.
    /// How it is shown is the setlist's too.
    struct SetlistDisplay
    {
        enum class Mode { List, Scroll, Grid } mode = Mode::List; // a list with Previous and Next; a list to scroll and tap; a grid with Previous and Next
        int columns = 4, rows = 3;                                 // the grid's, 1 to 8 each
    };
    // Every slot, page after page: what Previous and Next step through, by this index.
    std::vector<Control::SetlistSlot> slotsOf(const Control::Setlist& setlist);

    /// The Setlist app: a setlist's pages, one shown at a time, its slots in order, the one
    /// playing marked, with big Previous and Next for the stage. It reads and writes setlists
    /// through the library. Add asks whichever app picks Performances (the Library, RFC 0022);
    /// it also takes Performances other apps send.
    class SetlistPane : public juce::Component, private juce::ListBoxModel
    {
    public:
        SetlistPane(Engine& engine, Actions& actions);
        ~SetlistPane() override;

        void open(const std::string& id);        // a library setlist
        void create(const std::string& name);    // a new one, one page
        juce::String add(const std::string& id); // to the end of the page shown; what was done, or empty if it is not in the library
        void addEmpty();                         // an empty slot at the end of the page shown
        void addFromLibrary();                   // asks for a pick
        // By slot index (slotsOf): removing one, moving one within its page, opening one, its
        // color in this setlist (empty: the Performance's) and its label (empty: the name).
        void remove(int index);
        void move(int index, int by);
        void go(int index);
        void setColor(int index, const std::string& color);
        void setLabel(int index, const std::string& label);
        juce::Colour colorAt(int index) const;   // what it shows: its own, else its Performance's; transparent for none
        juce::String nameAt(int index) const;    // what it shows: its label, else its Performance's name, else the name last seen
        void setDisplay(SetlistDisplay display);
        void showPage(int page);
        void addPage(const std::string& name);
        void renamePage(int page, const std::string& name);
        void removePage(int page);               // and its slots; a setlist keeps at least one page
        void step(int by);                       // to the next or previous slot that opens, across pages
        void mark(int index);                    // the slot playing, without opening it (an app started again, RFC 0022)
        void exportSetlist();                    // with its Performances and all they need, as the Library exports it
        std::function<void()> onCurrent;         // the slot playing changed
        const Control::Setlist& setlist() const { return setlist_; }
        SetlistDisplay display() const;
        int current() const { return current_; }
        int page() const { return page_; }
        const juce::String& message() const { return message_; }

        void paint(juce::Graphics&) override;
        void resized() override;
        bool keyPressed(const juce::KeyPress&) override;

    private:
        class Grid;
        int getNumRows() override;
        void paintListBoxItem(int row, juce::Graphics&, int width, int height, bool selected) override;
        void listBoxItemClicked(int row, const juce::MouseEvent&) override;
        void listBoxItemDoubleClicked(int row, const juce::MouseEvent&) override { go(first() + row); }
        void returnKeyPressed(int row) override { go(first() + row); }
        void selectedRowsChanged(int) override { updateButtons(); }
        void save();
        void refreshChooser();
        void refreshShown();                     // names and colors as the library has them now
        void updateButtons();
        void say(const juce::String& text);
        void slotMenu(int index);
        void pageMenu();
        void ask(const juce::String& title, const juce::String& initial, std::function<void(std::string)> then);
        int selected() const;                    // what Remove and the moves act on, by slot index
        int first() const;                       // the slot index of the page shown's first slot
        int count() const;                       // slots in every page
        Control::SetlistSlot* slotAt(int index);
        const Control::SetlistSlot* slotAt(int index) const;
        int pageOf(int index) const;
        bool opens(int index) const;             // a Performance in the library, not an empty slot or a placeholder
        void showDisplay();
        int mode() const { return std::max(1, desktopMode(*this)); }

        Engine& engine_;
        Actions& actions_;
        Control::Setlist setlist_;
        struct Shown { bool missing = false; std::string name, color; }; // per slot, as the library has it
        std::vector<Shown> shown_;
        int current_ = -1, page_ = 0;
        juce::String message_;
        juce::ComboBox chooser_, pages_, view_, columns_, rows_;
        std::unique_ptr<Grid> grid_;
        std::unique_ptr<juce::FileChooser> fileChooser_;
        juce::TextButton new_{"New"}, pageButton_{"Page..."}, export_{"Export..."}, previous_{"Previous"}, next_{"Next"}, add_{"Add"}, gap_{"Add empty"}, remove_{"Remove"}, up_{"Up"}, down_{"Down"};
        juce::ListBox list_{"Setlist", this};
        juce::Label status_;
        std::vector<Control::LibraryItem> setlists_; // the chooser's
    };
}
