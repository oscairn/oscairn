// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AudioEngine.h"
#include "NullDevice.h"
#include "WebTransport.h"
#include "../standalone/Profile.h"

#include "oscairn/midi_input.h"
#include "oscairn/realtime.h"

#include <algorithm>
#include <cmath>
#include <fstream>
#include <set>
#include <sstream>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#ifdef __linux__
#include <alsa/asoundlib.h>
#endif

namespace oscairn::host
{
    namespace
    {
        // The name JUCE lists a device by. On Linux a profile may name an ALSA device by its
        // address (hw:1,0, or any PCM name such as hw:CARD=Dummy,DEV=0): JUCE lists ALSA's name
        // hints by their descriptions, so the address becomes the description of its hint.
        // Anything else (a Core Audio device's name, a description) is returned as given.
        std::string listedName( const std::string& name )
        {
#ifdef __linux__
            if( name.empty() ) return name;
            // ALSA hints at hw devices only with defaults.namehint.extended on (the image's
            // /etc/asound.conf), and JUCE opens only what is hinted.
            int card = -1, device = -1;
            char tail = 0;
            const bool numbered = std::sscanf( name.c_str(), "hw:%d,%d%c", &card, &device, &tail ) == 2;
            void** hints = nullptr;
            if( snd_device_name_hint( -1, "pcm", &hints ) < 0 ) return name;
            std::string found;
            for( auto** h = reinterpret_cast<char**>( hints ); *h && found.empty(); ++h )
            {
                char* id = snd_device_name_get_hint( *h, "NAME" );
                char* description = snd_device_name_get_hint( *h, "DESC" );
                bool match = id && name == id;
                if( !match && numbered && id )
                {
                    char cardName[64] = {};
                    int hintDevice = -1;
                    if( std::sscanf( id, "hw:CARD=%63[^,],DEV=%d%c", cardName, &hintDevice, &tail ) == 2 )
                        match = hintDevice == device && snd_card_get_index( cardName ) == card;
                }
                if( match )
                {
                    found = description && *description ? description : id; // as JUCE names it
                    for( size_t at; ( at = found.find( '\n' ) ) != std::string::npos; ) found.replace( at, 1, "; " );
                }
                std::free( id );
                std::free( description );
            }
            snd_device_name_free_hint( hints );
            if( !found.empty() ) return found;
#endif
            return name;
        }
        // The user's choice (audio.set): audio.json in the data folder, only what they chose. Unreadable: none.
        std::filesystem::path choiceFile() { return dataHome() / "audio.json"; }
        Control::AudioSetting readChoice()
        {
            Control::AudioSetting s;
            try
            {
                const auto v = juce::JSON::parse( juce::File( choiceFile().string() ).loadFileAsString() );
                if( !v.isObject() || v["format"] != "oscairn.audio" ) return s;
                if( v["output"].isString() ) s.output = v["output"].toString().toStdString();
                if( v["input"].isString() ) s.input = v["input"].toString().toStdString();
                if( double( v["sampleRate"] ) > 0 ) s.sampleRate = double( v["sampleRate"] );
                if( int( v["bufferSize"] ) > 0 ) s.bufferSize = uint32_t( int( v["bufferSize"] ) );
            }
            catch( const std::exception& ) {} // no home folder
            return s;
        }
        void writeChoice( const Control::AudioSetting& s )
        {
            auto* o = new juce::DynamicObject;
            const juce::var v( o );
            o->setProperty( "format", "oscairn.audio" );
            o->setProperty( "version", 1 );
            if( s.output ) o->setProperty( "output", juce::String::fromUTF8( s.output->c_str() ) );
            if( s.input ) o->setProperty( "input", juce::String::fromUTF8( s.input->c_str() ) );
            if( s.sampleRate ) o->setProperty( "sampleRate", *s.sampleRate );
            if( s.bufferSize ) o->setProperty( "bufferSize", int( *s.bufferSize ) );
            writeAtomically( choiceFile(), juce::JSON::toString( v ).toStdString() + "\n" );
        }
        juce::StringArray deviceNames( juce::AudioDeviceManager& devices )
        {
            juce::StringArray names;
            if( auto* type = devices.getCurrentDeviceTypeObject() ) { names = type->getDeviceNames( false ); names.add( {} ); names.addArray( type->getDeviceNames( true ) ); }
            return names;
        }
    }

