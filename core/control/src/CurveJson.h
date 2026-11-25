// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// A control target's curve as the Performance, the Sound and the control protocol write it: a
// name, or for db, power and table an object with its numbers (RFC 0011's control tree).
#include "JsonInput.h"
#include "oscairn/parts.h"
#include <locale>
#include <sstream>

namespace oscairn::detail {
inline std::string curveJson( const MacroTarget& t, std::string ( *number )( double ) )
{
    using C = MacroTarget::Curve;
    if( t.curve == C::Db ) return "{\"kind\":\"db\",\"min\":" + number( t.low ) + ",\"max\":" + number( t.high ) + '}';
    if( t.curve == C::Power ) return "{\"kind\":\"power\",\"exponent\":" + number( t.exponent ) + '}';
    if( t.curve == C::Table )
    {
        std::string points;
        for( const double p : t.points ) points += ( points.empty() ? "" : "," ) + number( p );
        return "{\"kind\":\"table\",\"points\":[" + points + "]}";
    }
    return std::string( "\"" ) + curveKinds[size_t( t.curve )] + '"';
}

// Sets t's curve from v; false when v is not one, its numbers included (MacroTarget::validCurve).
inline bool curveFrom( const json_value_s* v, MacroTarget& t )
{
    auto name = []( const json_value_s* s ) { return s && s->type == json_type_string ? std::string( static_cast<const json_string_s*>( s->payload )->string, static_cast<const json_string_s*>( s->payload )->string_size ) : std::string(); };
    auto number = []( const json_value_s* n, double& out ) {
        if( !n || n->type != json_type_number ) return false;
        const auto* x = static_cast<const json_number_s*>( n->payload );
        std::istringstream in( std::string( x->number, x->number_size ) ); in.imbue( std::locale::classic() );
        return bool( in >> out ) && in.eof();
    };
    if( v && v->type == json_type_string )
    {
        const auto n = name( v );
        for( size_t i = 0; i < 4; ++i ) if( n == curveKinds[i] ) { t.curve = MacroTarget::Curve( i ); return true; }
        return false;
    }
    if( !v || v->type != json_type_object ) return false;
    size_t count = 0;
    for( auto* e = static_cast<const json_object_s*>( v->payload )->start; e; e = e->next ) ++count;
    const auto kind = name( member( v, "kind" ) );
    if( kind == "db" ) { t.curve = MacroTarget::Curve::Db; if( count != 3 || !number( member( v, "min" ), t.low ) || !number( member( v, "max" ), t.high ) ) return false; }
    else if( kind == "power" ) { t.curve = MacroTarget::Curve::Power; if( count != 2 || !number( member( v, "exponent" ), t.exponent ) ) return false; }
    else if( kind == "table" )
    {
        t.curve = MacroTarget::Curve::Table;
        const auto* points = member( v, "points" );
        if( count != 2 || !points || points->type != json_type_array ) return false;
        t.points.clear();
        for( auto* e = static_cast<const json_array_s*>( points->payload )->start; e; e = e->next )
        {
            if( t.points.size() == 128 ) return false;
            if( !number( e->value, t.points.emplace_back() ) ) return false;
        }
    }
    else return false;
    return t.validCurve();
}
}
