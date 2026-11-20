// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0046: voice importance across parts, the owner's scenario, driven through the Rust engine core's C interface with
// the plugins modeled (no clock): a block's time is the sum of the voices it sounds. The plugins give their voices up
// through the SDK's own order (voice-budget-plugin.hpp: victim() within the limits the core sets, Importance for the
// report draft 2 adds), as an engine built on it does.
//
// A piano part with 30 notes under the sustain pedal (struck one a block, older ones quieter as a piano's decay) and an
// expensive lead holding two keys, in every placement: a worker per plugin, a chain each (to the budget the same: a
// lane each), a pool with both in one worker or apart, one instance shared by both parts, and in the engine's process.
//   - Same thread (pool together, shared, in process): the lane over its time gives up the piano's oldest, quietest
//     pedaled voices, as many as the lane's excess and no more (within a voice or two), the lead none.
//   - Apart, the device out of core time (the lanes' total past it): the same, from the piano's lane.
//   - Apart, the lead's own lane over its time: the lead gives up its own released voices; the piano, on another lane,
//     loses nothing (taking its voices would free nothing the lead's thread lacks).
//   - The piano without the report (draft 1, the host's notes): the same order from what the host knows of its notes.
//   - Held 2,000 blocks with a new piano note every 25: no flapping, the lead never touched, no part silent, and the
//     plugins keep to the caps they are given (no importance_mismatch).
//   oscairn-core-importance-test
#include "../src/engine_core.h"
#include "oscairn/graph.h"
#include <clap/clap.h>
#include <oscairn-clap/voice-budget-plugin.hpp>
#include <algorithm>
#include <cmath>
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
constexpr double blockMs=frames*1000.0/48000;
int failures=0;
void check(bool ok,const std::string& what){if(!ok){std::printf("FAIL: %s\n",what.c_str());++failures;}}
template<class T> Event from(const T& x){Event e{};std::memcpy(e.bytes,&x,sizeof x);return e;}

enum class Mode { Plugin, Chain, PoolTogether, PoolApart, Shared, InProcess };
const char* name(Mode m){switch(m){case Mode::Plugin:return "a worker per plugin";case Mode::Chain:return "chain";case Mode::PoolTogether:return "pool, one worker";case Mode::PoolApart:return "pool, apart";case Mode::Shared:return "one shared instance";default:return "in process";}}
bool apart(Mode m){return m==Mode::Plugin || m==Mode::Chain || m==Mode::PoolApart;}

struct Setup
{
    Mode mode=Mode::PoolTogether;
    float pianoUs=20,leadUs=300;   // a voice's cost a block
    uint32_t leadKeys=2;            // held; with leadReleased more struck and let go
    uint32_t leadReleased=0;
    float allowance=1333.33f,budget=-1,cores=3;
    bool pianoReports=true;
    uint32_t blocks=400,newEvery=0; // a new piano note every newEvery blocks from 100 on (0: none)
};
struct Seen { uint32_t pianoLost=0,leadLost=0,pianoLeft=0,leadLeft=0,leadHeldLeft=0; bool oldestFirst=true; uint32_t fewestPiano=UINT32_MAX,fewestLead=UINT32_MAX; uint64_t mismatch=0,decisionsLate=0; float excessVoices=0,missingVoices=0; };