    AudioEngine::AudioEngine( const std::string& pluginDir, AudioConfiguration audio, std::unique_ptr<juce::AudioIODeviceType> devices ) : channelMap_(std::move(audio),maxFrames)
    {
        useWebTransport(); // package repositories on the web (RFC 0031)
        juce::AudioDeviceManager::AudioDeviceSetup preferred;
        const auto& config=channelMap_.configuration();
        controllers_=std::make_unique<ControllerPorts>(config.controllers); // a bad profile file refuses to start before any device opens
        // A channel map names one device's channels: the profile's, the only one. --audio null is the only one too, and no choice of the user's applies to it.
        fixed_=config.explicitOutputs || config.nullDevice;
        auto choice=config.nullDevice?Control::AudioSetting{}:readChoice();
        if(fixed_){choice.output.reset();choice.input.reset();}
        preferred.bufferSize = choice.bufferSize ? int(*choice.bufferSize) : config.bufferFrames ? int(config.bufferFrames) : OSCAIRN_AUDIO_BUFFER_FRAMES;
        if( config.sampleRate ) preferred.sampleRate = config.sampleRate;
        else if( choice.sampleRate ) preferred.sampleRate = *choice.sampleRate;
        // A profile's device opens all of its channels in each direction used, mapped or not.
        const auto inputs=config.inputs.empty()?0:std::max(ChannelMap::required(config.inputs),config.deviceChannels);
        const auto outputs=std::max(ChannelMap::required(config.outputs),config.deviceChannels);
        preferred.useDefaultInputChannels=false;preferred.useDefaultOutputChannels=false;
        preferred.inputChannels.setRange(0,inputs,true);preferred.outputChannels.setRange(0,outputs,true);
        if(config.nullDevice) // the device a profile names becomes the null device, with its channels; nothing else is listed
        {
            juce::Array<double> rates{48000,44100,88200,96000}; // the first is its default
            if(config.sampleRate)rates.addIfNotAlreadyThere(config.sampleRate);
            juce::Array<int> sizes{64,128,256,512};
            if(preferred.bufferSize>0)sizes.addIfNotAlreadyThere(preferred.bufferSize);
            devices=std::make_unique<NullType>(juce::StringArray{"null"},inputs?juce::StringArray{"null"}:juce::StringArray{},[=](const juce::String& name){
                return new NullDevice(name,rates,sizes,int(outputs),int(inputs),true);});
        }
        if( devices ) devices_.addAudioDeviceType( std::move( devices ) ); // before the first scan, so the system's are never made
        // The user's choice, then the profile's device (or the options'), then the default; each passed over is noted.
        const auto inputName=listedName(config.inputDevice),outputName=listedName(config.outputDevice);
        struct Candidate { std::string output, input, source; };
        std::vector<Candidate> candidates;
        if(choice.output || (choice.input && inputs)) candidates.push_back({choice.output.value_or(outputName),inputs?choice.input.value_or(inputName):std::string(),"user"});
        if(!outputName.empty() || !inputName.empty()) candidates.push_back({outputName,inputName,"profile"});
        if(!fixed_ || candidates.empty()) candidates.push_back({{},{},"default"});
        if(config.nullDevice) candidates={{"null",inputs?"null":"","null"}};
        for(const auto& c:candidates)
        {
            preferred.inputDeviceName=c.input;preferred.outputDeviceName=c.output;
            auto error = devices_.initialise(inputs,outputs,nullptr,c.input.empty() && c.output.empty(),{},&preferred);
            juce::AudioDeviceManager::AudioDeviceSetup actual;devices_.getAudioDeviceSetup(actual);
            if(error.isEmpty() && !devices_.getCurrentAudioDevice())error="no device";
            if(error.isEmpty() && ((!c.input.empty() && actual.inputDeviceName.toStdString()!=c.input) || (!c.output.empty() && actual.outputDeviceName.toStdString()!=c.output)))
                error="another device opened";
            if(error.isEmpty()){source_=c.source;break;}
            notice_+=(notice_.empty()?"":"; ")+std::string(c.source=="user"?"the device chosen":c.source=="profile"?"the profile's device":"the default device")+
                (c.output.empty() && c.input.empty()?"":" "+(c.output.empty()?c.input:c.output))+" did not open: "+error.toStdString();
            devices_.closeAudioDevice();
        }
        auto*      device = devices_.getCurrentAudioDevice();
        if( source_.empty() || !device ) throw std::runtime_error( "no audio output: " + notice_ );
        if( !notice_.empty() ) std::fprintf( stderr, "oscairn: %s\n", notice_.c_str() );
        sampleRate_ = device->getCurrentSampleRate();
        std::fprintf( stderr, "oscairn: audio output \"%s\", %.0f Hz, %d-sample buffer\n", device->getName().toRawUTF8(), sampleRate_.load(),
                      device->getCurrentBufferSizeSamples() );
        if( preferred.bufferSize > 0 && device->getCurrentBufferSizeSamples() != preferred.bufferSize )
            std::fprintf( stderr, "oscairn: requested %d frames; using the device's supported size above\n", preferred.bufferSize );
        initialDevice_=opened_=device->getName().toStdString();initialInputs_=device->getInputChannelNames();initialOutputs_=device->getOutputChannelNames();
        if(config.sampleRate && std::abs(device->getCurrentSampleRate()-config.sampleRate)>=.5)
            throw std::runtime_error("the audio device runs at "+std::to_string(int(device->getCurrentSampleRate()))+" Hz, not the "+std::to_string(config.sampleRate)+" Hz asked for");
        audioDeviceAboutToStart(device);
        if(!started_)throw std::runtime_error("Requested audio channels are unavailable; no output fallback is permitted for explicit mappings");

        session_        = std::make_unique<Session>( sampleRate_, maxFrames, Lv2BufferLimits{}, channelMap_.layout() );
        session_->setBundledPlugins( pluginDir );
        const auto seq   = session_->add( "sequencer", pluginDir + "/oscairn-step-sequencer.clap" );
        const auto synth = session_->add( "synth", pluginDir + "/oscairn-basic-synth.clap" );
        const auto delay = session_->add( "delay", pluginDir + "/oscairn-delay.clap" );
        // The startup Performance, in parts like any other: a Synth part the keys play (the
        // Basic Synth into the Delay), and a Groovebox part whose step sequencer plays the same
        // synth. The nodes keep the names the Groovebox and the panel layouts bind to.
        const auto synthPart = session_->addPart( "Synth" );
        session_->insertIntoPart( synthPart, 0, synth );
        session_->insertIntoPart( synthPart, 1, delay );
        PartInput generates; generates.kind = PartInput::Kind::None;
        const auto groovebox = session_->addPart( "Groovebox", generates );
        session_->insertIntoPart( groovebox, 0, seq );
        session_->connectNotes( seq, synth );
        session_->commit();
        session_->clearHistory(); // undo starts from the startup instrument

        // RFC 0011's instant controls: a control's move that waits for the control thread wakes it.
        session_->setControllerWake( [this] { triggerAsyncUpdate(); } );
        // The device's own control groups, which give its panel's controls their default targets
        // (RFC 0024: sliders on part levels): without the app nothing else sends them, and a
        // control no group names plays nothing. An app that attaches sends its own in their place.
        if( !config.profile.empty() )
        {
            session_->setControllers( standalone::controlGroups( standalone::readProfile( config.profile ) ), {} );
            std::fprintf( stderr, "oscairn: control groups from the profile %s, until an app sends its own\n", config.profile.c_str() );
        }
        if( !config.panelDevice.empty() )
        {
            // A device panel's own controls, played as a controller's are, by their identities.
            panel_ = openPanel( config.panelDevice, config.panelMap, [this]( const std::string& control, const ControlInput& input ) {
                session_->controllerControl( control, input );
            }, [this]( uint32_t pad, bool on ) {
                { std::lock_guard lock( padLock_ ); pads_.emplace_back( pad, on ); }
                triggerAsyncUpdate();
            } );
            if( !panel_ ) throw std::runtime_error( "this build reads no panel (OSCAIRN_ROLAND_FANTOM0 is off)" );
        }
        // Like the panel, read with --audio null too: a device named on purpose, not one found.
        if( !config.keyboardDevice.empty() )
        {
            const auto inputs = keyboardInputs();
            if( inputs.empty() ) throw std::runtime_error( "this build reads no keyboard (OSCAIRN_ROLAND_FANTOM0 is off)" );
            for( const auto& input : inputs )
            {
                keyboardNames_.push_back( input.name );
                keyboardSources_.push_back( sourceNumber( input.key, keyboardSources_ ) );
            }
        }
#ifdef __linux__
        // Decided once: a kernel either has the sequencer or not. Where it has, its ports are the
        // raw devices', so never both.
        rawMidi_ = !config.nullDevice && !alsaSequencer();
        if( rawMidi_ ) std::fprintf( stderr, "oscairn: no ALSA sequencer; MIDI inputs are read as ALSA raw MIDI\n" );
#endif
        openMidiInputs();
        if( !config.keyboardDevice.empty() )
            keyboard_ = openKeyboard( config.keyboardDevice, [this]( size_t input, const uint8_t* bytes, size_t size ) {
                incoming( juce::MidiMessage( bytes, int( size ) ), nullptr, -1, keyboardSources_[input] );
            }, config.keyboardLocalOff );
        openMidiOutputs();
        sender_.startThread();
        midiDevicesChanged_ = juce::MidiDeviceListConnection::make( [this] { midiDevicesDirty_ = true; } );
        session_->setAudioHost( [this] { return listDevices(); }, [this]( const Control::AudioSetting& s ) { return setDevice( s ); } );
        // External controllers (RFC 0024): the musician's choices and ignored ports, kept here.
        session_->setControllerHost( { [this] { return controllers_->list(); },
            [this]( const std::string& input, const std::optional<std::vector<std::string>>& profiles ) { return controllers_->choose( input, profiles ); },
            [this]( const std::string& input, bool ignored ) { return controllers_->ignore( input, ignored ); } } );
        seenDevices_ = deviceNames( devices_ );
        devices_.addChangeListener( this );
        devices_.addAudioCallback( this );
        // RFC 0031: the packages that come with the release, a repository snapshot in the app's
        // Resources (macOS) or beside the programs (Linux), installed once per release as jobs.
        const auto here = juce::File::getSpecialLocation( juce::File::currentExecutableFile ).getParentDirectory();
        const auto shipped = here.getSiblingFile( "Resources" ).getChildFile( "Preinstalled" );
        session_->preinstall( ( shipped.isDirectory() ? shipped : here.getChildFile( "Preinstalled" ) ).getFullPathName().toStdString() );
        service();
        startTimer( 5 ); // control ticks are independent of the panel's repaint rate
    }

