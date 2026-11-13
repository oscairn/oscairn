// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Keys of a Linux input device, by function (InputKeys.h).
#include "InputKeys.h"

#include <cerrno>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <dirent.h>
#include <fcntl.h>
#include <linux/input.h>
#include <poll.h>
#include <sys/ioctl.h>
#include <unistd.h>

namespace oscairn::host
{
    void InputKeys::standardError( const std::string& line ) { std::fprintf( stderr, "oscairn: input keys: %s\n", line.c_str() ); }

    InputKeys::InputKeys( std::string name, std::map<uint16_t, std::string> keys, Play play, Log log, std::string folder )
        : name_( std::move( name ) ), folder_( std::move( folder ) ), keys_( std::move( keys ) ), play_( std::move( play ) ), log_( std::move( log ) ), owned_( true )
    {
        thread_ = std::thread( [this] { run(); } );
    }

    InputKeys::InputKeys( int fd, std::map<uint16_t, std::string> keys, Play play, Log log )
        : keys_( std::move( keys ) ), play_( std::move( play ) ), log_( std::move( log ) ), fd_( fd )
    {
        thread_ = std::thread( [this] { run(); } );
    }

    InputKeys::~InputKeys()
    {
        { std::lock_guard lock( lock_ ); stop_ = true; }
        stopped_.notify_all();
        thread_.join();
        if( owned_ && fd_ >= 0 ) ::close( fd_ );
    }

    bool InputKeys::pause()
    {
        std::unique_lock lock( lock_ );
        return !stopped_.wait_for( lock, std::chrono::seconds( 1 ), [this] { return stop_.load(); } );
    }

    int InputKeys::find()
    {
        DIR* dir = ::opendir( folder_.c_str() );
        if( !dir ) return -1;
        int found = -1;
        while( const auto* e = found < 0 ? ::readdir( dir ) : nullptr )
        {
            if( std::strncmp( e->d_name, "event", 5 ) != 0 ) continue;
            const auto path = folder_ + "/" + e->d_name;
            const int fd = ::open( path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC ); // read only, never grabbed (no EVIOCGRAB)
            if( fd < 0 ) continue;
            char got[256] = {};
            if( ::ioctl( fd, EVIOCGNAME( sizeof got - 1 ), got ) >= 0 && name_ == got ) { found = fd; if( log_ ) log_( "reading " + path + " (" + name_ + ")" ); }
            else ::close( fd );
        }
        ::closedir( dir );
        return found;
    }

    void InputKeys::run()
    {
        bool reported = false; // the device not found, once until it is
        input_event events[16];
        size_t have = 0; // bytes of a record cut by a read (a pipe's; the kernel gives whole records)
        while( !stop_ )
        {
            if( fd_ < 0 )
            {
                if( ( fd_ = find() ) < 0 )
                {
                    if( !reported && log_ ) log_( "no input device named " + name_ + " in " + folder_ + "; trying again each second" );
                    reported = true;
                    if( !pause() ) return;
                    continue;
                }
                reported = false;
                have = 0;
            }
            pollfd p{ fd_, POLLIN, 0 };
            const int ready = ::poll( &p, 1, 100 ); // the timeout bounds how long stopping waits
            if( ready == 0 || ( ready < 0 && errno == EINTR ) ) continue;
            const auto n = ready > 0 ? ::read( fd_, reinterpret_cast<char*>( events ) + have, sizeof events - have ) : -1;
            if( n > 0 )
            {
                have += size_t( n );
                const size_t whole = have / sizeof( input_event );
                for( size_t i = 0; i < whole; ++i )
                {
                    const auto& e = events[i];
                    if( e.type != EV_KEY || e.value > 1 ) continue; // 2 is the key's autorepeat
                    if( const auto k = keys_.find( e.code ); k != keys_.end() && play_ )
                        play_( k->second, { ControlInput::Kind::Switch, double( e.value ) } );
                }
                have -= whole * sizeof( input_event );
                if( have ) std::memmove( events, reinterpret_cast<char*>( events ) + whole * sizeof( input_event ), have );
                continue;
            }
            if( n < 0 && ( errno == EAGAIN || errno == EINTR ) ) continue;
            if( !owned_ ) return; // a descriptor given: its end is the end
            const int error = errno;
            ::close( fd_ ); fd_ = -1;
            if( log_ ) log_( name_ + ( n == 0 ? " ended" : std::string( ": " ) + std::strerror( error ) ) + "; looking for it again" );
            if( !pause() ) return;
        }
    }
}