// A modeled instance: its voices, each a note's; its contexts (a channel each); the sustain pedal by channel.
struct Voice { bool on=false,held=false,pedaled=false; int32_t id=-1; int16_t channel=0,key=0; float db=0; uint32_t born=0; uint64_t order=0; };
struct Instance
{
    std::vector<int16_t> channels; std::vector<uint32_t> ids; // its contexts: a part's channel and sound ID each (SELECTED alone)
    bool reports=true,ownVoices=true;
    float weight=1; // a lead voice's cost against a piano voice's, which a shared instance reports
    std::vector<Voice> voices=std::vector<Voice>(128);
    bool pedal[16]{};uint64_t started=0;
    oscairn::voices::Limits limits;oscairn::voices::Importance<> importance;
    std::vector<Event> ends;
    uint32_t context(int16_t channel) const{for(size_t c=0;c<channels.size();++c)if(channels[c]==channel)return ids[c];return OSCAIRN_VOICE_BUDGET_SELECTED;}
    float level(const Voice& v,uint32_t block) const{return v.db-(v.channel==0?6.f*float(block-v.born)*float(blockMs)/1000.f:0.f);} // a piano decays 6 dB a second
    oscairn::voices::Candidate candidate(const Voice& v,uint32_t block) const
    {return {context(v.channel),true,v.held,false,false,std::pow(10.0,level(v,block)/20.0),v.order,v.pedaled,double(block-v.born)*blockMs,v.channel?weight:1.f};}
    uint32_t sounding(int32_t channel=-1) const{uint32_t n=0;for(const auto& v:voices)n+=v.on && (channel<0 || v.channel==channel);return n;}
    void end(Voice& v){clap_event_note_t x{};x.header={sizeof x,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_END,0};x.note_id=v.id;x.channel=v.channel;x.key=v.key;ends.push_back(from(x));v.on=false;}
    // victim() over every voice, within one context (`only`) or the instance.
    int victim(uint32_t block,uint32_t incoming,uint32_t only=oscairn::voices::none)
    {
        std::vector<oscairn::voices::Candidate> c(voices.size());for(size_t i=0;i<voices.size();++i)if(voices[i].on)c[i]=candidate(voices[i],block);
        return oscairn::voices::victim(voices.size(),[&](size_t i){return voices[i].on?&c[i]:nullptr;},limits,incoming,only);
    }
    void keepWithin(uint32_t block,std::vector<Voice>& lost)
    {
        for(size_t c=0;c<channels.size();++c){const auto l=limits.of(ids[c],true).limit;while(l && sounding(channels[c])>l){const int i=victim(block,oscairn::voices::none,ids[c]);if(i<0)break;lost.push_back(voices[size_t(i)]);end(voices[size_t(i)]);}}
        while(limits.limit && sounding()>limits.limit){const int i=victim(block,oscairn::voices::none);if(i<0)break;lost.push_back(voices[size_t(i)]);end(voices[size_t(i)]);}
    }
    void play(const Event& e,uint32_t block,std::vector<Voice>& lost)
    {
        clap_event_header_t h;std::memcpy(&h,e.bytes,sizeof h);
        if(h.space_id!=CLAP_CORE_EVENT_SPACE_ID)return;
        if(h.type==CLAP_EVENT_MIDI){clap_event_midi_t m;std::memcpy(&m,e.bytes,sizeof m);if((m.data[0]&0xf0)==0xb0 && m.data[1]==64){const int c=m.data[0]&15;pedal[c]=m.data[2]>=64;if(!pedal[c])for(auto& v:voices)if(v.on && v.channel==c && v.pedaled){v.pedaled=false;}}return;}
        if(h.size!=sizeof(clap_event_note_t))return;
        clap_event_note_t n;std::memcpy(&n,e.bytes,sizeof n);
        if(h.type==CLAP_EVENT_NOTE_ON)
        {
            const uint32_t id=context(n.channel);
            const auto own=limits.of(id,true).limit;
            if(own && sounding(n.channel)>=own){const int i=victim(block,id,id);if(i>=0){lost.push_back(voices[size_t(i)]);end(voices[size_t(i)]);}}
            if(limits.limit && sounding()>=limits.limit){const int i=victim(block,id);if(i>=0){lost.push_back(voices[size_t(i)]);end(voices[size_t(i)]);}}
            for(auto& v:voices)if(!v.on){v={true,true,false,n.note_id,n.channel,n.key,float(20*std::log10(std::max(n.velocity,1e-3))),block,++started};break;}
            return;
        }
        for(auto& v:voices)if(v.on && v.id==n.note_id && v.key==n.key && v.channel==n.channel)
        {
            if(h.type==CLAP_EVENT_NOTE_OFF && v.held){v.held=false;v.pedaled=pedal[v.channel];if(!v.pedaled)v.db-=40;} // a release: soon gone
            if(h.type==CLAP_EVENT_NOTE_CHOKE){lost.push_back(v);end(v);}
        }
    }
};