    AudioEngine::~AudioEngine()
    {
        stopTimer();
        panel_.reset(); // its thread plays the session's controls
        keyboard_.reset();
        session_->setControllerWake( {} );
        session_->setAudioHost( {}, {} );
        session_->setControllerHost( {} );
        cancelPendingUpdate();
        devices_.removeChangeListener( this );
        devices_.removeAudioCallback( this ); // waits for the callback to finish
        sender_.stopThread( 1000 );
        for( auto& input : midiInputs_ )
            input->stop();
        midiInputs_.clear();
#ifdef __linux__
        rawInputs_.reset();
#endif
        midiOutputs_.clear();
    }

    void AudioEngine::applyRate()
    {
        if( const double rate = pendingRate_.exchange( 0 ); rate > 0 )
        {
            try { session_->setSampleRate( rate ); }
            catch( const std::exception& e ) { notice_ = e.what(); std::fprintf( stderr, "oscairn: %s\n", e.what() ); }
            sampleRate_ = rate;
            started_ = true;
            std::fprintf( stderr, "oscairn: audio now at %.0f Hz; plugins activated again at it\n", rate );
        }
    }

    void AudioEngine::changeListenerCallback( juce::ChangeBroadcaster* )
    {
        if( auto names = deviceNames( devices_ ); names != seenDevices_ ) { seenDevices_ = std::move( names ); ++deviceList_; }
        // The device went away: the device manager opened the default instead (played when not fixed), or none.
        auto* device = devices_.getCurrentAudioDevice();
        if( const auto name = device ? device->getName().toStdString() : std::string(); name != opened_ )
        {
            notice_ = opened_ + " went away" + ( name.empty() ? std::string( "; no audio device is open" ) : fixed_ ? "; the profile fixes the device, so nothing plays" : "; " + name + " plays instead" );
            if( !fixed_ && !name.empty() ) source_ = "default";
            opened_ = name;
            std::fprintf( stderr, "oscairn: %s\n", notice_.c_str() );
        }
    }

