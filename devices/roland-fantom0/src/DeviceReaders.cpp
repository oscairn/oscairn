// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The FANTOM-0's panel and keys for the engine (hosts/audio's DeviceReaders.h): the panel reader
// (panel_reader.h) and the message link's reader (BmcLink.h).
#include "DeviceReaders.h"
#include "BmcLink.h"
#include "oscairn/panel_reader.h"

#include <cstdio>
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

    std::unique_ptr<DeviceReader> openPanel( const std::string& device, const std::string& map, PlayControl play )
    {
        // A device panel's own controls, played as a controller's are, by their identities.
        std::ifstream file( map, std::ios::binary );
        if( !file ) throw std::runtime_error( "cannot read the panel map " + map );
        std::stringstream text; text << file.rdbuf();
        return std::make_unique<Held<PanelReader>>( device, PanelMap::parse( text.str() ), [play = std::move( play )]( const PanelEvent& e ) {
            if( !e.control.empty() ) play( e.control, e.input );
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

    std::unique_ptr<DeviceReader> openKeyboard( const std::string& device, PlayMidi play )
    {
        return std::make_unique<Held<BmcLinkReader>>( device, std::move( play ) );
    }
}
