// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The FANTOM-0's panel and keys for the engine (hosts/audio's DeviceReaders.h): the panel reader
// (panel_reader.h), attached to the panel holder, the message link's reader (BmcLink.h), and
// the keys of an input device that are not on the panel (InputKeys.h).
#include "DeviceReaders.h"
#include "BmcLink.h"
#ifdef __linux__
#include "InputKeys.h"
#endif
#include "oscairn/panel_reader.h"

#include <cstdio>
#include <cstdlib>
#include <fstream>
#include <sstream>
#include <stdexcept>

namespace oscairn::host
{
    namespace
    {
        template<class Reader> struct Held : DeviceReader
        {
            template<class... A> explicit Held( A&&... a ) : reader( std::forward<A>( a )... ) {}
            Reader reader;
        };

        PanelMap readMap( const std::string& map )
        {
            std::ifstream file( map, std::ios::binary );
            if( !file ) throw std::runtime_error( "cannot read the panel map " + map );
            std::stringstream text; text << file.rdbuf();
            return PanelMap::parse( text.str() );
        }
    }

    std::unique_ptr<DeviceReader> openPanel( const std::string& device, const std::string& map, PlayControl play, PlayPad pad, PlayNavigated navigate )
    {
        // A device panel's own controls, played as a controller's are, by their identities; its
        // pads ("pad:N" in the map) through the Performance's pad map. The device is the panel
        // holder's (docs/fantom-0.md, "The panel holder"): the engine attaches to it.
        auto parsed = readMap( map );
        std::fprintf( stderr, "oscairn: panel: %s through the panel holder at %s\n", device.c_str(), panelHolderSocket );
        return std::make_unique<Held<PanelReader>>( panelHolderSocket, std::move( parsed ), [play = std::move( play ), pad = std::move( pad ), navigate = std::move( navigate )]( const PanelEvent& e ) {
            if( e.control.empty() || e.chatter ) return;
            if( e.control.starts_with( "pad:" ) )
            {
                const auto n = std::atoi( e.control.c_str() + 4 );
                if( n >= 1 && e.input.kind == ControlInput::Kind::Switch ) pad( uint32_t( n - 1 ), e.input.value >= .5 );
                return;
            }
            if( e.navigation ) navigate( e.control, e.input, *e.navigation ); else play( e.control, e.input );
        } );
    }

    std::vector<KeyboardInput> keyboardInputs()
    {
        std::vector<KeyboardInput> inputs;
        for( size_t i = 0; i < bmcSources.size(); ++i )
        {
            char key[16];
            std::snprintf( key, sizeof key, "bmc:%x", unsigned( bmcSources[i] ) );
            inputs.push_back( { bmcInputName( i ), key } );
        }
        return inputs;
    }

    std::unique_ptr<DeviceReader> openKeyboard( const std::string& device, PlayMidi play, bool localOff )
    {
        // A cold start: oscairn-fantom-start --cold sets it to selector 2's value (BmcLink.h's bmcLinuxUp).
        std::optional<uint32_t> cold;
        if( const char* v = std::getenv( "OSCAIRN_BMC_COLD" ); v && *v ) cold = uint32_t( std::strtoul( v, nullptr, 10 ) );
        return std::make_unique<Held<BmcLinkReader>>( device, std::move( play ), BmcLinkReader::standardError, std::chrono::milliseconds( 500 ), localOff, cold );
    }

    SetLamps panelLamps( const std::string& map, DeviceReader& keyboard )
    {
        auto* link = dynamic_cast<Held<BmcLinkReader>*>( &keyboard );
        if( !link ) return {};
        std::map<std::string, uint16_t> named;
        for( const auto& c : readMap( map ).controls ) if( c.lamp ) named[c.control] = *c.lamp;
        auto lamps = std::make_shared<BmcLamps>( [link]( const uint8_t* frame, size_t size ) { return link->reader.send( frame, size ); } );
        return [named = std::move( named ), lamps]( const std::vector<Control::Lamp>& set ) {
            Control::Lamps r{ true, 0, {} };
            std::vector<std::pair<uint16_t, bool>> numbers;
            for( const auto& l : set )
            {
                const auto at = l.number ? named.end() : named.find( l.control );
                if( l.number ) numbers.emplace_back( *l.number, l.on );
                else if( at != named.end() ) numbers.emplace_back( at->second, l.on );
                else r.unknown.push_back( l.control );
            }
            const int sent = lamps->set( numbers );
            if( sent < 0 ) throw std::runtime_error( "Too many lamp changes at once; none set" );
            r.sent = uint32_t( sent );
            return r;
        };
    }

    std::unique_ptr<DeviceReader> openInputKeys( const std::string& name, const std::map<uint16_t, std::string>& keys, PlayControl play )
    {
#ifdef __linux__
        std::fprintf( stderr, "oscairn: input keys: %zu keys of %s\n", keys.size(), name.c_str() );
        return std::make_unique<Held<InputKeys>>( name, keys, std::move( play ) );
#else
        (void)name; (void)keys; (void)play;
        return nullptr; // evdev is Linux's
#endif
    }
}