    Control::AudioDevices AudioEngine::listDevices()
    {
        Control::AudioDevices d;
        const auto& config = channelMap_.configuration();
        juce::AudioDeviceManager::AudioDeviceSetup setup;
        devices_.getAudioDeviceSetup( setup );
        auto* type = devices_.getCurrentDeviceTypeObject();
        auto* current = devices_.getCurrentAudioDevice();
        d.output = setup.outputDeviceName.toStdString();
        if( !config.inputs.empty() ) d.input = setup.inputDeviceName.toStdString();
        if( current ) { d.sampleRate = current->getCurrentSampleRate(); d.bufferSize = uint32_t( current->getCurrentBufferSizeSamples() ); }
        d.source = source_; d.notice = notice_; d.fixed = fixed_;
        auto describe = [&]( const juce::String& name, bool input ) {
            Control::AudioDevice a{ name.toStdString(), {}, {} };
            // The open device answers for itself; another is made to ask it, never opened.
            std::unique_ptr<juce::AudioIODevice> probe;
            auto* device = current && name == ( input ? setup.inputDeviceName : setup.outputDeviceName ) ? current
                : ( probe.reset( type->createDevice( input ? juce::String() : name, input ? name : juce::String() ) ), probe.get() );
            if( !device ) return a;
            for( auto r : device->getAvailableSampleRates() ) if( !config.sampleRate || std::abs( r - config.sampleRate ) < .5 ) a.sampleRates.push_back( r ); // a profile's rate is the only one
            for( auto b : device->getAvailableBufferSizes() ) if( b > 0 ) a.bufferSizes.push_back( uint32_t( b ) );
            return a;
        };
        if( type )
        {
            for( const auto& name : type->getDeviceNames( false ) ) if( !fixed_ || name == setup.outputDeviceName ) d.outputs.push_back( describe( name, false ) );
            if( !config.inputs.empty() )
                for( const auto& name : type->getDeviceNames( true ) ) if( !fixed_ || name == setup.inputDeviceName ) d.inputs.push_back( describe( name, true ) );
        }
        return d;
    }

