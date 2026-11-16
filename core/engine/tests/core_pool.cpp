// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045: the voices a part keeps when every plugin of a Performance renders in one worker (compute.workers pool, one
// host: the lane every plugin shares) against a worker each, driven through the Rust engine core's C interface with the
// plugins modeled so a block's time is a function of its voices (no clock). Sick tines' shape: one shared instrument
// (no oscairn.voice-budget: the host fades its notes) of three sounds, 20 effects after it and a master, the effects
// cost `effect` us a block each, the instrument 80 us and 40 us a voice. The sustain pedal down on each sound's channel,
// six keys struck, then each sound strikes a key again every 375 blocks (1 s), so notes pile up under the pedal.
//   oscairn-core-pool-test
#include "../src/engine_core.h"
#include "oscairn/graph.h"
#include <clap/clap.h>
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <new>
#include <string>
#include <vector>

namespace
{
using Event=oscairn_event;
constexpr uint32_t frames=128;
int failures=0;
void check(bool ok,const std::string& what){if(!ok){std::printf("FAIL: %s\n",what.c_str());++failures;}}

template<class T> Event from(const T& x){Event e{};std::memcpy(e.bytes,&x,sizeof x);return e;}

struct Core // one implementation's entry points, as core_diff.cpp's
{
    const char* name;
    decltype(&rust_core_route_new) routeNew; decltype(&rust_core_route_free) routeFree; decltype(&rust_core_route_live) routeLive;
    decltype(&rust_core_notes_new) notesNew; decltype(&rust_core_notes_free) notesFree; decltype(&rust_core_notes_begin) notesBegin;
    decltype(&rust_core_notes_route) notesRoute; decltype(&rust_core_notes_finish) notesFinish; decltype(&rust_core_notes_end) notesEnd;
};
#define OSCAIRN_CORE_TABLE(p) Core{#p,p##route_new,p##route_free,p##route_live,p##notes_new,p##notes_free,p##notes_begin,p##notes_route,p##notes_finish,p##notes_end}
const Core cores[2]{OSCAIRN_CORE_TABLE(cpp_core_),OSCAIRN_CORE_TABLE(rust_core_)};

// voices: all three sounds' at each block; least: the fewest any one sound had.
struct Run { std::vector<uint32_t> voices,least; uint64_t faded=0; };
// oneWorker: every effect in the instrument's worker (one lane), else a worker each.
Run run(const Core& core,uint32_t blocks,bool oneWorker,float effect)
{
    using A=oscairn::Graph::Activity;
    constexpr uint32_t effects=21,contexts=3;
    struct Act { alignas(A) unsigned char bytes[sizeof(A)]{}; A* get(){return std::launder(reinterpret_cast<A*>(bytes));} oscairn_activity* core(){return reinterpret_cast<oscairn_activity*>(bytes);} };
    std::vector<Act> act(1+effects+contexts);
    for(auto& a:act)new(a.bytes) A;
    act[0].get()->seedRest=80;act[0].get()->seedNote=40;
    for(uint32_t e=0;e<effects;++e)act[1+e].get()->seedRest=effect;
    oscairn_route* routes[contexts];oscairn_zones zones{};oscairn_source sources[contexts];oscairn_context ctx[contexts];
    for(uint32_t c=0;c<contexts;++c){routes[c]=core.routeNew(1,1,256);sources[c]={routes[c],&zones,nullptr,0,int16_t(c),false};ctx[c]={int16_t(c),act[1+effects+c].core()};}
    const oscairn_link link{1,1.f};
    std::vector<oscairn_link> links(effects);
    std::vector<oscairn_step> steps;
    steps.push_back({act[0].core(),sources,contexts,ctx,contexts,0,false,false,&link,1});
    for(uint32_t e=0;e<effects;++e){links[e]={e+2,1.f};steps.push_back({act[1+e].core(),nullptr,0,nullptr,0,oneWorker?0u:1u+e,false,false,e+1<effects?&links[e]:nullptr,e+1<effects?1u:0u});}
    std::vector<uint8_t> budgeted(steps.size());
    bool flags[64]{};
    oscairn_notes* notes=core.notesNew(steps.data(),uint32_t(steps.size()),oneWorker?1u:1u+effects,flags);
    const oscairn_block timing{-1,2666.67f,2266.67f,1333.33f,3,96000,0};
    std::vector<Event> merged(kick::clap::Host::maxProcessEvents),ends,due;
    std::vector<int16_t> heard(merged.size());
    uint32_t id=0;Run r;
    for(uint32_t b=0;b<blocks;++b)
    {
        core.notesBegin(notes,&timing);
        std::vector<Event> in;std::vector<int16_t> src;
        for(int16_t c=0;c<int16_t(contexts);++c)
        {
            auto on=[&](uint16_t type,int16_t key){clap_event_note_t n{};n.header={sizeof n,0,CLAP_CORE_EVENT_SPACE_ID,type,0};n.note_id=-1;n.channel=c;n.key=key;n.velocity=.8;in.push_back(from(n));};
            if(b==0)
            {
                clap_event_midi_t m{};m.header={sizeof m,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};m.data[0]=uint8_t(0xb0|c);m.data[1]=64;m.data[2]=127;in.push_back(from(m));
                for(int16_t k=0;k<6;++k){on(CLAP_EVENT_NOTE_ON,int16_t(48+k));on(CLAP_EVENT_NOTE_OFF,int16_t(48+k));}
            }
            else if((b+125*uint32_t(c))%375==0){const int16_t k=int16_t(48+(b/375)%6);on(CLAP_EVENT_NOTE_OFF,k);on(CLAP_EVENT_NOTE_ON,k);on(CLAP_EVENT_NOTE_OFF,k);}
            while(src.size()<in.size())src.push_back(c);
        }
        std::vector<oscairn_key> keys;
        for(uint32_t i=0;i<in.size();++i)keys.push_back({0,i,&in[i],src[i],0xff,false});
        bool overflow=false;
        const uint32_t n=core.notesRoute(notes,0,keys.data(),uint32_t(keys.size()),frames,&id,merged.data(),uint32_t(merged.size()),heard.data(),&overflow);
        due.swap(ends);ends.clear();
        for(uint32_t i=0;i<n;++i)
        {
            clap_event_note_t x;std::memcpy(&x,merged[i].bytes,sizeof x);
            if(x.header.space_id==CLAP_CORE_EVENT_SPACE_ID && x.header.size==sizeof x && x.header.type==CLAP_EVENT_NOTE_CHOKE)
            {x.header.type=CLAP_EVENT_NOTE_END;ends.push_back(from(x));}
        }
        uint32_t voices=0,least=UINT32_MAX;for(auto* rt:routes){const auto live=core.routeLive(rt);voices+=live;least=std::min(least,live);}
        const oscairn_finish played{80.f+40.f*float(voices),true,false,false,false,true,0,0,nullptr,0,due.data(),uint32_t(due.size())};
        core.notesFinish(notes,0,&played);
        for(uint32_t e=0;e<effects;++e)
        {
            bool none=false;
            core.notesRoute(notes,1+e,nullptr,0,frames,&id,merged.data(),uint32_t(merged.size()),nullptr,&none);
            const oscairn_finish fx{effect,true,false,false,false,false,0,0,nullptr,0,nullptr,0};
            core.notesFinish(notes,1+e,&fx);
        }
        core.notesEnd(notes,2266.67f-played.took-effect*effects/(oneWorker?1.f:float(effects)));
        r.voices.push_back(voices);r.least.push_back(least);
    }
    auto* a=act[0].get();
    r.faded=a->stolen.load();
    core.notesFree(notes);for(auto* rt:routes)core.routeFree(rt);
    return r;
}
}

