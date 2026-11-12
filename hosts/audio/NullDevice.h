// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <juce_audio_devices/juce_audio_devices.h>

#include <chrono>
#include <functional>
#include <thread>
#include <utility>

namespace oscairn::host
{
    /// An audio device that writes nowhere: the engine's `--audio null` (tests and checks; never a
    /// fallback), and the device selection test's devices. Paced, its own thread plays the callback
    /// at the pace of its rate and buffer, as a device's would; unpaced, its holder calls render().
    /// Its inputs are silent.
    class NullDevice : public juce::AudioIODevice, private juce::Thread
    {
    public:
        NullDevice( const juce::String& name, juce::Array<double> rates, juce::Array<int> sizes, int outputs = 2, int inputs = 0, bool paced = false )
            : juce::AudioIODevice( name, "Null" ), juce::Thread( "oscairn null audio" ), rates_( std::move( rates ) ), sizes_( std::move( sizes ) ),
              outputs_( outputs ), inputs_( inputs ), paced_( paced ) {}
        ~NullDevice() override { close(); }
        juce::StringArray getOutputChannelNames() override { return names( "Output", outputs_ ); }
        juce::StringArray getInputChannelNames() override { return names( "Input", inputs_ ); }
        juce::Array<double> getAvailableSampleRates() override { return rates_; }
        juce::Array<int> getAvailableBufferSizes() override { return sizes_; }
        int getDefaultBufferSize() override { return sizes_[0]; }
        juce::String open( const juce::BigInteger& inputs, const juce::BigInteger& outputs, double rate, int size ) override
        {
            close();
            rate_ = rate > 0 ? rate : rates_[0]; size_ = size > 0 ? size : getDefaultBufferSize();
            juce::BigInteger out, in; out.setRange( 0, outputs_, true ); in.setRange( 0, inputs_, true );
            activeOutputs_ = outputs & out; activeInputs_ = inputs & in;
            buffer_.setSize( outputs_ + inputs_, size_ ); buffer_.clear(); // every block's, made here, never on the device's thread
            open_ = true;
            return {};
        }
        void close() override { stop(); open_ = false; }
        bool isOpen() override { return open_; }
        void start( juce::AudioIODeviceCallback* callback ) override
        {
            if( !callback || !open_ || callback_ ) return;
            callback->audioDeviceAboutToStart( this );
            callback_ = callback; // before the thread starts, and cleared after it stops: never raced
            if( paced_ ) startRealtimeThread( juce::Thread::RealtimeOptions{}.withPeriodMs( size_ * 1000.0 / rate_ ) );
        }
        void stop() override
        {
            if( paced_ ) stopThread( 1000 );
            if( auto* c = std::exchange( callback_, nullptr ) ) c->audioDeviceStopped();
        }
        bool isPlaying() override { return callback_ != nullptr; }
        juce::String getLastError() override { return {}; }
        int getCurrentBufferSizeSamples() override { return size_; }
        double getCurrentSampleRate() override { return rate_; }
        int getCurrentBitDepth() override { return 32; }
        juce::BigInteger getActiveOutputChannels() const override { return activeOutputs_; }
        juce::BigInteger getActiveInputChannels() const override { return activeInputs_; }
        int getOutputLatencyInSamples() override { return 0; }
        int getInputLatencyInSamples() override { return 0; }
        /// One callback, played now; its first output channel, which the next callback overwrites.
        const float* render()
        {
            auto* const* channels = buffer_.getArrayOfWritePointers();
            for( int c = 0; c < outputs_; ++c ) juce::FloatVectorOperations::clear( channels[c], size_ );
            callback_->audioDeviceIOCallbackWithContext( channels + outputs_, inputs_, channels, outputs_, size_, {} );
            return channels[0];
        }

    private:
        void run() override
        {
            using Clock = std::chrono::steady_clock;
            const auto period = std::chrono::duration_cast<Clock::duration>( std::chrono::duration<double>( size_ / rate_ ) );
            auto next = Clock::now();
            while( !threadShouldExit() )
            {
                render();
                next += period;
                if( const auto now = Clock::now(); now - next > 4 * period ) next = now; // far behind (the machine stalled): on from now, never in a burst
                std::this_thread::sleep_until( next );
            }
        }
        static juce::StringArray names( const char* kind, int count )
        {
            juce::StringArray list;
            for( int i = 1; i <= count; ++i ) list.add( juce::String( kind ) + " " + juce::String( i ) );
            return list;
        }
        juce::Array<double> rates_;
        juce::Array<int> sizes_;
        const int outputs_, inputs_;
        const bool paced_;
        juce::BigInteger activeOutputs_, activeInputs_;
        juce::AudioBuffer<float> buffer_;
        double rate_ = 0;
        int size_ = 0;
        bool open_ = false;
        juce::AudioIODeviceCallback* callback_ = nullptr;
    };

    /// The devices a NullType lists, each made by `make` when opened or asked: output and input
    /// devices by name, those of `--audio null` or a test's. change() replaces them, as devices
    /// plugged in or out.
    class NullType : public juce::AudioIODeviceType
    {
    public:
        using Make = std::function<juce::AudioIODevice*( const juce::String& name )>;
        NullType( juce::StringArray outputs, juce::StringArray inputs, Make make )
            : juce::AudioIODeviceType( "Null" ), outputs_( std::move( outputs ) ), inputs_( std::move( inputs ) ), make_( std::move( make ) ) {}
        void scanForDevices() override {}
        juce::StringArray getDeviceNames( bool input ) const override { return input ? inputs_ : outputs_; }
        int getDefaultDeviceIndex( bool ) const override { return 0; }
        int getIndexOfDevice( juce::AudioIODevice* device, bool input ) const override { return device ? ( input ? inputs_ : outputs_ ).indexOf( device->getName() ) : -1; }
        bool hasSeparateInputsAndOutputs() const override { return true; }
        juce::AudioIODevice* createDevice( const juce::String& output, const juce::String& input ) override
        {
            if( output.isNotEmpty() ) return outputs_.contains( output ) ? make_( output ) : nullptr;
            return inputs_.contains( input ) ? make_( input ) : nullptr;
        }
        void change( juce::StringArray outputs ) { outputs_ = std::move( outputs ); callDeviceChangeListeners(); }

    private:
        juce::StringArray outputs_, inputs_;
        Make make_;
    };
}