    Control::AudioDevices AudioEngine::setDevice( const Control::AudioSetting& s )
    {
        const auto now = listDevices();
        if( fixed_ && ( ( s.output && *s.output != now.output ) || ( s.input && *s.input != now.input ) ) )
            throw std::invalid_argument( ( channelMap_.configuration().nullDevice ? "--audio null fixes the audio device: " : "The device profile fixes the audio device: " ) + now.output );
        if( s.input && channelMap_.configuration().inputs.empty() ) throw std::invalid_argument( "This engine opens no audio input" );
        auto find = []( const std::vector<Control::AudioDevice>& list, const std::string& name, const char* what ) {
            const auto at = std::find_if( list.begin(), list.end(), [&]( const auto& a ) { return a.name == name; } );
            if( at == list.end() ) throw std::invalid_argument( std::string( "No audio " ) + what + " is named " + name );
            return *at;
        };
        const auto output = find( now.outputs, s.output.value_or( now.output ), "output" );
        if( s.input ) find( now.inputs, *s.input, "input" );
        auto offered = []( const auto& values ) { std::string out; for( auto v : values ) out += ( out.empty() ? "" : ", " ) + std::to_string( int64_t( v ) ); return out; };
        if( s.sampleRate && std::none_of( output.sampleRates.begin(), output.sampleRates.end(), [&]( double r ) { return std::abs( r - *s.sampleRate ) < .5; } ) )
            throw std::invalid_argument( std::to_string( int64_t( *s.sampleRate ) ) + " Hz is not a sample rate " + output.name + " offers (" + offered( output.sampleRates ) + ")" );
        if( s.bufferSize && std::find( output.bufferSizes.begin(), output.bufferSizes.end(), *s.bufferSize ) == output.bufferSizes.end() )
            throw std::invalid_argument( std::to_string( *s.bufferSize ) + " is not a buffer size " + output.name + " offers (" + offered( output.bufferSizes ) + ")" );

        juce::AudioDeviceManager::AudioDeviceSetup previous;
        devices_.getAudioDeviceSetup( previous );
        auto next = previous;
        if( s.output ) next.outputDeviceName = *s.output;
        if( s.input ) next.inputDeviceName = *s.input;
        if( s.sampleRate ) next.sampleRate = *s.sampleRate;
        if( s.bufferSize ) next.bufferSize = int( *s.bufferSize );
        // The device restarts (silent between), its start recording it; at another rate it stays
        // silent until every plugin is activated again at that rate, here.
        auto open = [&]( const juce::AudioDeviceManager::AudioDeviceSetup& setup ) {
            { const juce::ScopedLock lock( devices_.getAudioCallbackLock() ); adopt_ = true; }
            auto error = devices_.setAudioDeviceSetup( setup, true ).toStdString();
            if( error.empty() && !devices_.getCurrentAudioDevice() ) error = "no device opened";
            if( error.empty() && !started_ && pendingRate_ <= 0 ) error = "it lacks the channels this engine plays";
            { const juce::ScopedLock lock( devices_.getAudioCallbackLock() ); adopt_ = false; }
            if( error.empty() ) { applyRate(); if( auto* d = devices_.getCurrentAudioDevice() ) opened_ = d->getName().toStdString(); }
            return error;
        };
        notice_.clear();
        if( const auto error = open( next ); !error.empty() )
        {
            const auto back = open( previous );
            if( !back.empty() ) opened_.clear();
            throw std::runtime_error( next.outputDeviceName.toStdString() + " did not open: " + error +
                ( back.empty() ? "; " + previous.outputDeviceName.toStdString() + " plays again, as it was" : "; nor did " + previous.outputDeviceName.toStdString() + " again: " + back ) );
        }
        if( s.output || s.input ) source_ = "user";
        // Saved as the user's, over what they chose before; never the null device's, which is no choice of theirs.
        if( channelMap_.configuration().nullDevice ) return listDevices();
        auto choice = readChoice();
        if( s.output ) choice.output = s.output;
        if( s.input ) choice.input = s.input;
        if( s.sampleRate ) choice.sampleRate = s.sampleRate;
        if( s.bufferSize ) choice.bufferSize = s.bufferSize;
        try { writeChoice( choice ); }
        catch( const std::exception& e ) { notice_ = std::string( "the choice is not saved: " ) + e.what(); }
        return listDevices();
    }

    void AudioEngine::playPads()
    {
        std::vector<std::pair<uint32_t, bool>> pads;
        { std::lock_guard lock( padLock_ ); pads.swap( pads_ ); }
        // A pad has no velocity on this panel: full, unless the pad's assignment sets one.
        for( const auto& [pad, on] : pads )
            if( pad < PadMap::pads ) session_->pad( pad, on, 1.0 );
    }

    void AudioEngine::service()
    {
        applyRate();
        session_->service();
        if( midiDevicesDirty_.exchange( false ) )
            { openMidiInputs(); openMidiOutputs(); }
        Control::SessionStatus status;
        if(auto* device = devices_.getCurrentAudioDevice())
        {
            status.audioDevice = device->getName().toStdString();
            status.sampleRate = device->getCurrentSampleRate();
            status.bufferSize = static_cast<uint32_t>(device->getCurrentBufferSizeSamples());
            auto describe=[](const auto& selected,const juce::StringArray& names,const juce::BigInteger& active){
                std::vector<Control::DeviceChannel> result;for(auto i:selected)result.push_back({i,i<static_cast<uint32_t>(names.size())?names[static_cast<int>(i)].toStdString():"Unavailable",active[static_cast<int>(i)]});return result;};
            status.audioInputs=describe(channelMap_.configuration().inputs,device->getInputChannelNames(),device->getActiveInputChannels());
            status.audioOutputs=describe(channelMap_.configuration().outputs,device->getOutputChannelNames(),device->getActiveOutputChannels());
            // ALSA's underruns and overruns, CoreAudio's overload notifications; -1 where JUCE has none.
            const auto xruns=device->getXRunCount();
            status.audioUnderruns=xruns>=0?uint64_t(xruns):lateCallbacks_.load(std::memory_order_relaxed);
        }
        status.audioDeviceList=deviceList_;
        status.audioReady=started_.load();
        status.cpuLoad = devices_.getCpuUsage();
        status.midiEventDrops=droppedMidi_.load()+droppedOut_.load();
        // shortcut: every desktop input is reported as USB; port and network kinds need the device's own input table (RFC 0012).
        for(size_t i = 0; i < midiNames_.size(); ++i) status.midiInputs.push_back({MidiEndpoint::Kind::Usb, midiSources_[i], midiNames_[i]});
        for(size_t i = 0; i < midiOutputs_.size(); ++i) status.midiOutputs.push_back({MidiEndpoint::Kind::Usb, outputNumbers_[i], midiOutputs_[i]->getName().toStdString()});
        const auto changes = controllers_->changes();
        status.midiPortList = changes;
        session_->setHostStatus(std::move(status));
        // The inputs or a choice changed: the controllers they make played (after the status, which names the inputs).
        if( changes != controllerChanges_ ) { controllerChanges_ = changes; auto a = controllers_->applied(); session_->setHostControllers( std::move( a.groups ), std::move( a.ports ), std::move( a.ignored ) ); }
        if( serviced_ ) serviced_();
    }

