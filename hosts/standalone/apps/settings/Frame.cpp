// SPDX-License-Identifier: AGPL-3.0-or-later
#include "Frame.h"

namespace oscairn::standalone::settings
{
    namespace
    {
        // Round the frame's content and between its parts; a device's screen's least (RFC 0019).
        int margin(const juce::Component& c) { return dense(c) ? denseMargin : 16; }
        int gap(const juce::Component& c) { return dense(c) ? denseGap : 8; }
        int controlHeight(int mode) { return mode == 2 ? 44 : 30; }
        juce::Font font(TextRole role, int mode, bool bold = false) { return juce::Font(juce::FontOptions(textSize(role, mode), bold ? juce::Font::bold : juce::Font::plain)); }
        // An explanation wrapped to `width`, as drawn and as measured.
        juce::TextLayout wrapped(const juce::String& text, int width, int mode, juce::Colour color)
        {
            juce::AttributedString s;
            s.append(text, font(TextRole::Small, mode), color);
            s.setWordWrap(juce::AttributedString::byWord);
            juce::TextLayout layout;
            layout.createLayout(s, float(std::max(1, width)));
            return layout;
        }
    }

    Row::Row(const juce::String& label, juce::Component& control, const juce::String& explanation) : label_(label), explanation_(explanation), control_(control)
    {
        setTitle(label);
        control.setTitle(label);
        addAndMakeVisible(control);
    }

    void Row::setExplanation(const juce::String& text, bool danger)
    {
        if(text == explanation_ && danger == danger_) return;
        explanation_ = text; danger_ = danger;
        control_.setHelpText(text);
        if(auto* page = dynamic_cast<Page*>(getParentComponent())) page->heightChanged();
        resized(); repaint();
    }

    int Row::explanationHeight(int width) const
    {
        if(explanation_.isEmpty()) return 0;
        const int column = stacked(width) ? width : width - labelWidth(width);
        return 4 + int(std::ceil(wrapped(explanation_, column, mode(), {}).getHeight()));
    }

    int Row::heightFor(int width) const
    {
        const int m = mode();
        const int top = stacked(width) ? int(textSize(TextRole::Body, m) * 1.5f) + 4 : 0;
        return 2 * gap(*this) + top + controlHeight(m) + explanationHeight(width);
    }

    void Row::resized()
    {
        const int m = mode();
        auto area = getLocalBounds().reduced(0, gap(*this));
        if(stacked(getWidth())) area.removeFromTop(int(textSize(TextRole::Body, m) * 1.5f) + 4);
        else area.removeFromLeft(labelWidth(getWidth()));
        control_.setBounds(area.removeFromTop(controlHeight(m)).withWidth(std::min(area.getWidth(), 420)));
    }

    void Row::paint(juce::Graphics& g)
    {
        const int m = mode();
        const auto bounds = getLocalBounds().reduced(0, gap(*this));
        g.setColour(ink(Ink::Text));
        const auto labelBox = stacked(getWidth()) ? bounds.withHeight(int(textSize(TextRole::Body, m) * 1.5f))
                                                  : bounds.withWidth(labelWidth(getWidth()) - 12).withHeight(controlHeight(m));
        drawInked(g, label_, labelBox, font(TextRole::Body, m, true));
        if(explanation_.isNotEmpty())
        {
            const auto x = stacked(getWidth()) ? 0 : labelWidth(getWidth());
            const auto box = juce::Rectangle<int>(x, control_.getBottom() + 4, getWidth() - x, getHeight());
            wrapped(explanation_, box.getWidth(), m, danger_ ? ink(Ink::Danger) : ink(Ink::Muted)).draw(g, box.toFloat());
        }
        g.setColour(ink(Ink::Border).withAlpha(.35f));
        g.fillRect(getLocalBounds().removeFromBottom(1));
    }

    Row& Page::addRow(const juce::String& label, juce::Component& control, const juce::String& explanation)
    {
        rows_.push_back(std::make_unique<Row>(label, control, explanation));
        addAndMakeVisible(*rows_.back());
        return *rows_.back();
    }

    void Page::clearRows()
    {
        rows_.clear(); // the controls outlive their rows: a page owns them, and addRow takes them again
    }

    int Page::heightFor(int width) const
    {
        int h = 0;
        for(const auto& r : rows_) if(r->isVisible()) h += r->heightFor(width);
        return h;
    }

    void Page::resized()
    {
        int y = 0;
        for(const auto& r : rows_)
            if(r->isVisible()) { const int h = r->heightFor(getWidth()); r->setBounds(0, y, getWidth(), h); y += h; }
    }

    std::vector<const PageInfo*> shownPages(const std::vector<PageInfo>& all, const Profile& here)
    {
        std::vector<const PageInfo*> out;
        for(const auto& p : all) if(p.make && (!p.applies || p.applies(here))) out.push_back(&p);
        return out;
    }

