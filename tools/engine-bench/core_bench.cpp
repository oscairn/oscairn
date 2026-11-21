// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine core's voice budget, timed per block: 8 parts, each on a lane of its own (chain), four pianos holding pedaled
// notes (0 to 240 in all, a re-struck key ending its earlier note as key assign Single does), reporting their voices and
// importance (oscairn.voice-budget draft 2) or not. Each phase of a block through the core's C interface: begin, the
// steps' routing and voice limits, their finish (the importance report taken), end (the lanes judged, recover); and the
// worker's side apart: building the report (Importance::list, the SDK's helper). The lanes stay well within their
// limits: the budget's cost when nothing has to be given up. An "admit" row strikes one note into each piano every block.
// Build at another commit with -DOSCAIRN_BENCH_DRAFT1 for a core whose finish has no importance fields.
//   oscairn-core-bench [--check] [blocks]
#include "../../core/engine/src/engine_core.h"
#include "oscairn/graph.h"
#include <clap/clap.h>
#include <oscairn-clap/voice-budget-plugin.hpp>
#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <new>
#include <string>
#include <vector>

namespace
{
using Event=oscairn_event;
using Clock=std::chrono::steady_clock;
constexpr uint32_t frames=128,parts=8,pianos=4;
constexpr double blockMs=frames*1000.0/48000;
template<class T> Event from(const T& x){Event e{};std::memcpy(e.bytes,&x,sizeof x);return e;}
double us(Clock::duration d){return std::chrono::duration<double,std::micro>(d).count();}

struct Voice { bool on=false,held=false,pedaled=false; int32_t id=-1; int16_t key=0; float db=0; uint32_t born=0; uint64_t order=0; };
struct Instance
{
    std::vector<Voice> voices=std::vector<Voice>(128);
    bool pedal=false;uint64_t started=0;
    oscairn::voices::Limits limits;
#ifndef OSCAIRN_BENCH_DRAFT1
    oscairn::voices::Importance<> importance;
#endif
    std::vector<Event> ends;
    uint32_t sounding() const{uint32_t n=0;for(const auto& v:voices)n+=v.on;return n;}
    void end(Voice& v){clap_event_note_t x{};x.header={sizeof x,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_END,0};x.note_id=v.id;x.channel=0;x.key=v.key;ends.push_back(from(x));v.on=false;}
#ifndef OSCAIRN_BENCH_DRAFT1
    oscairn::voices::Candidate candidate(const Voice& v,uint32_t block) const
    {return {OSCAIRN_VOICE_BUDGET_SELECTED,true,v.held,false,false,std::pow(10.0,(v.db-6.f*float(block-v.born)*float(blockMs)/1000.f)/20.0),v.order,v.pedaled,double(block-v.born)*blockMs,2};}
#endif
    void play(const Event& e,uint32_t block)
    {
        clap_event_header_t h;std::memcpy(&h,e.bytes,sizeof h);
        if(h.space_id!=CLAP_CORE_EVENT_SPACE_ID)return;
        if(h.type==CLAP_EVENT_MIDI){clap_event_midi_t m;std::memcpy(&m,e.bytes,sizeof m);if((m.data[0]&0xf0)==0xb0 && m.data[1]==64)pedal=m.data[2]>=64;return;}
        if(h.size!=sizeof(clap_event_note_t))return;
        clap_event_note_t n;std::memcpy(&n,e.bytes,sizeof n);
        if(h.type==CLAP_EVENT_NOTE_ON)
        {
            for(auto& v:voices)if(v.on && v.key==n.key)end(v); // key assign Single
            for(auto& v:voices)if(!v.on){v={true,true,false,n.note_id,n.key,float(20*std::log10(std::max(n.velocity,1e-3))),block,++started};break;}
            return;
        }
        for(auto& v:voices)if(v.on && v.id==n.note_id && v.key==n.key)
        {
            if(h.type==CLAP_EVENT_NOTE_OFF && v.held){v.held=false;v.pedaled=pedal;}
            if(h.type==CLAP_EVENT_NOTE_CHOKE)end(v);
        }
    }
};

struct Times { double begin=0,route=0,finish=0,end=0,worker=0; uint32_t blocks=0; uint64_t sounding=0; };

// `held` pedaled notes spread over the pianos; `reports`: the pianos and the lead list their voices; `admit`: a key struck
// into each piano every measured block.
Times run(uint32_t held,bool reports,bool admit,uint32_t measured)
{
    using A=oscairn::Graph::Activity;
    struct Act { alignas(A) unsigned char bytes[sizeof(A)]{}; A* get(){return std::launder(reinterpret_cast<A*>(bytes));} oscairn_activity* core(){return reinterpret_cast<oscairn_activity*>(bytes);} };
    std::vector<Act> act(parts);
    for(auto& a:act)new(a.bytes) A;
    std::vector<oscairn_zones> zones(parts);
    std::vector<oscairn_route*> routes(parts);
    std::vector<oscairn_source> sources(parts);
    std::vector<oscairn_step> steps(parts);
    std::vector<Instance> inst(parts);
    for(uint32_t p=0;p<parts;++p)
    {
        auto& z=zones[p].zones[0];z.low_key=0;z.high_key=127;z.low_velocity=0;z.high_velocity=1;z.channels=0xffff;z.curve=1;z.sources=~0ull;z.max_notes=128;z.part=p+1;z.priority=5;zones[p].count=1;
        routes[p]=rust_core_route_new(1,1,256);
        sources[p]={routes[p],&zones[p],nullptr,0,-1,false,-1};
        steps[p]=oscairn_step{act[p].core(),&sources[p],1,nullptr,0,p,false,false,nullptr,0};
    }
    bool flags[parts]{};
    oscairn_notes* notes=rust_core_notes_new(steps.data(),parts,parts,flags);
    const oscairn_block timing{-1,2666.67f,2266.67f,2000,3,96000,0};
    std::vector<Event> merged(kick::clap::Host::maxProcessEvents);
    uint32_t id=0;Times t;
    const uint32_t warm=held/pianos+40,blocks=warm+measured;
    for(uint32_t b=0;b<blocks;++b)
    {
        const bool timed=b>=warm;
        auto at=Clock::now();
        rust_core_notes_begin(notes,&timing);
        if(timed)t.begin+=us(Clock::now()-at);
        for(uint32_t s=0;s<parts;++s)
        {
            auto& x=inst[s];
            std::vector<Event> in;
            auto note=[&](uint16_t type,int16_t key){clap_event_note_t n{};n.header={sizeof n,0,CLAP_CORE_EVENT_SPACE_ID,type,0};n.note_id=-1;n.channel=0;n.key=key;n.velocity=.8;in.push_back(from(n));};
            if(s<pianos)
            {
                if(b==0){clap_event_midi_t m{};m.header={sizeof m,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};m.data[0]=0xb0;m.data[1]=64;m.data[2]=127;in.push_back(from(m));}
                const uint32_t mine=held/pianos+(s<held%pianos);
                if(b>=1 && b<=mine){note(CLAP_EVENT_NOTE_ON,int16_t(24+b));note(CLAP_EVENT_NOTE_OFF,int16_t(24+b));}
                if(admit && timed && mine){const int16_t k=int16_t(25+b%mine);note(CLAP_EVENT_NOTE_ON,k);note(CLAP_EVENT_NOTE_OFF,k);}
            }
            std::vector<oscairn_key> keys;
            for(uint32_t i=0;i<in.size();++i)keys.push_back({0,i,&in[i],0,0xff,false});
            bool overflow=false;
            at=Clock::now();
            const uint32_t n=rust_core_notes_route(notes,s,keys.data(),uint32_t(keys.size()),frames,&id,merged.data(),uint32_t(merged.size()),nullptr,&overflow);
            uint32_t ctx[1]{};
            const uint32_t limit=rust_core_notes_voice_limit(notes,s,x.sounding(),ctx,0);
            if(timed)t.route+=us(Clock::now()-at);
            act[s].get()->voiceLimit=limit;
            oscairn_voice_budget_context_t c{OSCAIRN_VOICE_BUDGET_SELECTED,0,0,5,0};
            x.limits.set(limit,&c,1);
            x.ends.clear();
            for(uint32_t i=0;i<n;++i)x.play(merged[i],b);
            const float took=100+10.f*float(x.sounding());
            // The worker's side: its report, built after its block.
#ifndef OSCAIRN_BENCH_DRAFT1
            at=Clock::now();
            std::vector<oscairn::voices::Candidate> cs(x.voices.size());for(size_t i=0;i<x.voices.size();++i)if(x.voices[i].on)cs[i]=x.candidate(x.voices[i],b);
            x.importance.list(x.voices.size(),[&](size_t i){return x.voices[i].on?&cs[i]:nullptr;},x.limits);
            oscairn_voice_budget_importance_t lists[OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS];
            const uint32_t listed=reports?x.importance.write(lists,OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS):0;
            if(timed)t.worker+=us(Clock::now()-at);
#endif
            oscairn_finish f{};
            f.took=took;f.processed=true;f.own_voices=reports;f.honors_volume=true;f.voices=reports?x.sounding():0;
            f.sent=x.ends.data();f.sent_count=uint32_t(x.ends.size());
#ifndef OSCAIRN_BENCH_DRAFT1
            f.importance=lists;f.importance_count=listed;
#endif
            at=Clock::now();
            rust_core_notes_finish(notes,s,&f);
            if(timed)t.finish+=us(Clock::now()-at);
            if(timed)t.sounding+=x.sounding();
        }
        at=Clock::now();
        rust_core_notes_end(notes,2000);
        if(timed){t.end+=us(Clock::now()-at);++t.blocks;}
    }
    rust_core_notes_free(notes);for(auto* r:routes)rust_core_route_free(r);
    return t;
}
}

