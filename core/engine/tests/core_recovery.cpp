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
        rust_core_notes_end(notes,2266.67f-f.took); // the callback's slack: its deadline less the plugin's block
        r.limit.push_back(a->voiceLimit);r.voices.push_back(voices);r.lost.push_back(lost);
    }
    r.unfit=a->unfitBlocks;
    rust_core_notes_free(notes);rust_core_route_free(route);
    return r;
}
uint32_t count(const std::vector<bool>& v,uint32_t from,uint32_t to){uint32_t n=0;for(uint32_t i=from;i<to && i<v.size();++i)n+=v[i];return n;}

// Three sounds on one shared instance (a plugin without oscairn.voice-budget: the host fades its notes) on lane 0, into an
// effect on lane 1, as Sick tines' five parts on one FM instance into the effect workers. The sustain pedal down on each
// sound's channel, six keys struck, then each sound strikes a key again every 375 blocks (1 s), staggered, so notes pile
// up under the pedal. The callback's time is the instrument's block (20 us and 30 us a voice) and the effects' (`effects`
// us a block, by block); with less than 150 us of its deadline left the effect loses a block, by a chance of the shortfall
// over 600 us (a fixed seed): the losses on the downstream lane, the voices on the upstream one.
struct Chain { std::vector<uint32_t> voices; std::vector<bool> lost; uint64_t faded=0,unfit=0; };
template<class Effects> Chain chain(uint32_t blocks,Effects&& effects)
{
    using A=oscairn::Graph::Activity;
    struct Act { alignas(A) unsigned char bytes[sizeof(A)]{}; A* get(){return std::launder(reinterpret_cast<A*>(bytes));} oscairn_activity* core(){return reinterpret_cast<oscairn_activity*>(bytes);} };
    Act instrument,effect,context[3];
    new(instrument.bytes) A;new(effect.bytes) A;for(auto& c:context)new(c.bytes) A;
    instrument.get()->seedRest=20;instrument.get()->seedNote=30;effect.get()->seedRest=300;
    oscairn_route* routes[3];oscairn_zones zones{};oscairn_source sources[3];oscairn_context contexts[3];
    for(int c=0;c<3;++c){routes[c]=rust_core_route_new(1,1,256);sources[c]={routes[c],&zones,nullptr,0,int16_t(c),false};contexts[c]={int16_t(c),context[c].core()};}
    const oscairn_link link{1,1.f};
    const oscairn_step steps[2]{{instrument.core(),sources,3,contexts,3,0,false,false,&link,1},{effect.core(),nullptr,0,nullptr,0,1,false,false,nullptr,0}};
    bool budgeted[2]{};
    oscairn_notes* notes=rust_core_notes_new(steps,2,2,budgeted);
    const oscairn_block timing{-1,2666.67f,2266.67f,1333.33f,2,96000,0};
    std::vector<Event> merged(kick::clap::Host::maxProcessEvents),ends,due;
    std::vector<int16_t> heard(merged.size());
    uint32_t id=0;uint64_t seed=12345;Chain r;
    auto chance=[&]{seed=seed*6364136223846793005ull+1442695040888963407ull;return double(seed>>11)/double(1ull<<53);};
    for(uint32_t b=0;b<blocks;++b)
    {
        rust_core_notes_begin(notes,&timing);
        std::vector<Event> in;std::vector<int16_t> src;
        for(int16_t c=0;c<3;++c)
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
        const uint32_t n=rust_core_notes_route(notes,0,keys.data(),uint32_t(keys.size()),frames,&id,merged.data(),uint32_t(merged.size()),heard.data(),&overflow);
        // The plugin: a note choked (a fade's end) ends the next block; every other note sounds on under the pedal.
        due.swap(ends);ends.clear();
        for(uint32_t i=0;i<n;++i)
        {
            clap_event_note_t x;std::memcpy(&x,merged[i].bytes,sizeof x);
            if(x.header.space_id==CLAP_CORE_EVENT_SPACE_ID && x.header.size==sizeof x && x.header.type==CLAP_EVENT_NOTE_CHOKE)
            {x.header.type=CLAP_EVENT_NOTE_END;ends.push_back(from(x));}
        }
        uint32_t voices=0;for(auto* rt:routes)voices+=rust_core_route_live(rt);
        const oscairn_finish played{20.f+30.f*float(voices),true,false,false,false,true,0,0,nullptr,0,due.data(),uint32_t(due.size())};
        rust_core_notes_finish(notes,0,&played);
        bool none=false;
        rust_core_notes_route(notes,1,nullptr,0,frames,&id,merged.data(),uint32_t(merged.size()),nullptr,&none);
        const float slack=2266.67f-played.took-effects(b);
        const bool lost=slack<150 && chance()<(150-slack)/600;
        const oscairn_finish fx{effects(b),true,false,lost,false,false,0,0,nullptr,0,nullptr,0};
        rust_core_notes_finish(notes,1,&fx);
        rust_core_notes_end(notes,slack);
        r.voices.push_back(voices);r.lost.push_back(lost);
    }
    r.faded=instrument.get()->stolenForLoss+instrument.get()->stolenForDeadline;r.unfit=instrument.get()->unfitBlocks;
    rust_core_notes_free(notes);for(auto* rt:routes)rust_core_route_free(rt);
    return r;
}
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
    {
        // The pedalled chain: effects of 1,900 us leave room for 6 voices across three sounds, for 75 s; then 500 us for 25 s.
        const auto r=chain(37500,[](uint32_t b){return b<28125?1900.f:500.f;});
        std::printf("chain: voices at 5 s steps:");for(uint32_t b=0;b<37500;b+=1875)std::printf(" %u",r.voices[b]);
        std::printf("; lost per 5 s:");for(uint32_t b=0;b<37500;b+=1875)std::printf(" %u",count(r.lost,b,b+1875));
        std::printf("; %llu given up, %llu unfit\n",(unsigned long long)r.faded,(unsigned long long)r.unfit);
        uint32_t settled=0;for(uint32_t b=0;b<28125;++b)if(r.lost[b])settled=b;
        std::printf("chain: last block lost %u (%.1f s), voices at 75 s %u, at the end %u\n",settled,settled/375.0,r.voices[28124],r.voices.back());
        check(settled<5625,"settled within 15 s, then 60 s with keys struck again and no block lost: last lost at "+std::to_string(settled));
        check(r.voices.back()>r.voices[28124]+5,"and voices come back when the effects' load falls: "+std::to_string(r.voices.back()));
    }
    return failures?1:0;
}
