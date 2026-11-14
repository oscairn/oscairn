// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "../../Engine.h"
#include "../../Icons.h"
#include "../../Profile.h"
#include "../../Theme.h"
#include <juce_gui_basics/juce_gui_basics.h>

namespace oscairn::standalone::settings
{
    /// One setting: its label, its control, and an optional line under them saying what it does
    /// or why it is as it is (in the danger color for a failure). Every page is made of these, so
    /// every page lines up the same: the label in the left two fifths (280 at most), the control beside it;
    /// on a narrow page the label above its control.
    class Row : public juce::Component
    {
    public:
        Row(const juce::String& label, juce::Component& control, const juce::String& explanation = {});
        void setExplanation(const juce::String& text, bool danger = false);
        const juce::String& label() const { return label_; }
        const juce::String& explanation() const { return explanation_; }
        bool danger() const { return danger_; }
        int heightFor(int width) const;
        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        int mode() const { return std::max(1, desktopMode(*this)); }
        bool stacked(int width) const { return width < 480; }
        static int labelWidth(int width) { return std::min(width * 2 / 5, 280); }
        int explanationHeight(int width) const;
        juce::String label_, explanation_;
        juce::Component& control_;
        bool danger_ = false;
    };

    /// A page of Settings: its rows, top to bottom, in the frame's scrolling area under its title.
    /// A page is one small class (AudioPage.h is the example): it makes its controls, adds a Row
    /// for each, and keeps them current.
    class Page : public juce::Component
    {
    public:
        Row& addRow(const juce::String& label, juce::Component& control, const juce::String& explanation = {});
        void clearRows(); // drop every row (their controls, which the page owns, stay); for a list whose rows come and go
        int heightFor(int width) const; // its rows' (the hidden ones left out), for the scrolling area
        void resized() override;
        // Its height changed (a row shown or hidden, an explanation changed): the frame lays it out again.
        void heightChanged() { if(onHeight) onHeight(); }
        std::function<void()> onHeight;

    private:
        std::vector<std::unique_ptr<Row>> rows_;
    };

    /// A page as the list knows it (Pages.cpp): its id, its title, where it applies and how it is
    /// made. `applies` reads the device's profile (its OS is platform.os: macos, linux, windows;
    /// a device's image has a platform.runtime, the desktop none); null: everywhere. A page this
    /// build does not have is left out of the list where it is not built (#if, as RFC 0036 does
    /// for Bluetooth). `make` null: a page an RFC names that is not built yet, never shown.
    struct PageInfo
    {
        std::string id;
        juce::String title;
        std::function<bool(const Profile&)> applies;
        std::function<std::unique_ptr<Page>(Engine&)> make;
    };
    const std::vector<PageInfo>& pages();                                                       // the one list, in order
    std::vector<const PageInfo*> shownPages(const std::vector<PageInfo>& all, const Profile& here); // built, and applying here

    /// The Settings app's window content: the list of pages and the page chosen, under its title
    /// in a scrolling area. Wide, with a mouse, the list is a column at the left and a page shows
    /// beside it; narrow or with touch, the list fills the window and a page replaces it, with
    /// Back (design-language.md's 30 and 44 pixel controls, 8-pixel spacing, 16-pixel margins).
    class Frame : public juce::Component, private juce::ListBoxModel
    {
    public:
        Frame(Engine& engine, const Profile& here, const std::vector<PageInfo>& all = pages());
        const std::vector<const PageInfo*>& shown() const { return shown_; }
        bool open(const std::string& id); // false: not shown here
        void back();                       // to the list, where it replaces the page
        void scrollPage(int by) { pageList(list_, by); } // the list a page down, or up when negative (pageList)
        const PageInfo* current() const { return current_ < 0 ? nullptr : shown_[size_t(current_)]; }
        Page* page() const { return page_.get(); }
        bool listing() const { return list_.isVisible() && !page_; } // only the list shows
        void paint(juce::Graphics&) override;
        void resized() override;

    private:
        int mode() const { return std::max(1, desktopMode(*this)); }
        bool sideBySide() const { return (mode() == 1 || dense(*this)) && getWidth() >= 720; } // a device's screen: as wide, side by side too
        int getNumRows() override { return int(shown_.size()); }
        void paintListBoxItem(int row, juce::Graphics&, int width, int height, bool selected) override;
        void listBoxItemClicked(int row, const juce::MouseEvent&) override { open(shown_[size_t(row)]->id); }
        void returnKeyPressed(int row) override { open(shown_[size_t(row)]->id); }
        void layoutPage();

        Engine& engine_;
        std::vector<const PageInfo*> shown_;
        int current_ = -1;
        std::unique_ptr<Page> page_;
        juce::ListBox list_{"Settings", this};
        juce::Viewport scroll_;
        IconButton back_{IconButton::Glyph::Back, "Back"};
        juce::Rectangle<int> title_;
    };
}
