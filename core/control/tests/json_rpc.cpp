// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/json_rpc.h"
#include "oscairn/session.h"
#include "AllocationGuard.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <iostream>
#include <stdexcept>

namespace
{
    struct Fixture
    {
        oscairn::Session session{48000,512};
        oscairn::JsonRpc rpc{session};
        oscairn::NodeId synth;
        std::array<float,512> left{}, right{};
        explicit Fixture( const char* path )
        {
            synth = session.add("synth",path);
            session.connectNotes(oscairn::graphInput,synth);
            session.connectAudio(synth,oscairn::graphOutput);
            session.commit();
            session.clearHistory(); // as the engine starts: its startup graph is no edit
        }
        void render( uint32_t frames = 512 )
        {
            session.service();
            float* out[]{left.data(),right.data()};
            forbidAllocation = true;
            const bool ok = session.process(out,frames);
            forbidAllocation = false;
            if( !ok ) throw std::runtime_error("render failed");
        }
        void call( const std::string& method, const std::string& params )
        {
            const auto response = rpc.dispatch("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"" + method + "\",\"params\":" + params + '}');
            if( !response || response->find("\"error\"") != std::string::npos )
                throw std::runtime_error(response.value_or("No response"));
        }
    };
    void equivalence( const char* path )
    {
        Fixture wire(path), direct(path);
        double energy = 0;
        for( unsigned i = 0; i < 500; ++i )
        {
            if( i == 0 )
            {
                wire.call("transport.set", R"({"playing":true,"tempo":137})");
                direct.session.setPlaying(true); direct.session.setTempo(137);
            }
            if( i == 1 )
            {
                wire.call("notes.send", R"({"on":true,"key":57,"velocity":0.73,"channel":2})");
                direct.session.note(true,57,0.73,2);
            }
            if( i == 20 )
            {
                wire.call("params.set", R"({"values":[{"node":"synth","param":1,"value":0.82},{"node":1,"param":2,"value":0.4}]})");
                direct.session.setParameter(direct.synth,1,0.82);
                direct.session.setParameter(direct.synth,2,0.4);
            }
            if( i == 45 )
            {
                wire.call("notes.send", R"({"on":false,"key":57,"channel":2})");
                direct.session.note(false,57,0,2);
            }
            const uint32_t frames = i % 2 ? 37 : 512;
            wire.render(frames); direct.render(frames);
            if( !std::equal(wire.left.begin(),wire.left.begin()+frames,direct.left.begin()) ||
                !std::equal(wire.right.begin(),wire.right.begin()+frames,direct.right.begin()) )
                throw std::runtime_error("JSON and typed controls rendered different audio");
            for( uint32_t j = 0; j < frames; ++j ) energy += wire.left[j]*wire.left[j];
        }
        if( energy < 1 || std::abs(wire.left[36]) > 1e-6f ) throw std::runtime_error("Missing note or unreleased voice");
        std::cout << "ok: JSON and typed control render identical audio without render allocations\n";
    }
    // notes.send as a MIDI input (its input field): a zone limited to that input hears it and not
    // the built-in source, the status lists each source's held keys, and a release for an input
    // gone still lets go.
    void sources( const char* path )
    {
        using oscairn::MidiEndpoint;
        Fixture f(path);
        oscairn::Control::SessionStatus status;
        status.midiInputs = {{MidiEndpoint::Kind::Usb, 3, "KeyLab 61"}, {MidiEndpoint::Kind::Usb, 4, "Other"}};
        f.session.setHostStatus(status);
        uint32_t wire = 0;
        for( const auto& w : f.session.graphState().wires ) if( w.from == oscairn::graphInput && w.to == f.synth ) wire = w.id;
        oscairn::NoteZone zone; zone.sources = 1ull << 3; // the KeyLab's notes only
        f.session.setZones(wire, {zone}); f.session.commit();
        auto energy = [&]( unsigned blocks ) { double e = 0; for( unsigned i = 0; i < blocks; ++i ) { f.render(); for( const float x : f.left ) e += double(x) * x; } return e; };
        auto held = [&] { std::string out; for( const auto& h : f.session.sessionStatus().heldNotes ) { out += std::to_string(h.source) + ":"; for( const auto k : h.keys ) out += std::to_string(k) + ","; out += ";"; } return out; };
        auto refused = [&]( const std::string& params ) { const auto r = f.rpc.dispatch(R"({"jsonrpc":"2.0","id":1,"method":"notes.send","params":)" + params + '}'); return r && r->find("-32602") != std::string::npos; };
        energy(4);
        f.call("notes.send", R"({"on":true,"key":60,"velocity":0.8})");
        if( energy(40) > 1e-9 ) throw std::runtime_error("A built-in note played a zone limited to the KeyLab");
        if( held() != "0:60,;" ) throw std::runtime_error("The built-in source's held key not listed: " + held());
        f.call("notes.send", R"({"on":false,"key":60})");
        f.call("notes.send", R"({"on":true,"key":62,"velocity":0.8,"input":"KeyLab 61"})");
        if( energy(40) < 1e-3 ) throw std::runtime_error("A note sent as the KeyLab missed its zone");
        if( held() != "3:62,;" ) throw std::runtime_error("The KeyLab's held key not listed: " + held());
        if( const auto r = f.rpc.dispatch(R"({"jsonrpc":"2.0","id":1,"method":"session.status"})"); !r || r->find(R"("heldNotes":[{"source":3,"keys":[62]}])") == std::string::npos )
            throw std::runtime_error("session.status has no heldNotes: " + r.value_or(""));
        if( !refused(R"({"on":true,"key":64,"input":"Nope"})") || !refused(R"({"on":true,"key":64,"input":""})") || !refused(R"({"on":true,"key":64,"input":3})") )
            throw std::runtime_error("A note on for an input not there, or an empty or numeric name, was not refused");
        f.session.setHostStatus({}); // the KeyLab unplugged with its key held: the release still reaches the zone
        f.call("notes.send", R"({"on":false,"key":62,"input":"KeyLab 61"})");
        energy(400);
        if( !held().empty() || std::abs(f.left[511]) > 1e-6f ) throw std::runtime_error("A release for an input gone did not let go: " + held());
        std::cout << "ok: notes sent as a MIDI input play its zones, and each source's held keys are listed\n";
    }
}
int main( int argc, char** argv )
{
    if( argc < 2 ) return 2;
    try
    {
        if( argc == 2 ) { equivalence(argv[1]); sources(argv[1]); return 0; }
        // Test-only line driver. Blank output represents no JSON-RPC response. After --stdio, a
        // repository snapshot preinstalled from as the engine does at start (preinstall.py).
        Fixture f(argv[1]);
        if( argc > 3 ) f.session.preinstall(argv[3]);
        for( std::string line; std::getline(std::cin,line); )
        {
            if( line == "!render" || line == "!drain" )
            {
                const auto blocks = line == "!drain" ? (oscairn::Session::commandCapacity + kick::clap::Host::maxEvents) / kick::clap::Host::maxBlockEvents + 2 : 1;
                for( uint32_t i = 0; i < blocks; ++i ) { f.render(); f.session.service(); }
                std::cout << "{}\n";
            }
            else if( line.starts_with("!poll ") )
            {
                const auto events = f.rpc.notifications(std::stoull(line.substr(6)));
                std::cout << '[';
                for(size_t i=0;i<events.size();++i) { if(i)std::cout<<','; std::cout<<events[i]; }
                std::cout << "]\n";
            }
            else std::cout << f.rpc.dispatch(line).value_or("") << '\n';
            std::cout.flush();
        }
    }
    catch( const std::exception& e ) { std::cerr << e.what() << '\n'; return 1; }
}
