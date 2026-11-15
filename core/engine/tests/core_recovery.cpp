// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045's guard, 12k: a voice-budget plugin's limit after it was lowered for lost blocks, driven through the Rust
// engine core's C interface with the plugin modeled so every block is a function of its voices (no clock): a block
// costs 20 us and 50 us a voice, and while an overload is on, a block with more than `over` voices is lost. Sixteen
// keys are held from the start and one is struck again every 4 blocks (let go and struck in the same block), so
// notes are never all let go, as with a pedal held through a passage.
//   transient: 10 voices fit from block 300 to 500, then nothing is lost again: voices go during it; after it the
//     limit is back to at least the voices that fit less one (its ceiling less one) within 150 blocks (0.4 s), past
//     the voices that fit by probing within 1,600 (4.3 s), and
//     every voice back by the end (5,000 blocks, the ceiling forgotten after 10 s healthy).
//   permanent: 8 voices fit from block 300 on, for 6,000 blocks (16 s): the limit settles under 9 and the blocks lost
//     after the first second of it stay few (one probe's losses each, the probes rare).
//   sparse: 8 voices fit and while more sound one block in 150 is lost (a chain near the callback's deadline): voices
//     still go, losses that far apart count, until they stop.
//   unfit: one block in 150 lost whatever sounds (the effects after it leave no room): down to one voice and held
//     there, the blocks lost with nothing left to give counted (unfitBlocks).
//   oscairn-core-recovery-test
#include "../src/engine_core.h"
#include "oscairn/graph.h"
#include <clap/clap.h>
#include <algorithm>
#include <climits>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
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
Event note(uint16_t type,int16_t key)
{
    clap_event_note_t n{};n.header={sizeof n,0,CLAP_CORE_EVENT_SPACE_ID,type,0};
    n.note_id=-1;n.port_index=0;n.channel=0;n.key=key;n.velocity=.8;return from(n);
}

struct Run { std::vector<uint32_t> limit,voices; std::vector<bool> lost; uint64_t unfit=0; };
// blocks: how long; overloaded: whether block b is in the overload; over: the voices that fit in it; every: one block
// in this many loses then (1: each; more: sparse losses, as a chain near the callback's deadline loses them).
template<class Overloaded> Run run(uint32_t blocks,Overloaded&& overloaded,uint32_t over,uint32_t every=1)
{
    oscairn_route* route=rust_core_route_new(1,1,256);
    alignas(oscairn::Graph::Activity) unsigned char bytes[sizeof(oscairn::Graph::Activity)]{};
    auto* a=new(bytes) oscairn::Graph::Activity;a->seedRest=20;a->seedNote=50;
    oscairn_zones zones{};
    const oscairn_source source{route,&zones,nullptr,0,-1,false};
    const oscairn_step step{reinterpret_cast<oscairn_activity*>(bytes),&source,1,nullptr,0,0,false,false,nullptr,0};
    bool budgeted=false;
    oscairn_notes* notes=rust_core_notes_new(&step,1,1,&budgeted);
    const oscairn_block timing{-1,2666.67f,2266.67f,1333.33f,1,96000,0};
    std::vector<Event> merged(kick::clap::Host::maxProcessEvents);
    uint32_t id=0;Run r;
    for(uint32_t b=0;b<blocks;++b)
    {
        rust_core_notes_begin(notes,&timing);
        std::vector<Event> in;
        if(b==0)for(int16_t k=0;k<16;++k)in.push_back(note(CLAP_EVENT_NOTE_ON,int16_t(48+k)));
        else if(b%4==0){in.push_back(note(CLAP_EVENT_NOTE_OFF,int16_t(48+b/4%16)));in.push_back(note(CLAP_EVENT_NOTE_ON,int16_t(48+b/4%16)));}
        std::vector<oscairn_key> keys;
        for(uint32_t i=0;i<in.size();++i)keys.push_back({0,i,&in[i],0,0xff,false});
        bool overflow=false;
        rust_core_notes_route(notes,0,keys.data(),uint32_t(keys.size()),frames,&id,merged.data(),uint32_t(merged.size()),nullptr,&overflow);
        const uint32_t live=rust_core_route_live(route),limit=a->voiceLimit;
        const uint32_t voices=limit?std::min(live,limit):live; // the plugin keeps within its limit
        const bool lost=overloaded(b) && voices>over && b%every==0;
        const oscairn_finish f{20.f+50.f*float(voices),true,false,lost,true,true,voices,50,nullptr,0,nullptr,0};
        rust_core_notes_finish(notes,0,&f);
        a->voiceLimit=rust_core_notes_voice_limit(notes,0,voices,nullptr,0);
        rust_core_notes_end(notes,150.f); // the callback 150 us inside its deadline, as on the unit
        r.limit.push_back(a->voiceLimit);r.voices.push_back(voices);r.lost.push_back(lost);
    }
    r.unfit=a->unfitBlocks;
    rust_core_notes_free(notes);rust_core_route_free(route);
    return r;
}
uint32_t count(const std::vector<bool>& v,uint32_t from,uint32_t to){uint32_t n=0;for(uint32_t i=from;i<to && i<v.size();++i)n+=v[i];return n;}
}

