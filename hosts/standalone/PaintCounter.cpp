// SPDX-License-Identifier: AGPL-3.0-or-later
#include "PaintCounter.h"
#include "Theme.h"
#include <chrono>
#include <cstdio>

namespace oscairn::standalone
{
    namespace
    {
        juce::int64 pending = 0; // the pixels of the frame being painted, until its window says it was
    }

    PaintCounter::PaintCounter(bool log) : log_(log)
    {
        jassert(paintedPixels == nullptr); // one at a time
        paintedPixels = &pending;
        watch();
        if(log_) startTimer(1000);
    }

    PaintCounter::~PaintCounter()
    {
        for(const auto& [window, counts] : counts_) window->removeComponentListener(this);
        paintedPixels = nullptr;
    }

    void PaintCounter::watch()
    {
        auto& desktop = juce::Desktop::getInstance();
        for(int i = 0; i < desktop.getNumComponents(); ++i)
        {
            auto* window = desktop.getComponent(i);
            if(counts_.count(window)) continue;
            counts_[window] = {};
            window->addComponentListener(this);
            pending = 0; // painted before it was watched: not counted
#if JUCE_MAC
            if(auto* peer = window->getPeer(); peer && peer->getCurrentRenderingEngine() != 0) peer->setCurrentRenderingEngine(0);
#endif
        }
    }

    PaintCounter::Counts PaintCounter::counts(const juce::Component& window) const
    {
        const auto found = counts_.find(const_cast<juce::Component*>(&window));
        return found == counts_.end() ? Counts{} : found->second;
    }

    PaintCounter::Counts PaintCounter::total() const
    {
        Counts all;
        for(const auto& [window, c] : counts_) { all.frames += c.frames; all.pixels += c.pixels; all.ms += c.ms; }
        return all;
    }

    void PaintCounter::reset()
    {
        for(auto& [window, c] : counts_) c = {};
        second_.clear();
        pending = 0;
    }

    void PaintCounter::componentPainted(juce::Component& window, const juce::ComponentPaintDiagnostics& d)
    {
        const auto ms = d.totalPaintDuration.get<std::chrono::duration<double, std::milli>>();
        for(auto* c : {&counts_[&window], &second_[&window]}) { ++c->frames; c->pixels += pending; c->ms += ms; }
        pending = 0;
    }

    void PaintCounter::componentBeingDeleted(juce::Component& window)
    {
        window.removeComponentListener(this);
        counts_.erase(&window);
        second_.erase(&window);
    }

    void PaintCounter::timerCallback()
    {
        watch();
        const auto now = juce::Time::getMillisecondCounterHiRes(), seconds = (now - since_) / 1000.0;
        since_ = now;
        for(const auto& [window, c] : second_)
        {
            auto name = window->getTitle().isNotEmpty() ? window->getTitle() : window->getName();
            std::fprintf(stderr, "oscairn: UI paint \"%s\": %.1f frames/s, %.0f pixels/s, %.2f ms/s\n", name.toRawUTF8(),
                         c.frames / seconds, double(c.pixels) / seconds, c.ms / seconds);
        }
        second_.clear();
    }

    std::unique_ptr<PaintCounter> PaintCounter::fromEnvironment()
    {
        if(juce::SystemStats::getEnvironmentVariable("OSCAIRN_UI_PAINT_LOG", {}) != "1") return nullptr;
        return std::make_unique<PaintCounter>(true);
    }
}