Seen run(const Setup& setup)
{
    using A=oscairn::Graph::Activity;
    struct Act { alignas(A) unsigned char bytes[sizeof(A)]{}; A* get(){return std::launder(reinterpret_cast<A*>(bytes));} oscairn_activity* core(){return reinterpret_cast<oscairn_activity*>(bytes);} };
    const bool shared=setup.mode==Mode::Shared,inProcess=setup.mode==Mode::InProcess;
    // Steps: the piano's and the lead's instance, or one for both. Part 1 the piano (priority 5), part 2 the lead (10).
    const uint32_t stepCount=shared?1:2;
    std::vector<Act> act(stepCount+(shared?2:0));
    for(auto& a:act)new(a.bytes) A;
    oscairn_zones zones[2]{};
    for(int p=0;p<2;++p){auto& z=zones[p].zones[0];z.low_key=0;z.high_key=127;z.low_velocity=0;z.high_velocity=1;z.channels=uint16_t(1u<<p);z.curve=1;z.sources=~0ull;z.max_notes=128;z.part=uint32_t(p+1);z.reserve=0;z.priority=uint8_t(p?10:5);zones[p].count=1;}
    oscairn_route* routes[2]{rust_core_route_new(1,1,256),rust_core_route_new(1,1,256)};
    oscairn_source sources[2];for(int p=0;p<2;++p)sources[p]={routes[p],&zones[p],nullptr,0,int16_t(shared?p:-1),false,-1};
    const oscairn_context contexts[2]{{0,act[1].core(),7},{1,act[2 % act.size()].core(),8}};
    std::vector<oscairn_step> steps(stepCount);
    std::vector<Instance> inst(stepCount);
    if(shared)
    {
        steps[0]=oscairn_step{act[0].core(),sources,2,contexts,2,0,false,false,nullptr,0};
        inst[0].channels={0,1};inst[0].ids={7,8};inst[0].weight=setup.leadUs/setup.pianoUs;
    }
    else for(uint32_t p=0;p<2;++p)
    {
        steps[p]=oscairn_step{act[p].core(),&sources[p],1,nullptr,0,apart(setup.mode)?p:0u,inProcess,false,nullptr,0};
        inst[p].channels={int16_t(p)};inst[p].ids={OSCAIRN_VOICE_BUDGET_SELECTED};
    }
    if(!setup.pianoReports && !shared){inst[0].reports=false;inst[0].ownVoices=false;}
    bool flags[8]{};
    oscairn_notes* notes=rust_core_notes_new(steps.data(),stepCount,apart(setup.mode)?2:1,flags);
    const oscairn_block timing{setup.budget,2666.67f,2266.67f,setup.allowance,setup.cores,96000,0};
    std::vector<Event> merged(kick::clap::Host::maxProcessEvents);
    uint32_t id=0;Seen seen;std::vector<Voice> lost;
    float excess=0; // the voices' cost past what the rule allows, at the first decision
    float before[2]{},missing=0;
    for(uint32_t b=0;b<setup.blocks;++b)
    {
        rust_core_notes_begin(notes,&timing);
        // The player: the pedal down on the piano's channel; 30 piano notes, struck and let go, one a block, softer
        // as they go; the lead's keys held from block 40; later a new piano note now and then.
        std::vector<Event> in;std::vector<int16_t> src;
        auto note=[&](uint16_t type,int16_t channel,int16_t key,double velocity){clap_event_note_t n{};n.header={sizeof n,0,CLAP_CORE_EVENT_SPACE_ID,type,0};n.note_id=-1;n.channel=channel;n.key=key;n.velocity=velocity;in.push_back(from(n));src.push_back(channel);};
        if(b==0){clap_event_midi_t m{};m.header={sizeof m,0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};m.data[0]=0xb0;m.data[1]=64;m.data[2]=127;in.push_back(from(m));src.push_back(0);}
        if(b>=1 && b<=30){note(CLAP_EVENT_NOTE_ON,0,int16_t(30+b),.9);note(CLAP_EVENT_NOTE_OFF,0,int16_t(30+b),0);}
        // The lead's struck and let go notes come first: one struck after the held keys is protected at its note-on
        // (RFC 0046's 50 ms), so a held key would give way to it.
        if(b>=40 && b<40+setup.leadReleased){note(CLAP_EVENT_NOTE_ON,1,int16_t(90+b),.8);note(CLAP_EVENT_NOTE_OFF,1,int16_t(90+b),0);}
        if(b>=40+setup.leadReleased && b<40+setup.leadReleased+setup.leadKeys)note(CLAP_EVENT_NOTE_ON,1,int16_t(80+b-40),.8);
        if(setup.newEvery && b>=100 && b%setup.newEvery==0){const int16_t k=int16_t(70+(b/setup.newEvery)%10);note(CLAP_EVENT_NOTE_ON,0,k,.9);note(CLAP_EVENT_NOTE_OFF,0,k,0);}
        float lanes[2]{};
        for(uint32_t s=0;s<stepCount;++s)
        {
            auto& x=inst[s];
            std::vector<oscairn_key> keys;
            for(uint32_t i=0;i<in.size();++i)
                if(shared || src[i]==int16_t(s))keys.push_back({0,i,&in[i],int16_t(shared?src[i]:0),0xff,false});
            bool overflow=false;
            const uint32_t n=rust_core_notes_route(notes,s,keys.data(),uint32_t(keys.size()),frames,&id,merged.data(),uint32_t(merged.size()),nullptr,&overflow);
            // The limits the core gives it, once its notes are routed, before its block (run.rs: prepare, then limit_voices).
            uint32_t ctx[2]{};
            if(x.ownVoices)
            {
                const uint32_t limit=rust_core_notes_voice_limit(notes,s,x.sounding(),ctx,shared?2:0);
                act[s].get()->voiceLimit=limit;
                oscairn_voice_budget_context_t c[2]{};
                for(size_t k=0;k<x.channels.size();++k)c[k]={x.ids[k],shared?ctx[k]:0,0,uint8_t(x.channels[k]?10:5),0};
                x.limits.set(limit,c,uint32_t(x.channels.size()));
            }
            x.ends.clear();
            if(x.ownVoices)x.keepWithin(b,lost);
            for(uint32_t i=0;i<n;++i)x.play(merged[i],b,lost);
            // Its block: each voice's cost; its report.
            float took=10;for(const auto& v:x.voices)if(v.on)took+=v.channel==0?setup.pianoUs:setup.leadUs;
            lanes[apart(setup.mode)?s:0]+=took;
            std::vector<oscairn::voices::Candidate> c(x.voices.size());for(size_t i=0;i<x.voices.size();++i)if(x.voices[i].on)c[i]=x.candidate(x.voices[i],b);
            x.importance.list(x.voices.size(),[&](size_t i){return x.voices[i].on?&c[i]:nullptr;},x.limits);
            oscairn_voice_budget_importance_t lists[OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS];
            const uint32_t listed=x.reports?x.importance.write(lists,OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS):0;
            const uint32_t ids[2]{7,8};
            const oscairn_finish f{took,true,false,false,x.ownVoices,true,x.sounding(),0,nullptr,0,x.ends.data(),uint32_t(x.ends.size()),lists,listed,ids,shared?2u:0u};
            rust_core_notes_finish(notes,s,&f);
        }
        // Who went: the piano's notes struck earlier must have gone before any struck later (oldest, quietest first).
        for(const auto& v:lost){if(v.channel==0){++seen.pianoLost;for(const auto& w:inst[0].voices)if(w.on && w.channel==0 && w.born<v.born && b<100)seen.oldestFirst=false;}else ++seen.leadLost;}
        if(!lost.empty() && b>=100)seen.decisionsLate+=lost.size();
        // The excess: the busiest lane past its soft limit in the block before the first voice went (a plugin gives voices
        // up at the start of its block, so this block already has fewer).
        // Past the lane's limit is what is missing; the rule takes down to its soft limit (85 %), so fades have time: at
        // least the one, at most the other (one cost per voice for a shared instance's voices undercounts the piano's).
        if(!lost.empty() && excess==0){const float limit=0.8f*(inProcess?2266.67f:setup.allowance);float total=0;for(float l:before)total=std::max(total,l);excess=std::max(0.f,(total-0.85f*limit)/setup.pianoUs);missing=std::max(0.f,(total-limit)/setup.pianoUs);}
        std::copy(lanes,lanes+2,before);
        lost.clear();
        rust_core_notes_end(notes,2000);
        uint32_t piano=0,lead=0;for(const auto& x:inst){piano+=x.sounding(0);lead+=x.sounding(1);}
        if(b>=45){seen.fewestPiano=std::min(seen.fewestPiano,piano);seen.fewestLead=std::min(seen.fewestLead,lead);}
    }
    for(const auto& x:inst){seen.pianoLeft+=x.sounding(0);seen.leadLeft+=x.sounding(1);for(const auto& v:x.voices)seen.leadHeldLeft+=v.on && v.channel==1 && v.held;}
    for(auto& a:act)seen.mismatch+=a.get()->importanceMismatch.load();
    seen.excessVoices=excess;seen.missingVoices=missing;
    rust_core_notes_free(notes);for(auto* r:routes)rust_core_route_free(r);
    return seen;
}
}