int main()
{
    // The numbers the rule states (RFC 0045, 12k): 8 healthy blocks for each voice back below the ceiling, a probe
    // above it after 750, each failed probe doubling that.
    constexpr uint32_t recovered=150,probed=1600; // 0.4 s; 4.3 s
    {
        const auto r=run(5000,[](uint32_t b){return b>=300 && b<500;},10);
        constexpr uint32_t fit=10;
        uint32_t before=r.voices[299],least=99,back=UINT32_MAX,above=UINT32_MAX;
        for(uint32_t b=300;b<500;++b)least=std::min(least,r.voices[b]);
        for(uint32_t b=500;b<5000 && back==UINT32_MAX;++b)if(r.limit[b]+1>=fit)back=b-500;
        for(uint32_t b=500;b<5000 && above==UINT32_MAX;++b)if(r.limit[b]>fit)above=b-500;
        std::printf("transient: %u voices before, at least %u in the overload (%u blocks lost), %u at its end: %u or more %u blocks after, past %u %u after; %u voices at the end, %u lost after it\n",
            before,least,count(r.lost,300,500),r.voices[499],fit-1,back,fit,above,r.voices.back(),count(r.lost,500,5000));
        check(before>fit,"more voices than fit in the overload before it");
        check(least<=fit,"voices go in the overload");
        check(back<=recovered,"after it, the limit is back to its ceiling less one within "+std::to_string(recovered)+" blocks: "+std::to_string(back));
        check(above<=probed,"and above it by probing within "+std::to_string(probed)+" blocks: "+std::to_string(above));
        check(r.limit.back()>=16,"until every voice is back, notes still held: "+std::to_string(r.limit.back()));
        check(count(r.lost,500,5000)==0,"nothing lost once the overload is over");
    }
    {
        const auto r=run(6300,[](uint32_t b){return b>=300;},8);
        const uint32_t late=count(r.lost,675,6300);uint32_t most=0;
        for(uint32_t b=675;b<6300;++b)most=std::max(most,r.voices[b]);
        std::printf("permanent: %u blocks lost in its first second, %u in the 15 s after, at most %u voices then, %u at the end\n",count(r.lost,300,675),late,most,r.voices.back());
        check(r.voices.back()<=8,"it settles at the voices that fit");
        check(late<=12,"probes lose few blocks: "+std::to_string(late));
    }
    {
        // Sparse losses: one block in 150 lost while more than 8 voices sound, for 30 s from block 300.
        const auto r=run(11550,[](uint32_t b){return b>=300;},8,150);
        std::printf("sparse: %u blocks lost in the first 10 s, %u in the 20 s after, %u voices at the end\n",count(r.lost,300,4050),count(r.lost,4050,11550),r.voices.back());
        check(r.voices.back()<=8,"sparse losses shed voices too: "+std::to_string(r.voices.back()));
        check(count(r.lost,4050,11550)<=8,"and the losses become rare: "+std::to_string(count(r.lost,4050,11550)));
    }
    {
        // A chain that cannot fit: one block in 150 lost whatever sounds. The voices go down to one and stay there,
        // and the blocks lost with nothing left to give are counted.
        const auto r=run(6300,[](uint32_t b){return b>=300;},0,150);
        uint32_t least=99;for(uint32_t b=3000;b<6300;++b)least=std::min(least,r.voices[b]);
        std::printf("unfit: %u voices at the end, at least %u from block 3000, %llu blocks lost with nothing left to give\n",r.voices.back(),least,(unsigned long long)r.unfit);
        check(r.voices.back()==1 && least==1,"down to one voice and held there: "+std::to_string(r.voices.back()));
        check(r.unfit>0,"the chain that cannot fit is counted");
    }
    return failures?1:0;
}