int main()
{
    // The time of the effects beside the instrument in its worker is not its notes' to give up: the voices a part keeps
    // with one worker for everything are fewer as they cost more, and never none (the first second's sustain pedal
    // aside, every sound holds a voice at every block), where a worker each leaves them what the instrument alone fits.
    for(const auto& core:cores)for(const float effect:{7.f,20.f,40.f})
    {
        const auto each=run(core,3750,false,effect),one=run(core,3750,true,effect);
        std::printf("%s, effects %.0f us: voices at 1 s steps, a worker each:",core.name,effect);for(uint32_t b=375;b<3750;b+=375)std::printf(" %u",each.voices[b]);
        std::printf("; one worker:");for(uint32_t b=375;b<3750;b+=375)std::printf(" %u",one.voices[b]);
        uint32_t fewest=UINT32_MAX;for(uint32_t b=375;b<3750;++b)fewest=std::min(fewest,one.least[b]);
        std::printf("; the fewest one sound had %u\n",fewest);
        check(fewest>=1,std::string(core.name)+": with every effect in one worker (each "+std::to_string(int(effect))+" us) a sound is never left without a voice: "+std::to_string(fewest));
        check(each.voices[3000]>=21,std::string(core.name)+": a worker each leaves the instrument its voices: "+std::to_string(each.voices[3000]));
    }
    return failures?1:0;
}