    void AudioEngine::openMidiOutputs()
    {
        std::vector<std::unique_ptr<juce::MidiOutput>> outputs;
        std::vector<uint8_t> numbers;
        for( const auto& info : channelMap_.configuration().nullDevice ? juce::Array<juce::MidiDeviceInfo>() : juce::MidiOutput::getAvailableDevices() ) // the null device: none, so a test sends nothing out
        {
            auto known = outputNumberOf_.find( info.identifier );
            if( known == outputNumberOf_.end() )
            {
                std::set<uint8_t> used; // the lowest free number, 1 to 63; a device keeps its number while the app runs
                for( const auto& [id, number] : outputNumberOf_ ) used.insert( number );
                uint8_t number = 1;
                while( number < maxSources && used.count( number ) ) ++number;
                if( number == maxSources ) continue; // shortcut: past 63 outputs ever seen, new ones are not opened until the app restarts
                known = outputNumberOf_.emplace( info.identifier, number ).first;
            }
            if( auto output = juce::MidiOutput::openDevice( info.identifier ) )
            {
                output->startBackgroundThread(); // sends each block's messages at their times
                outputs.push_back( std::move( output ) );
                numbers.push_back( known->second );
            }
        }
        std::lock_guard lock( outputLock_ );
        midiOutputs_.swap( outputs );
        outputNumbers_ = std::move( numbers );
    }

    void AudioEngine::queueMidiOut( uint32_t offset, double seconds ) noexcept
    {
        const auto messages = session_->midiOutput();
        if( messages.empty() ) return;
        const auto scope = outFifo_.write( (int)messages.size() );
        const auto written = size_t( scope.blockSize1 + scope.blockSize2 );
        for( size_t i = 0; i < written; ++i )
        {
            const auto& m = messages[i];
            auto& o = outgoing_[(size_t)( (int)i < scope.blockSize1 ? scope.startIndex1 + (int)i : scope.startIndex2 + (int)i - scope.blockSize1 )];
            o = { seconds * 1000 + ( offset + m.time ) * 1000 / sampleRate_ + outputLatencyMs_, m.ports, m.size, { m.bytes[0], m.bytes[1], m.bytes[2] } };
        }
        droppedOut_ += messages.size() - written;
    }

    void AudioEngine::Sender::run()
    {
        while( !threadShouldExit() )
        {
            {
                const auto scope = engine.outFifo_.read( engine.outFifo_.getNumReady() );
                std::lock_guard lock( engine.outputLock_ );
                auto send = [&]( int index ) {
                    const auto& o = engine.outgoing_[(size_t)index];
                    juce::MidiBuffer block;
                    block.addEvent( o.bytes, o.size, 0 );
                    for( size_t i = 0; i < engine.midiOutputs_.size(); ++i )
                        if( engine.outputNumbers_[i] < 64 && ( o.ports >> engine.outputNumbers_[i] & 1 ) )
                            engine.midiOutputs_[i]->sendBlockOfMessages( block, o.ms, engine.sampleRate_ );
                };
                for( int i = 0; i < scope.blockSize1; ++i ) send( scope.startIndex1 + i );
                for( int i = 0; i < scope.blockSize2; ++i ) send( scope.startIndex2 + i );
            }
            wait( 1 ); // well inside the audio output's latency, which every message is due after
        }
    }

    uint8_t AudioEngine::sourceNumber( const juce::String& identifier, const std::vector<uint8_t>& taken )
    {
        if( const auto known = sourceOf_.find( identifier ); known != sourceOf_.end() ) return known->second;
        // The lowest free number; past 63 devices, one of an absent device.
        std::set<uint8_t> used;
        for( const auto& [id, source] : sourceOf_ ) used.insert( source );
        uint8_t source = 1;
        while( source < maxSources && used.count( source ) ) ++source;
        if( source == maxSources )
            for( auto it = sourceOf_.begin(); it != sourceOf_.end(); ++it )
                if( std::none_of( taken.begin(), taken.end(), [&]( uint8_t s ) { return s == it->second; } ) &&
                    std::find( keyboardSources_.begin(), keyboardSources_.end(), it->second ) == keyboardSources_.end() ) // the keyboard's are always open
                    { source = it->second; sourceOf_.erase( it ); break; }
        if( source == maxSources ) return source; // 63 inputs open at once
        sourceOf_.emplace( identifier, source );
        return source;
    }

