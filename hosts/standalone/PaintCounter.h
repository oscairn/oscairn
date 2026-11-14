// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <juce_gui_basics/juce_gui_basics.h>
#include <map>

namespace oscairn::standalone
{
    /// What the windows draw (RFC 0045: on a device the app's drawing slows the audio in other
    /// processes): for each window on the desktop, its frames (each time it is painted), the
    /// pixels painted in them (the frame's clip region, in the window's physical pixels) and
    /// the time its painting took. Message thread only. While one counts, macOS windows paint
    /// with JUCE's software renderer, as Linux's always do, so the pixels can be counted there too.
    class PaintCounter : private juce::ComponentListener, private juce::Timer
    {
    public:
        struct Counts { int frames = 0; juce::int64 pixels = 0; double ms = 0; };
        explicit PaintCounter(bool log = false); // log: a line a second for each window that drew, to stderr
        ~PaintCounter() override;
        void watch();                                     // every window on the desktop now, besides those already watched
        Counts counts(const juce::Component& window) const; // since the last reset
        Counts total() const;                             // every window's
        void reset();
        // OSCAIRN_UI_PAINT_LOG=1: one that logs, for the app to keep while it runs. Off: none.
        static std::unique_ptr<PaintCounter> fromEnvironment();

    private:
        void timerCallback() override;
        void componentPainted(juce::Component&, const juce::ComponentPaintDiagnostics&) override;
        void componentBeingDeleted(juce::Component&) override;

        std::map<juce::Component*, Counts> counts_, second_;
        bool log_;
        double since_ = juce::Time::getMillisecondCounterHiRes();
    };
}