int main(int argc,char** argv)
{
    bool check=false;uint32_t measured=20000;
    for(int i=1;i<argc;++i){if(std::string(argv[i])=="--check")check=true;else measured=uint32_t(std::max(1,std::atoi(argv[i])));}
    if(check)measured=200;
    std::printf("%-6s %-9s %-6s %8s %8s %8s %8s %8s | %8s %9s\n","held","reports","admit","begin","route","finish","end","core","worker","sounding");
    int failures=0;
    for(const bool admit:{false,true})
        for(const bool reports:{false,true})
            for(const uint32_t held:{0u,30u,120u,240u})
            {
                Times best;double bestCore=1e30;
                for(int r=0;r<(check?1:5);++r) // the fastest of five: the machine's noise only adds
                {
                    const auto t=run(held,reports,admit,measured);
                    const double core=(t.begin+t.route+t.finish+t.end)/t.blocks;
                    if(core<bestCore){bestCore=core;best=t;}
                }
                const double n=best.blocks;
                std::printf("%-6u %-9s %-6s %8.3f %8.3f %8.3f %8.3f %8.3f | %8.3f %9.1f\n",held,reports?"draft 2":"none",admit?"yes":"no",
                            best.begin/n,best.route/n,best.finish/n,best.end/n,bestCore,best.worker/n,double(best.sounding)/n);
                // Nothing given up: every note the pianos hold still sounds.
                if(!admit && best.sounding/best.blocks<held){std::printf("FAIL: %u held, %.1f sounding\n",held,double(best.sounding)/n);++failures;}
            }
    std::printf("us per block (all 8 parts); worker: building the pianos' reports, not the engine thread's\n");
    return failures?1:0;
}
