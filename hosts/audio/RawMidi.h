// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// ALSA raw MIDI input, for a Linux kernel without the ALSA sequencer (a FANTOM-0's), where JUCE's
// MIDI input, which reads only the sequencer, finds nothing. Linux only.

#include <chrono>
#include <cstdint>
#include <functional>
#include <string>
#include <thread>
#include <vector>

namespace oscairn::host
{
    /// Whether the ALSA sequencer opens (/dev/snd/seq). Where it does, JUCE's inputs are used.
    bool alsaSequencer();

    /// A raw MIDI input: one input subdevice of a card's raw MIDI device. `identifier` is its
    /// address, hw:CARD,DEVICE,SUBDEVICE, as `amidi -l` lists it; `name` is the name the
    /// sequencer would give its port (snd-seq-midi's: the subdevice's name, else the device's), so
    /// an input has the same name, and the musician's choices for it apply, with or without the
    /// sequencer.
    struct RawMidiPort
    {
        std::string identifier, name;
    };
    std::vector<RawMidiPort> rawMidiPorts(); // every card's, in card, device and subdevice order

    /// snd-seq-midi's name for port `subdevice` of a raw MIDI device of `ports` (the larger of
    /// its input and output subdevice counts), from the subdevice's and the device's names.
    std::string sequencerPortName( const std::string& subdeviceName, const std::string& deviceName, unsigned ports, int card, int device, unsigned subdevice );

    /// Opens a raw MIDI input by its identifier, non-blocking, as a source for the reader; an
    /// empty source (fd -1) if it does not open.
    struct RawMidiSource
    {
        int fd = -1;                 // read() and poll() on it
        std::function<void()> close; // called once, when the reader is done with it
    };
    RawMidiSource openRawMidi( const std::string& identifier );

    /// Reads raw MIDI sources on a thread of its own, so the audio and message threads never wait
    /// on a device, and cuts each one's bytes into messages (MidiStream.h). A source that ends or
    /// fails (a device unplugged) is closed and read no more. Devices coming and going: every
    /// `rescan` it lists the inputs with `scan` and calls `changed` once when the list differs from
    /// `seen` (the inputs when it started) or a source ended, so the engine opens them all again; at
    /// most once per `rescan`, so a device that keeps failing cannot spin. ALSA's raw MIDI has no
    /// notification of devices coming and going, and opening the cards' control devices once a
    /// second is cheap.
    class RawMidiReader
    {
    public:
        using Message = std::function<void( size_t source, const uint8_t* bytes, size_t size )>; // on the reader's thread
        RawMidiReader( std::vector<RawMidiSource> sources, Message message, std::vector<std::string> seen, std::function<std::vector<std::string>()> scan,
                       std::function<void()> changed, std::chrono::milliseconds rescan = std::chrono::seconds( 1 ) );
        ~RawMidiReader(); // stops the thread, then closes the sources still open

    private:
        void run();
        std::vector<RawMidiSource> sources_;
        Message message_;
        std::vector<std::string> seen_;
        std::function<std::vector<std::string>()> scan_;
        std::function<void()> changed_;
        std::chrono::milliseconds rescan_;
        int wake_[2] = { -1, -1 }; // a pipe: the destructor writes to stop the thread's poll
        std::thread thread_;
    };
}