int main()
{
    const Mode modes[]{Mode::Plugin,Mode::Chain,Mode::PoolTogether,Mode::PoolApart,Mode::Shared,Mode::InProcess};
    std::printf("%-22s %-28s %6s %6s %6s %6s\n","placement","case","piano-","lead-","piano","lead");
    auto row=[](const char* placement,const char* what,const Seen& s){std::printf("%-22s %-28s %6u %6u %6u %6u\n",placement,what,s.pianoLost,s.leadLost,s.pianoLeft,s.leadLeft);};
    for(const Mode m:modes)
    {
        Setup s;s.mode=m;
        if(m==Mode::InProcess){s.pianoUs=40;s.leadUs=600;} // the engine's process has the callback's time (80 % of the deadline)
        if(apart(m))
        {
            // The device out of core time: one core's budget, the lanes each within their own.
            Setup d=s;d.allowance=2000;d.budget=1500;d.cores=1;d.pianoUs=40;d.leadUs=400;
            const auto r=run(d);row(name(m),"device out of core time",r);
            check(r.leadLost==0 && r.leadHeldLeft==2,std::string(name(m))+": the lead keeps both keys for the lanes' total");
            check(r.pianoLost>=1 && r.pianoLeft>=1,std::string(name(m))+": the piano's voices go for the total, never all: "+std::to_string(r.pianoLost));
            check(r.oldestFirst,std::string(name(m))+": the piano's oldest, quietest voices first");
            // The lead's own lane over its time: only the lead's engine can free it.
            Setup o=s;o.leadKeys=2;o.leadReleased=3;o.leadUs=300;
            const auto q=run(o);row(name(m),"lead's own lane over",q);
            check(q.pianoLost==0,std::string(name(m))+": the piano, on another lane, loses nothing for the lead's lane: "+std::to_string(q.pianoLost));
            check(q.leadHeldLeft==2 && q.leadLost>=1,std::string(name(m))+": the lead gives up its own released voices, keeps its held keys");
            continue;
        }
        const auto r=run(s);row(name(m),"same thread over",r);
        check(r.leadLost==0 && r.leadHeldLeft==2,std::string(name(m))+": the lead keeps both keys: lost "+std::to_string(r.leadLost));
        check(r.pianoLeft>=1,std::string(name(m))+": the piano keeps a voice");
        check(r.oldestFirst,std::string(name(m))+": the piano's oldest, quietest voices first");
        check(float(r.pianoLost)<=std::ceil(r.excessVoices)+2 && float(r.pianoLost)+2>=std::floor(r.missingVoices),std::string(name(m))+": as many piano voices as the excess, no more: "+std::to_string(r.pianoLost)+" for "+std::to_string(r.missingVoices)+" to "+std::to_string(r.excessVoices));
        check(r.mismatch==0,std::string(name(m))+": every plugin kept to its cap");
    }
    // The piano without the report (draft 1): the host ranks its notes from what it knows (the pedal, the key, age).
    {
        Setup s;s.mode=Mode::PoolTogether;s.pianoReports=false;
        const auto r=run(s);row("pool, one worker","piano without the report",r);
        check(r.leadLost==0 && r.leadHeldLeft==2,"without the piano's report: the lead keeps both keys");
        check(r.pianoLost>=1 && r.pianoLeft>=1,"without the piano's report: its pedaled notes go, never all");
    }
    // No flapping: the overload held 2,000 blocks, a new piano note every 25.
    {
        Setup s;s.mode=Mode::PoolTogether;s.blocks=2000;s.newEvery=25;
        const auto r=run(s);row("pool, one worker","2,000 blocks, new notes",r);
        check(r.leadLost==0 && r.leadHeldLeft==2,"held: the lead never touched");
        check(r.fewestPiano>=1 && r.fewestLead>=2,"held: no part silent");
        check(r.mismatch==0,"held: every plugin kept to its cap");
        // Each new note may take one older pedaled voice; more than two given up a new note is the limit flapping.
        check(r.decisionsLate<=2*((2000-100)/25),"held: no more given up than the new notes bring: "+std::to_string(r.decisionsLate));
    }
    std::printf(failures?"FAIL: %d\n":"ok: voice importance across parts in every placement\n",failures);
    return failures?1:0;
}
