// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include "oscairn/session.h"
#include "ChannelMap.h"
#include "ControllerPorts.h"
#include "MidiBlock.h"
#ifdef __linux__
#include "RawMidi.h"
#endif
#if OSCAIRN_PANEL_ROLAND
#include "oscairn/panel_reader.h"
#endif

#include <juce_audio_devices/juce_audio_devices.h>

#include <atomic>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <vector>

namespace oscairn::host
{
    /// The demo instrument: the step sequencer and pads playing the basic synth, through
    /// the delay, on the default audio device, with every MIDI input as a source of notes,
    /// aftertouch and clock.
    ///
    /// Device ownership shared by the standalone in-process mode and engine helper.
    /// The UI and socket adapter reach the session through Control.
    class AudioEngine : private juce::AudioIODeviceCallback, private juce::MidiInputCallback, private juce::Timer, private juce::AsyncUpdater, private juce::ChangeListener
    {
    public:
        /// Loads the plugins from `pluginDir` and opens the user's saved audio device (audio.set),
        /// else the profile's (`audio`), else the default output. `devices`: a test's device type,
        /// listed and opened instead of the system's. With `audio.nullDevice` (--audio null) only the
        /// null device, and no MIDI device. Throws std::runtime_error if a plugin does not
        /// load or no audio output opens.
        explicit AudioEngine( const std::string& pluginDir, AudioConfiguration audio = {}, std::unique_ptr<juce::AudioIODeviceType> devices = {} );
        ~AudioEngine() override;

        Control& control() noexcept { return session_->control(); }

        /// Main-loop work: plugin callbacks, freeing retired graph plans, reopening MIDI
        /// inputs after devices change. Serviced by its own main-thread timer.
        void service();
        /// Called on the main thread after service() and after a control's move reached the model:
        /// the engine sends what changed then, not on a timer of its own.
        void setServiced( std::function<void()> serviced ) { serviced_ = std::move( serviced ); }

    private:
        // Audio thread
        void audioDeviceIOCallbackWithContext( const float* const* inputs, int numInputs, float* const* outputs, int numOutputs,
                                               int numSamples, const juce::AudioIODeviceCallbackContext& ) override;
        void audioDeviceAboutToStart( juce::AudioIODevice* ) override;
        void audioDeviceStopped() override { started_=false; }
        // MIDI input threads
        void handleIncomingMidiMessage( juce::MidiInput* input, const juce::MidiMessage& message ) override { incoming( message, input, -1 ); }
        // A message from a JUCE input, or from raw input `raw` (an index into midiSources_).
        void incoming( const juce::MidiMessage& message, const juce::MidiInput* input, int raw );

        uint8_t push( const juce::MidiMessage& message, const juce::MidiInput* input, int raw ); // the input's source number
        uint8_t sourceNumber( const juce::String& identifier, const std::vector<uint8_t>& taken ); // maxSources for none
        void handleAsyncUpdate() override { session_->serviceControllers(); if( serviced_ ) serviced_(); } // a control moved: its model, at once
        void openMidiInputs();
#ifdef __linux__
        void openRawMidiInputs();
#endif
        void openMidiOutputs();
        void queueMidiOut( uint32_t offset, double seconds ) noexcept; // audio thread: the session's MIDI out, for the sender
        void timerCallback() override { service(); }
        // The audio device (Control::audioDevices, setAudio), on the control thread.
        Control::AudioDevices listDevices();
        Control::AudioDevices setDevice( const Control::AudioSetting& setting );
        void applyRate(); // the device restarted at another rate: every plugin activated again at it
        void changeListenerCallback( juce::ChangeBroadcaster* ) override; // devices came or went

        juce::AudioDeviceManager devices_;
        std::atomic<double>      sampleRate_{ 48000.0 }; // the graph's: the sender thread reads it too
        std::atomic<double>      pendingRate_{ 0 };      // the device restarted at it: service() moves the graph to it
        static constexpr uint32_t maxFrames  = 512;