    Frame::Frame(Engine& engine, const Profile& here, const std::vector<PageInfo>& all) : engine_(engine), shown_(shownPages(all, here))
    {
        setTitle("Settings");
        list_.setColour(juce::ListBox::backgroundColourId, juce::Colours::transparentBlack);
        list_.setTitle("Pages");
        back_.setTooltip("Back to the list of settings");
        back_.onClick = [this] { back(); };
        scroll_.setScrollBarsShown(true, false);
        addAndMakeVisible(list_); addChildComponent(back_); addAndMakeVisible(scroll_);
    }

    bool Frame::open(const std::string& id)
    {
        const auto it = std::find_if(shown_.begin(), shown_.end(), [&](const PageInfo* p) { return p->id == id; });
        if(it == shown_.end()) return false;
        const int at = int(it - shown_.begin());
        if(at != current_ || !page_)
        {
            scroll_.setViewedComponent(nullptr, false);
            page_ = (*it)->make(engine_);
            page_->setTitle((*it)->title);
            page_->onHeight = [this] { layoutPage(); };
            scroll_.setViewedComponent(page_.get(), false);
            current_ = at;
        }
        list_.selectRow(at, true, true);
        resized(); repaint();
        return true;
    }

    void Frame::back()
    {
        scroll_.setViewedComponent(nullptr, false);
        page_.reset(); current_ = -1;
        list_.deselectAllRows();
        resized(); repaint();
    }

    void Frame::layoutPage()
    {
        if(!page_) return;
        const int width = scroll_.getMaximumVisibleWidth();
        page_->setSize(width, page_->heightFor(width));
        page_->resized();
    }

    void Frame::resized()
    {
        const int m = mode();
        const int margin = settings::margin(*this), gap = settings::gap(*this);
        list_.setRowHeight(dense(*this) ? 44 : m == 2 ? 56 : 36); // a device's screen: a row a touch target tall
        auto area = getLocalBounds().reduced(margin);
        const int heading = int(textSize(TextRole::Title, m)) + 2 * gap;
        // Wide with a mouse, the first page is chosen at once: a column with nothing beside it says nothing.
        if(sideBySide() && !page_ && !shown_.empty()) { open(shown_.front()->id); return; }
        back_.setVisible(!sideBySide() && page_);
        list_.setVisible(sideBySide() || !page_);
        scroll_.setVisible(page_ != nullptr);
        if(sideBySide())
        {
            list_.setBounds(area.removeFromLeft(dense(*this) ? 200 : 240).withTrimmedTop(heading));
            area.removeFromLeft(2 * margin);
            title_ = area.removeFromTop(heading);
        }
        else if(page_)
        {
            title_ = area.removeFromTop(std::max(heading, controlHeight(m)));
            back_.setBounds(title_.removeFromLeft(controlHeight(m)).withSizeKeepingCentre(controlHeight(m), controlHeight(m)));
            title_.removeFromLeft(gap);
        }
        else { title_ = area.removeFromTop(heading); list_.setBounds(area); }
        scroll_.setBounds(page_ ? area : juce::Rectangle<int>());
        layoutPage();
    }

    void Frame::paint(juce::Graphics& g)
    {
        g.fillAll(ink(Ink::Background));
        const int m = mode();
        g.setColour(ink(Ink::Text));
        drawInked(g, page_ ? page_->getTitle() : juce::String("Settings"), title_, font(TextRole::Title, m, true));
        if(sideBySide()) // the list's own heading, over the column
        {
            g.setColour(ink(Ink::Muted));
            drawInked(g, "Settings", list_.getBounds().withY(title_.getY()).withHeight(title_.getHeight()), font(TextRole::Small, m, true));
        }
    }

    void Frame::paintListBoxItem(int row, juce::Graphics& g, int width, int height, bool selected)
    {
        if(row < 0 || size_t(row) >= shown_.size()) return;
        const int m = mode();
        const juce::Rectangle<int> r(0, 0, width, height);
        if(selected) { g.setColour(ink(Ink::Accent).withAlpha(.22f)); g.fillRoundedRectangle(r.toFloat().reduced(0, 1), buttonRadius(float(height))); }
        else { g.setColour(ink(Ink::Border).withAlpha(.35f)); g.fillRect(r.withTop(height - 1)); }
        auto text = r.reduced(12, 0);
        if(!sideBySide()) drawIcon(g, "chevron-right", text.removeFromRight(height / 2).toFloat().withSizeKeepingCentre(float(height) / 3, float(height) / 3), ink(Ink::Muted));
        g.setColour(selected ? ink(Ink::Accent) : ink(Ink::Text));
        drawInked(g, shown_[size_t(row)]->title, text, font(TextRole::Body, m, selected));
    }
}
