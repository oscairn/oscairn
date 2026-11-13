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
    }

    std::unique_ptr<DeviceReader> openPanel( const std::string& device, const std::string& map, PlayControl play, PlayPad pad )
    {
        // A device panel's own controls, played as a controller's are, by their identities; its
        // pads ("pad:N" in the map) through the Performance's pad map. The device is the panel
        // holder's (docs/fantom-0.md, "The panel holder"): the engine attaches to it.
        std::ifstream file( map, std::ios::binary );
        if( !file ) throw std::runtime_error( "cannot read the panel map " + map );
        std::stringstream text; text << file.rdbuf();
        auto parsed = PanelMap::parse( text.str() );
        std::fprintf( stderr, "oscairn: panel: %s through the panel holder at %s\n", device.c_str(), panelHolderSocket );
        return std::make_unique<Held<PanelReader>>( panelHolderSocket, std::move( parsed ), [play = std::move( play ), pad = std::move( pad )]( const PanelEvent& e ) {
            if( e.control.empty() || e.chatter ) return;
            if( e.control.starts_with( "pad:" ) )
            {
                const auto n = std::atoi( e.control.c_str() + 4 );
                if( n >= 1 && e.input.kind == ControlInput::Kind::Switch ) pad( uint32_t( n - 1 ), e.input.value >= .5 );
                return;
            }
            play( e.control, e.input );
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
        return std::make_unique<Held<BmcLinkReader>>( device, std::move( play ), BmcLinkReader::standardError, std::chrono::milliseconds( 500 ), localOff );
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