    void AudioEngine::openMidiInputs()
    {
#ifdef __linux__
        if( rawMidi_ ) { openRawMidiInputs(); return; }
#endif
        {
            // Stopped outside the lock: a callback waiting for it must not block stop().
            std::vector<std::unique_ptr<juce::MidiInput>> old;
            {
                std::lock_guard lock( pushLock_ );
                old.swap( midiInputs_ );
                midiSources_.clear();
            }
            for( auto& input : old )
                input->stop();
        }
        std::vector<std::unique_ptr<juce::MidiInput>> inputs;
        std::vector<uint8_t> sources;
        for( const auto& info : channelMap_.configuration().nullDevice ? juce::Array<juce::MidiDeviceInfo>() : juce::MidiInput::getAvailableDevices() ) // the null device: none, so no note reaches a test
        {
            const auto source = sourceNumber( info.identifier, sources );
            if( source == maxSources ) continue;
            if( auto input = juce::MidiInput::openDevice( info.identifier, this ) )
            {
                inputs.push_back( std::move( input ) );
                sources.push_back( source );
            }
        }
        {
            std::lock_guard lock( pushLock_ );
            midiInputs_ = std::move( inputs );
            midiSources_ = std::move( sources );
            midiSources_.insert( midiSources_.end(), keyboardSources_.begin(), keyboardSources_.end() );
        }
        std::vector<std::string> names;
        for( auto& input : midiInputs_ )
        {
            input->start();
            names.push_back( input->getName().toStdString() );
        }
        names.insert( names.end(), keyboardNames_.begin(), keyboardNames_.end() );
        std::fprintf( stderr, "oscairn: %zu MIDI inputs open\n", midiInputs_.size() );
        midiNames_ = names;
        controllers_->setInputs( std::move( names ) );
    }

#ifdef __linux__
    void AudioEngine::openRawMidiInputs()
    {
        rawInputs_.reset(); // stops its thread and closes the devices; outside the lock, which its thread takes
        {
            std::lock_guard lock( pushLock_ );
            midiSources_.clear();
        }
        std::vector<RawMidiSource> opened;
        std::vector<uint8_t> sources;
        std::vector<std::string> names, seen;
        for( const auto& port : rawMidiPorts() )
        {
            seen.push_back( port.identifier );
            const auto source = sourceNumber( juce::String( port.identifier ), sources );
            if( source == maxSources ) continue;
            auto input = openRawMidi( port.identifier ); // not while another program has it
            if( input.fd < 0 ) continue;
            opened.push_back( std::move( input ) );
            sources.push_back( source );
            names.push_back( port.name );
        }
        {
            std::lock_guard lock( pushLock_ );
            midiSources_ = std::move( sources );
            midiSources_.insert( midiSources_.end(), keyboardSources_.begin(), keyboardSources_.end() );
        }
        names.insert( names.end(), keyboardNames_.begin(), keyboardNames_.end() );
        const auto count = opened.size();
        rawInputs_ = std::make_unique<RawMidiReader>( std::move( opened ),
            [this]( size_t input, const uint8_t* bytes, size_t size ) { incoming( juce::MidiMessage( bytes, int( size ) ), nullptr, int( input ) ); },
            std::move( seen ), [] { std::vector<std::string> ids; for( const auto& p : rawMidiPorts() ) ids.push_back( p.identifier ); return ids; },
            [this] { midiDevicesDirty_ = true; } );
        std::fprintf( stderr, "oscairn: %zu MIDI inputs open (ALSA raw MIDI)\n", count );
        midiNames_ = names;
        controllers_->setInputs( std::move( names ) );
    }
#endif

    void AudioEngine::incoming( const juce::MidiMessage& message, const juce::MidiInput* input, int raw, uint8_t known )
    {
        kick::clap::Event converted; // channel messages pass if the block will take them (midiToClap)
        if( oscairn::midiToClap( message.getRawData(), (size_t)message.getRawDataSize(), 0, converted ) || message.isMidiClock() || message.isMidiStart() || message.isMidiContinue() || message.isMidiStop()
            || message.isSongPositionPointer() )
        {
            const auto source = push( message, input, raw, known );
            // A controller's control (RFC 0011's instant controls), mapped here on the MIDI thread.
            if( message.isController() || message.isNoteOnOrOff() || message.isPitchWheel() || message.isChannelPressure() )
                session_->controllerMidi( source, message.getRawData(), (size_t)message.getRawDataSize() );
        }
    }

    uint8_t AudioEngine::push( const juce::MidiMessage& message, const juce::MidiInput* input, int raw, uint8_t known )
    {
        Incoming in{};
        const auto size = std::min( message.getRawDataSize(), 3 );
        std::copy_n( message.getRawData(), size, in.bytes );
        in.size = (uint8_t)size;
        in.source = known;
        in.seconds = juce::Time::getMillisecondCounterHiRes() * 0.001;

        std::lock_guard lock( pushLock_ );
        for( size_t i = 0; i < midiInputs_.size(); ++i )
            if( midiInputs_[i].get() == input ) in.source = midiSources_[i];
        if( raw >= 0 && size_t( raw ) < midiSources_.size() ) in.source = midiSources_[size_t( raw )]; // only raw inputs are open then
        const auto      scope = fifo_.write( 1 );
        if( scope.blockSize1 + scope.blockSize2 == 0 )
            {++droppedMidi_;midiOverflow_.store(true,std::memory_order_release);}
        else
            incoming_[(size_t)( scope.blockSize1 > 0 ? scope.startIndex1 : scope.startIndex2 )] = in;
        return in.source;
    }