        std::unique_ptr<Session> session_;
#if OSCAIRN_PANEL_ROLAND
        std::unique_ptr<PanelReader> panel_; // the profile's panel, on its own thread, into the session's controls
#endif
        std::function<void()> serviced_;
        ChannelMap channelMap_;
        // External controllers (RFC 0024): their profiles and the musician's choices, and the
        // changes() last played (main thread).
        std::unique_ptr<ControllerPorts> controllers_;
        uint64_t controllerChanges_ = 0;
        // The device the engine plays: what a restart must find again. Recorded by the constructor and
        // by audioDeviceAboutToStart, which the device manager runs under its callback lock; adopt_,
        // set by the control thread under that lock, has the next start record it (a device chosen;
        // any device restarting when not fixed).
        std::string initialDevice_;
        juce::StringArray initialInputs_,initialOutputs_;
        bool adopt_ = false;
        // Control thread: where the device came from ("user", "profile", "default"), a choice passed
        // over, whether the profile fixes it (its channel map names that device's channels), the open
        // device's name, and the devices last listed, counted in status's audioDeviceList.
        std::string source_, notice_, opened_;
        bool fixed_ = false;
        juce::StringArray seenDevices_;
        uint64_t deviceList_ = 0;
        std::atomic<bool>        started_{ false }; // the device's rate matched the graph's
        // Callbacks that came more than a buffer late, counted as underruns where the device
        // reports none of its own (audio thread; reset when the device starts).
        std::atomic<uint64_t>    lateCallbacks_{ 0 };
        double                   lastCallback_ = 0;
        bool                     realtime_ = false; // the device's thread set to the engine's priority (Linux)

        // Physical MIDI to the audio thread: many producers, serialized by the mutex
        // (never taken on the audio thread), one consumer.
        struct Incoming
        {
            uint8_t bytes[3];
            uint8_t size; // bytes used
            uint8_t source; // the input's source number (note_zone.h)
            double  seconds; // Time::getMillisecondCounterHiRes() / 1000 at arrival
        };
        std::mutex                    pushLock_;
        juce::AbstractFifo            fifo_{ 1024 };
        std::array<Incoming, 1024>    incoming_{};
        std::atomic<uint64_t>         droppedMidi_{ 0 };
        std::atomic<bool> midiOverflow_{false};

        // Audio thread preallocated event buffers:
        MidiBlock notes_;
        std::array<Session::MidiClockEvent, 1024> clocks_{};

        // Audio thread: when the last block started (seconds and sample), and where the
        // next one starts, on the transport's timeline. Places MIDI clock in time.
        double  blockSeconds_   = 0.0;
        int64_t lastBlockStart_ = 0, blockSample_ = 0;

        // MIDI inputs, main thread; each with its source number. The input threads read both
        // under pushLock_, which the main thread holds to change them. A device keeps its
        // number while the app runs, so selectors resolved before it was unplugged still match.
        std::vector<std::unique_ptr<juce::MidiInput>> midiInputs_;
        std::vector<uint8_t>                          midiSources_;
        std::vector<std::string>                      midiNames_; // the inputs' names, in midiSources_' order
        std::map<juce::String, uint8_t>               sourceOf_; // by device identifier
#ifdef __linux__
        // No ALSA sequencer (a FANTOM-0's kernel): the inputs are raw MIDI devices instead of JUCE's,
        // decided once at startup, read by one reader in midiSources_' order.
        bool                                          rawMidi_ = false;
        std::unique_ptr<RawMidiReader>                rawInputs_;
#endif
        juce::MidiDeviceListConnection                midiDevicesChanged_;

        // MIDI outputs (RFC 0012), each numbered by device like the inputs, apart from them. The
        // audio thread queues each message with the wall-clock time it is due (its sample, plus the
        // audio output's latency, so it sounds with the block it belongs to); the sender thread
        // hands it to JUCE's output, which sends it then. The lock is the sender's and the main
        // thread's (reopening), never the audio thread's.
        struct Outgoing { double ms; uint64_t ports; uint8_t size; uint8_t bytes[3]; };
        juce::AbstractFifo                            outFifo_{ 4096 };
        std::array<Outgoing, 4096>                    outgoing_{};
        std::atomic<uint64_t>                         droppedOut_{ 0 };
        double                                        outputLatencyMs_ = 0; // set before the device starts
        std::mutex                                    outputLock_;
        std::vector<std::unique_ptr<juce::MidiOutput>> midiOutputs_; // started, each with its number
        std::vector<uint8_t>                          outputNumbers_;
        std::map<juce::String, uint8_t>               outputNumberOf_; // by device identifier
        struct Sender : juce::Thread
        {
            explicit Sender( AudioEngine& e ) : juce::Thread( "oscairn MIDI out" ), engine( e ) {}
            void run() override;
            AudioEngine& engine;
        } sender_{ *this };
        std::atomic<bool>                             midiDevicesDirty_{ false };
    };
}