    void AudioEngine::audioDeviceAboutToStart( juce::AudioIODevice* device )
    {
        const auto& config=channelMap_.configuration();
        const auto inputMask=device->getActiveInputChannels(),outputMask=device->getActiveOutputChannels();
        const double rate=device->getCurrentSampleRate();
        // A device chosen, or one opened when the last went away and the profile fixes none, is the one played from now on.
        if(adopt_ || !fixed_){initialDevice_=device->getName().toStdString();initialInputs_=device->getInputChannelNames();initialOutputs_=device->getOutputChannelNames();}
        bool valid=rate>0 && device->getName().toStdString()==initialDevice_ &&
            device->getInputChannelNames()==initialInputs_ && device->getOutputChannelNames()==initialOutputs_;
        for(uint32_t i=0;i<ChannelMap::required(config.inputs);++i)valid=valid && inputMask[static_cast<int>(i)];
        const auto required=(!config.explicitOutputs && initialOutputs_.size()==1)?1:std::max(ChannelMap::required(config.outputs),config.deviceChannels);
        for(uint32_t i=0;i<required;++i)valid=valid && outputMask[static_cast<int>(i)];
        outputLatencyMs_=(device->getOutputLatencyInSamples()+device->getCurrentBufferSizeSamples())*1000.0/device->getCurrentSampleRate();
        lastCallback_=0;lateCallbacks_=0;realtime_=false;
        if(session_)session_->audioStarted(); // the constructor's first start: the session starts warming up when made
        // At another rate, silent until service() has the plugins activated again at it, on the control thread.
        const bool moved=std::abs(rate-sampleRate_)>=.5;
        pendingRate_=valid && moved?rate:0;
        started_=valid && !moved;
    }

    void AudioEngine::audioDeviceIOCallbackWithContext( const float* const* inputs, int numInputs, float* const* outputs, int numOutputs, int numSamples,
                                                   const juce::AudioIODeviceCallbackContext& )
    {
        if(numSamples<=0)return;
#ifdef __linux__
        // JUCE's ALSA thread is not real-time; once per device start it takes the engine's place in
        // RFC 0035's priority table (CoreAudio's thread on macOS already is).
        if(!realtime_){realtime_=true;oscairn::worker::makeRealtime(1e6*numSamples/sampleRate_,oscairn::worker::priority::engineAudio);}
#endif
        const double now=juce::Time::getMillisecondCounterHiRes()*0.001;
        if(lastCallback_>0 && now-lastCallback_>2.0*numSamples/sampleRate_)lateCallbacks_.fetch_add(1,std::memory_order_relaxed);
        lastCallback_=now;
        if(!started_ || numOutputs<1)
        {for(int c=0;c<numOutputs;++c)if(outputs[c])std::fill_n(outputs[c],numSamples,0.f);return;}
        // Device timestamps belong to this adapter. The core receives sample times,
        // with the existing policy: notes now, clock timestamps in the preceding block.
        uint32_t clockCount = 0;notes_.reset();
        const bool inputOverflow=midiOverflow_.exchange(false,std::memory_order_acquire);
        {
            const auto scope = fifo_.read( fifo_.getNumReady() );
            auto       take  = [&]( int index ) {
                const auto&   in     = incoming_[(size_t)index];
                const uint8_t status = in.bytes[0];
                if( status >= 0xF0 )
                {
                    if( blockSample_ == lastBlockStart_ )
                        return;
                    const auto offset = (int64_t)std::llround( ( in.seconds - blockSeconds_ ) * sampleRate_ );
                    const auto time   = std::clamp<int64_t>( lastBlockStart_ + offset, lastBlockStart_, blockSample_ - 1 );
                    if( clockCount < clocks_.size() )
                        clocks_[clockCount++] = { status, time, (uint16_t)( in.bytes[1] | ( in.bytes[2] << 7 ) ) };
                    return;
                }
                // Notes, aftertouch (as CLAP pressure), pitch bend and the performance controllers.
                notes_.append(in.bytes,in.size,in.source);
            };
            for( int i = 0; i < scope.blockSize1; ++i )
                take( scope.startIndex1 + i );
            for( int i = 0; i < scope.blockSize2; ++i )
                take( scope.startIndex2 + i );
        }
        droppedMidi_+=notes_.dropped;
        const bool panic=inputOverflow || notes_.dropped;
        if(panic){droppedMidi_+=notes_.count;notes_.count=0;}
        channelMap_.process(inputs,static_cast<uint32_t>(numInputs),outputs,static_cast<uint32_t>(numOutputs),static_cast<uint32_t>(numSamples),
            [&](const float* const* in,uint32_t ni,float* const* out,uint32_t no,uint32_t frames,uint32_t offset){
                if(offset==0 && panic)session_->panicInput();
                const bool rendered=session_->process(in,ni,out,no,frames,{notes_.events.data(),offset==0?notes_.count:0},{clocks_.data(),offset==0?clockCount:0},
                    {notes_.sources.data(),offset==0?notes_.count:0});
                queueMidiOut(offset,now);
                return rendered;
            });
        blockSeconds_   = now;
        lastBlockStart_ = blockSample_;
        blockSample_ += numSamples;
        // engine.timing: the block's start (the clock read above) and end, on the same clock (CLOCK_MONOTONIC on Linux, to the microsecond; mach_absolute_time on macOS).
        session_->blockTimes(int64_t(now*1e9),int64_t(juce::Time::getMillisecondCounterHiRes()*1e6),int64_t(1e9*numSamples/sampleRate_));
    }
}
