// SPDX-License-Identifier: AGPL-3.0-or-later
// The differential test of a plan's block (engine_core.h's run_*): the Rust implementation over its notes,
// audio and routes, driven by random plans and modeled plugins, and compared after every block with the
// outputs its C++ reference (Graph::process and beginBlock as they were) gave on the same streams: every
// call either makes on the plugins (in order, with what it hands them: the merged events, voice
// limits, listener events, deadlines), the graph's audio out, MIDI out, event drops, note IDs, every
// route's state, every activity and the parts' harmonies (vectors.h, core/engine/tests/vectors/engine-run.vec).
// Plans are replaced under it (retired connections, stranded plugins, taps gone, gains carried). The plugins are modeled:
// what each sends is a function of what it was handed (notes on and off as an arpeggiator's, NOTE_ENDs,
// parameter-target lane events, MIDI), blocks lost, late, reset, a clock that advances with each call.
// The core's guard rules the reference lacked get no input (as engine-core-diff): no missed blocks,
// no audio links, the callback's slack never low.
//   oscairn-run-diff-test replay FILE [plans] [blocks]
//   oscairn-run-diff-test record FILE COMMIT [plans] [blocks]   new vectors from the Rust core (docs/developer-guide.md)
#include "../src/engine_core.h"
#include "vectors.h"
#include "oscairn/graph.h"
#include <oscairn-clap/harmony.h>
#include <oscairn-clap/param-target.h>
#include <clap/clap.h>
#include <algorithm>
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <new>
#include <vector>

namespace
{
using Event=oscairn_event;
constexpr uint32_t frames=128,hosts=24;

struct Rng
{
    uint64_t s;
    uint64_t next() { s^=s<<13;s^=s>>7;s^=s<<17;return s; }
    uint32_t below(uint32_t n) { return uint32_t(next()%n); }
    double unit() { return double(next()>>11)/double(1ull<<53); }
};
template<class T> Event from(const T& x) { Event e{};std::memcpy(e.bytes,&x,sizeof x);return e; }
Event note(uint16_t type,uint32_t time,int16_t channel,int16_t key,double velocity,int32_t id=-1)
{
    clap_event_note_t n{};n.header={sizeof n,time,CLAP_CORE_EVENT_SPACE_ID,type,0};n.note_id=id;n.port_index=0;n.channel=channel;n.key=key;n.velocity=velocity;return from(n);
}
Event midi(uint32_t time,uint8_t a,uint8_t b,uint8_t c) { clap_event_midi_t m{};m.header={sizeof m,time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};m.data[0]=a;m.data[1]=b;m.data[2]=c;return from(m); }
Event param(uint32_t time,uint32_t id,double v,bool mod=false)
{
    clap_event_param_value_t p{};p.header={sizeof p,time,CLAP_CORE_EVENT_SPACE_ID,uint16_t(mod?CLAP_EVENT_PARAM_MOD:CLAP_EVENT_PARAM_VALUE),0};
    p.param_id=id;p.note_id=-1;p.port_index=p.channel=p.key=-1;p.value=v;return from(p);
}
Event lane(uint32_t time,uint32_t lane,bool set,double v,int32_t id,int16_t channel,int16_t key)
{
    oscairn_event_param_target_t e{};e.header={sizeof e,time,1000,OSCAIRN_EVENT_PARAM_TARGET,0};e.lane=lane;e.mode=set?OSCAIRN_PARAM_TARGET_SET:1;
    e.note_id=id;e.port_index=0;e.channel=channel;e.key=key;e.value=v;return from(e);
}
uint64_t hash(const void* p,size_t n,uint64_t h=1469598103934665603ull) { auto* b=static_cast<const uint8_t*>(p);for(size_t i=0;i<n;++i)h=(h^b[i])*1099511628211ull;return h; }

struct Activity
{
    alignas(oscairn::Graph::Activity) unsigned char bytes[sizeof(oscairn::Graph::Activity)]{};
    Activity() { new(bytes) oscairn::Graph::Activity; }
    oscairn::Graph::Activity& get() { return *std::launder(reinterpret_cast<oscairn::Graph::Activity*>(bytes)); }
    oscairn_activity* core() { return reinterpret_cast<oscairn_activity*>(bytes); }
};

// A plan.
struct Plan
{
    struct Source { int32_t from; uint32_t route; oscairn_zones zones{}; int16_t channel=-1; bool draining=false; };
    struct Step { uint32_t host; bool worker,late,ownVoices; std::vector<Source> sources; std::vector<int16_t> contexts; int32_t harmony=-1; std::vector<oscairn_lane_route> lanes; bool lanesIn=false; };
    std::vector<Step> steps; std::vector<uint32_t> waves;
    struct Harmony { uint32_t route; oscairn_zones zones{}; bool input; uint8_t root; uint16_t scale; };
    std::vector<Harmony> harmonies;
    struct Tap { int32_t step; uint32_t route; oscairn_zones zones{}; uint64_t ports; uint8_t channel; };
    std::vector<Tap> taps;
    std::vector<std::vector<oscairn_curve>> paths; std::vector<std::pair<double,bool>> controls;
    float budget=0; bool lossy=false; // lossy: blocks lost and reset, with the budget off (the Rust core's guard reacts to losses)
};

oscairn_zones zones(Rng& rng,bool wide=false)
{
    oscairn_zones z{};z.count=wide || rng.below(4)?1:0;
    auto& x=z.zones[0];x.low_key=uint8_t(wide?0:30+rng.below(10));x.high_key=uint8_t(wide?127:60+rng.below(40));x.high_velocity=1;x.channels=0xFFFF;x.curve=1;
    x.sources=~0ull;x.max_notes=uint16_t(rng.below(3)?0:2+rng.below(6));x.part=rng.below(3);x.reserve=uint16_t(rng.below(4));x.priority=uint8_t(rng.below(11));
    return z;
}

Plan randomPlan(Rng& rng,uint32_t& routes)
{
    Plan p;
    p.lossy=!rng.below(3);p.budget=p.lossy || rng.below(2)?0.f:-1.f; // no budget to shed for: its rules differ between the cores (engine-core-diff holds what they share)
    std::array<uint32_t,hosts> ids;for(uint32_t h=0;h<hosts;++h)ids[h]=h;
    for(uint32_t h=hosts;h>1;--h)std::swap(ids[h-1],ids[rng.below(h)]);
    const uint32_t n=1+rng.below(10);
    for(uint32_t i=0;i<n;++i)
    {
        // A plugin giving up its own voices only with no budget: the Rust core's voice limits under a budget have rules the reference lacks (RFC 0045 12k; engine-core-recovery).
        Plan::Step s{ids[i],rng.below(3)==0,rng.below(5)==0,p.budget==0 && rng.below(4)==0,{},{},-1,{},false};
        if(!rng.below(6))s.contexts={0,1};
        for(uint32_t k=0,m=rng.below(5)?1+rng.below(3):0;k<m;++k)
        {
            Plan::Source src{i && rng.below(2)?int32_t(rng.below(i)):-1,routes++,zones(rng),s.contexts.empty()?int16_t(-1):s.contexts[k%2],!rng.below(10)};
            s.sources.push_back(src);
        }
        p.steps.push_back(s);
    }
    // Waves: steps in order, a wave ending now and then (a step's sources are in earlier steps).
    for(uint32_t i=1;i<n;++i)if(rng.below(2))p.waves.push_back(i);
    p.waves.push_back(n);
    // Lanes: from a step to a later one, plain or through a control.
    for(uint32_t i=0;i+1<n;++i)for(uint32_t k=0,m=rng.below(3);k<m;++k)
    {
        const int32_t target=int32_t(i+1+rng.below(n-i-1));
        oscairn_lane_route r{rng.below(3),target,uint32_t(rng.below(4)),rng.below(2)?0.0:-24.0,rng.below(2)?1.0:24.0,!rng.below(4),-1,int16_t(rng.below(4)?-1:1),-1};
        if(!p.steps[size_t(target)].sources.empty() && rng.below(2))r.notes=int16_t(rng.below(uint32_t(p.steps[size_t(target)].sources.size())));
        if(!rng.below(2))
        {
            std::vector<oscairn_curve> path;
            for(uint32_t c=0,m2=1+rng.below(3);c<m2;++c)path.push_back({uint8_t(rng.below(6)),rng.unit()*.3,.5+rng.unit()*.5,rng.below(2)?0:rng.unit()*2-1,-60+rng.unit()*20,rng.unit()*6-6,.5+rng.unit()*3});
            r.control=int32_t(p.paths.size());p.paths.push_back(path);p.controls.push_back({rng.unit(),!rng.below(3)});
        }
        p.steps[i].lanes.push_back(r);p.steps[size_t(target)].lanesIn=true;
    }
    for(uint32_t h=0,m=rng.below(3);h<m;++h)
    {
        p.harmonies.push_back({routes++,zones(rng,true),rng.below(4)!=0,uint8_t(rng.below(12)),uint16_t(0xAB5|1)});
        for(auto& s:p.steps)if(!rng.below(3))s.harmony=int32_t(h);
    }
    for(uint32_t t=0,m=rng.below(3);t<m;++t)p.taps.push_back({rng.below(2)?-1:int32_t(rng.below(n)),routes++,zones(rng,true),uint64_t(1+rng.below(3)),uint8_t(rng.below(3)?0:1+rng.below(16))});
    return p;
}
// A played change: the same plugins, some connections gone, the taps redone.
Plan change(Rng& rng,Plan p,uint32_t& routes)
{
    for(auto& s:p.steps)if(!s.sources.empty() && !rng.below(3))s.sources.erase(s.sources.begin()+rng.below(uint32_t(s.sources.size())));
    if(!p.steps.empty() && !rng.below(3))p.steps.pop_back(),p.waves.back()=uint32_t(p.steps.size());
    if(p.steps.empty())return randomPlan(rng,routes);
    while(p.waves.size()>1 && p.waves[p.waves.size()-2]>=p.waves.back())p.waves.erase(p.waves.end()-2);
    for(auto& s:p.steps){for(auto& src:s.sources)if(src.from>=int32_t(p.steps.size()))src.from=-1;std::erase_if(s.lanes,[&](const auto& l){return l.target>=int32_t(p.steps.size());});}
    std::erase_if(p.taps,[&](const auto& t){return t.step>=int32_t(p.steps.size()) || !rng.below(3);});
    return p;
}

// A plan's routes, activities, notes, audio and run; the modeled plugins' state.
struct Side
{
    std::vector<oscairn_route*> routes;
    std::array<Activity,hosts*3> activities; // a host's, then its two contexts'
    std::array<oscairn_harmony_t,8> harmonies{};
    struct Built { oscairn_notes* notes=nullptr; oscairn_audio* audio=nullptr; oscairn_run* run=nullptr; };
    Built now, was;
    std::vector<uint64_t> log; // every call on the plugins, hashed with what it carried
    int64_t clock=1'000'000'000;
    uint64_t drops=0; uint32_t nextId=0, midiCount=0; std::array<oscairn_midi,1024> midi{};
    std::vector<std::vector<Event>> sent; // per step, what its plugin sent this block
    std::vector<uint32_t> hostOf;
    std::array<std::vector<float>,2> out{std::vector<float>(frames),std::vector<float>(frames)};
    Rng rng{1}; const Plan* plan=nullptr; uint32_t block=0;
    void free(Built& b) { if(b.run)rust_core_run_free(b.run);if(b.audio)rust_core_audio_free(b.audio);if(b.notes)rust_core_notes_free(b.notes);b={}; }
    ~Side() { free(now);free(was);for(auto* r:routes)rust_core_route_free(r); }
    oscairn_route* route(uint32_t r) { while(routes.size()<=r)routes.push_back(rust_core_route_new(1,uint16_t(routes.size()%3?1:0),routes.size()%2?64:0));return routes[r]; }
    void build(const Plan& p,bool replace)
    {
        std::vector<std::vector<oscairn_source>> sources(p.steps.size());std::vector<std::vector<oscairn_context>> contexts(p.steps.size());
        std::vector<oscairn_step> steps;std::vector<oscairn_audio_step> audio;std::vector<oscairn_audio_route> outs;
        hostOf.clear();
        for(size_t i=0;i<p.steps.size();++i)
        {
            const auto& s=p.steps[i];hostOf.push_back(s.host);
            for(const auto& src:s.sources)sources[i].push_back({route(src.route),&src.zones,nullptr,0,src.channel,src.draining,src.from});
            for(size_t c=0;c<s.contexts.size();++c)contexts[i].push_back({s.contexts[c],activities[s.host*3+1+c].core(),uint32_t(1000+s.host*3+c)});
            steps.push_back({activities[s.host*3].core(),sources[i].data(),uint32_t(sources[i].size()),contexts[i].data(),uint32_t(contexts[i].size()),uint32_t(s.worker?1+s.host%2:0),!s.worker,s.late,nullptr,0,
                             reinterpret_cast<const void*>(uintptr_t(s.host+1)),100+s.host,s.harmony,s.lanes.data(),uint32_t(s.lanes.size()),s.lanesIn,s.host%3!=0});
            audio.push_back({reinterpret_cast<const void*>(uintptr_t(s.host+1)),0,2,2,nullptr,activities[s.host*3].core(),nullptr,0,nullptr,0});
            for(uint32_t c=0;c<2;++c)outs.push_back({int32_t(i),c,c,.5f,{uint8_t(1+i%3),0,0,0}});
        }
        auto budgeted=std::make_unique<bool[]>(steps.size()+1);
        Built b;
        b.notes=rust_core_notes_new(steps.data(),uint32_t(steps.size()),3,budgeted.get());
        b.audio=rust_core_audio_new(audio.data(),uint32_t(audio.size()),outs.data(),uint32_t(outs.size()),0,2,frames);
        std::vector<oscairn_harmony_source> harmonies;
        for(size_t h=0;h<p.harmonies.size();++h)harmonies.push_back({route(p.harmonies[h].route),&p.harmonies[h].zones,p.harmonies[h].input,p.harmonies[h].root,p.harmonies[h].scale,&this->harmonies[h]});
        std::vector<oscairn_tap> taps;
        for(const auto& t:p.taps)taps.push_back({t.step,route(t.route),&t.zones,t.ports,t.channel});
        std::vector<oscairn_control> controls;
        for(size_t c=0;c<p.paths.size();++c)controls.push_back({p.paths[c].data(),uint32_t(p.paths[c].size()),p.controls[c].first,p.controls[c].second});
        b.run=rust_core_run_new(b.notes,b.audio,steps.data(),budgeted.get(),uint32_t(steps.size()),p.waves.data(),uint32_t(p.waves.size()),harmonies.data(),uint32_t(harmonies.size()),
                                taps.data(),uint32_t(taps.size()),controls.data(),uint32_t(controls.size()));
        if(now.run && replace)rust_core_run_adopt(b.run,now.run,true,240,midi.data(),&midiCount,&drops);
        free(was);was=now;now=b; // the plan replaced is kept a block, as Graph retires it
        plan=&p;sent.assign(p.steps.size(),{});
    }
};

// The plugins: what a step's plugin does with the events it is handed, a function of them and the block alone. Every
// draw is its own statement (as core_diff.cpp's events: argument and operand order is unspecified).
struct Model
{
    static Side& side(void* c) { return *static_cast<Side*>(c); }
    static void log(void* c,uint64_t what,uint32_t s,const void* p=nullptr,size_t n=0) { auto& x=side(c);x.log.push_back(hash(p,n,hash(&s,4,what)));x.clock+=1000+int64_t((x.log.size()*7919)%50000)+(x.log.size()%97?0:3'000'000); }
    static Rng rng(void* c,uint32_t s,uint32_t salt) { auto& x=side(c);return Rng{(uint64_t(x.block)*0x9E3779B97F4A7C15ull)^(uint64_t(x.hostOf[s])<<20)^salt^0x5555};}
    static bool play(void* c,uint32_t s,const Event* e,uint32_t n)
    {
        log(c,1,s,e,n*sizeof(Event));
        auto& x=side(c);auto r=rng(c,s,uint32_t(hash(e,n*sizeof(Event))));
        auto& out=x.sent[s];out.clear();
        for(uint32_t i=0;i<n;++i)
        {
            clap_event_note_t k;std::memcpy(&k,e[i].bytes,sizeof k);
            if(k.header.space_id!=CLAP_CORE_EVENT_SPACE_ID || k.header.size!=sizeof k)continue;
            if(k.header.type==CLAP_EVENT_NOTE_ON && out.size()<40){out.push_back(note(CLAP_EVENT_NOTE_ON,k.header.time,k.channel,int16_t(std::min(127,k.key+12)),k.velocity));out.push_back(note(CLAP_EVENT_NOTE_OFF,std::min(frames-1,k.header.time+30),k.channel,int16_t(std::min(127,k.key+12)),0));}
            if((k.header.type==CLAP_EVENT_NOTE_OFF || k.header.type==CLAP_EVENT_NOTE_CHOKE) && k.note_id>=0 && r.below(3)){auto end=note(CLAP_EVENT_NOTE_END,0,k.channel,k.key,0,k.note_id);out.push_back(end);}
        }
        for(uint32_t i=0,m=r.below(40)?r.below(4):600;i<m;++i)
        {
            const uint32_t time=r.below(frames),which=r.below(3);const bool set=r.below(2);const double u=r.unit(),scale=r.below(4)?2:6,v=u*scale-(r.below(4)?1:3);
            const int32_t id=r.below(4)?-1:int32_t(r.below(30));const int16_t ch=int16_t(r.below(4)),key=int16_t(r.below(4)?-1:60);
            out.push_back(lane(time,which,set,v,id,ch,key));
        }
        if(!r.below(5)){const uint32_t time=r.below(frames);const uint8_t status=uint8_t(0xB0|r.below(16)),a=uint8_t(r.below(128)),b=uint8_t(r.below(128));out.push_back(midi(time,status,a,b));}
        if(!r.below(7)){const uint32_t time=r.below(frames);const int16_t ch=int16_t(r.below(16)),key=int16_t(36+r.below(40));const double v=r.unit();out.push_back(note(CLAP_EVENT_NOTE_ON,time,ch,key,v));}
        auto* o=rust_core_audio_buffers(x.plan==nullptr?nullptr:x.now.audio,int32_t(s),true);
        for(uint32_t ch=0;ch<2;++ch)for(uint32_t f=0;f<frames;++f){const float sample=float(r.unit()-.5);o[ch][f]=sample*(r.below(9)?1.f:0.f);}
        return !side(c).plan->lossy || r.below(10);
    }
};

const oscairn_plugins plugins{nullptr,
    [](void* c,uint32_t s,uint32_t* n)->const Event*{auto& x=Model::side(c);Model::log(c,2,s);*n=uint32_t(x.sent[s].size());return x.sent[s].data();},
    [](void* c,uint32_t s,const Event* e,uint32_t n){const bool worker=Model::side(c).plan->steps[s].worker;Model::log(c,3,s);return worker && Model::play(c,s,e,n),worker;},
    [](void* c,uint32_t s,const Event* e,uint32_t n){return Model::play(c,s,e,n);},
    [](void* c,uint32_t s){Model::log(c,4,s);return !Model::side(c).plan->lossy || Model::rng(c,s,4).below(8)!=0;},
    [](void* c,uint32_t s){Model::log(c,5,s);return Model::side(c).plan->steps[s].late;},
    [](void* c,uint32_t s){Model::log(c,6,s);},
    [](void* c,uint32_t s){Model::log(c,7,s);return Model::rng(c,s,7).below(2)!=0;},
    [](void* c,uint32_t s){Model::log(c,8,s);return Model::side(c).plan->lossy && !Model::rng(c,s,8).below(20);},
    [](void* c,uint32_t s){Model::log(c,9,s);return !Model::rng(c,s,9).below(3);},
    [](void* c,uint32_t s){Model::log(c,10,s);return Model::side(c).plan->steps[s].ownVoices;},
    [](void* c,uint32_t s){Model::log(c,11,s);return Model::rng(c,s,11).below(2)!=0;},
    [](void* c,uint32_t s){Model::log(c,12,s);return uint64_t(0);}, // no missed blocks (the Rust core's guard alone reads them)
    [](void* c,uint32_t s){Model::log(c,13,s);return 5.0+Model::rng(c,s,13).below(55);}, // under the deadline on every lane: its rule is the Rust core's alone,
    [](void* c,uint32_t s,double* cost,const oscairn_voice_budget_note_t** notes,uint32_t* n){
        Model::log(c,14,s);static thread_local oscairn_voice_budget_note_t list[4];auto r=Model::rng(c,s,14);
        for(uint32_t i=0;i<4;++i)list[i]={int32_t(r.below(30))-1,0,int16_t(r.below(2)),int16_t(40+r.below(30)),1+r.below(3),float(r.unit())};
        *cost=10+r.below(40);*notes=list;*n=r.below(5);return r.below(20);},
    [](void* c,uint32_t s,const oscairn_voice_budget_importance_t** lists){ // draft 2's report (RFC 0046): a list per context, by sound ID 7 and up
        Model::log(c,24,s);static thread_local oscairn_voice_budget_importance_t list[2];auto r=Model::rng(c,s,24);
        const uint32_t n=r.below(3);
        for(uint32_t k=0;k<n;++k)
        {
            list[k]={};list[k].context=7+k;
            const uint32_t voices=r.below(20);
            const uint32_t listed=r.below(OSCAIRN_VOICE_BUDGET_LEAST+1);
            list[k].voices=uint16_t(voices);list[k].count=uint16_t(std::min(listed,voices));
            for(uint32_t i=0;i<list[k].count;++i){const uint32_t tier=r.below(4);const uint32_t age=r.below(3000);const uint32_t level=r.below(120);list[k].least[i]={uint8_t(tier),0,uint16_t(age),-float(level)};}
        }
        *lists=list;return n;},
    [](void* c,uint32_t s,uint32_t limit,const oscairn_voice_budget_context_t* p,uint32_t n){Model::log(c,15,s,p,n*sizeof *p);Model::log(c,15,s,&limit,4);},
    [](void* c,uint32_t s,uint32_t k){Model::log(c,16,s,&k,4);return 7+k;},
    [](void* c,uint32_t s,uint32_t k,oscairn_emit emit,void* e){Model::log(c,17,s,&k,4);auto r=Model::rng(c,s,17+k);for(uint32_t i=0,m=r.below(3);i<m;++i){const uint32_t id=r.below(5);const double v=r.unit();const auto p=param(0,id,v);emit(e,&p);}},
    [](void* c,uint32_t node,const Event* e,uint32_t n,bool sent){Model::log(c,18+sent,node,e,n*sizeof(Event));},
    [](void* c){return Model::side(c).clock;},
    [](void* c,int64_t at,bool only){Model::log(c,20+only,0,&at,8);},
    [](void* c,uint32_t n,oscairn_job job,void* context){Model::log(c,22,n);for(uint32_t i=0;i<n;++i)job(context,i);},
    [](void* c,uint32_t s,uint32_t frames){if(Model::side(c).plan->steps[s].host%3==0){std::fprintf(stderr,"FAIL: rest asked of step %u, which may not rest (no rest check, no input scan)\n",s);std::abort();}Model::log(c,23,s,&frames,4);const bool slept=Model::rng(c,s,23).below(3)==0;if(slept)Model::side(c).sent[s].clear();return slept;}}; // a host taking the block sends nothing

void input(Rng& rng,std::vector<Event>& out,std::vector<uint8_t>& tags)
{
    out.clear();tags.clear();
    const bool flood=!rng.below(50); // more harmony changes than a block holds
    for(uint32_t i=0,n=flood?40:rng.below(6);i<n;++i)
    {
        if(flood){out.push_back(note(CLAP_EVENT_NOTE_ON,i*3,0,int16_t(30+i),.5));tags.push_back(0xff);continue;}
        const uint32_t t=rng.below(rng.below(20)?frames:frames+8); // now and then past the block (passed on as is)
        const int16_t c=int16_t(rng.below(3)),k=int16_t(36+rng.below(30));
        switch(rng.below(8))
        {
            case 0: case 1: case 2: {const double v=rng.unit();const int32_t id=rng.below(3)?-1:int32_t(rng.below(20));out.push_back(note(CLAP_EVENT_NOTE_ON,t,c,k,v,id));}break;
            case 3: case 4: out.push_back(note(CLAP_EVENT_NOTE_OFF,t,c,k,0));break;
            case 5: out.push_back(midi(t,uint8_t(0xB0|c),64,uint8_t(rng.below(2)?0:127)));break;
            case 6: out.push_back(midi(t,uint8_t(0xD0|c),uint8_t(rng.below(128)),0));break;
            default: out.push_back(midi(t,uint8_t(0x90|c),uint8_t(k),uint8_t(rng.below(128))));break;
        }
        tags.push_back(uint8_t(rng.below(3)?rng.below(2):0xff));
    }
}

struct Totals { uint64_t plans=0,blocks=0,replaced=0,calls=0,midi=0; } totals;

void run(uint32_t seed,uint32_t blocks,vectors::Tape& tape)
{
    tape.plan(seed,blocks);
    Rng rng{seed*0x9E3779B97F4A7C15ull+7};
    uint32_t routes=0;
    std::vector<std::unique_ptr<Plan>> plans;plans.push_back(std::make_unique<Plan>(randomPlan(rng,routes)));
    auto side=std::make_unique<Side>();
    side->build(*plans.back(),false);
    std::vector<Event> events;std::vector<uint8_t> tags;
    std::array<float,256> live;live.fill(1.f);
    for(uint32_t b=0;b<blocks;++b)
    {
        if(b && !rng.below(200))
        {
            const bool played=rng.below(3)!=0;
            plans.push_back(std::make_unique<Plan>(played?change(rng,*plans.back(),routes):randomPlan(rng,routes)));
            side->build(*plans.back(),true);
            ++totals.replaced;
        }
        input(rng,events,tags);
        Event beat{};beat.bytes[0]=40;
        std::array<oscairn_param,3> params{};uint32_t paramCount=rng.below(4);
        for(uint32_t i=0;i<paramCount;++i){const auto& st=plans.back()->steps[rng.below(uint32_t(plans.back()->steps.size()))];const uint32_t node=rng.below(3)?100+st.host:1000+st.host*3+rng.below(2),id=rng.below(4);const double v=rng.unit();params[i]={node,param(0,id,v)};}
        for(uint32_t slot=1;slot<4;++slot)if(!rng.below(5))live[slot]=float(rng.unit());
        const auto& p=*plans.back();
        const bool panicInput=!rng.below(150),panicNotes=!rng.below(300),listening=rng.below(2),parallel=rng.below(4)!=0,hasDeadline=rng.below(2),deadlines=rng.below(2),transportWake=!rng.below(8);
        const float worth=rng.below(2)?0.f:30.f;
        const oscairn_block timing{p.budget,2666.67f,2266.67f,rng.below(2)?1333.33f:1000.f,1+float(rng.below(3)),uint64_t(rng.below(3)?96000:256),0};
        {
            auto& s=side;
            s->block=b;
            float* outs[2]{s->out[0].data(),s->out[1].data()};
            // The callback's deadline a second away: its slack never runs low (the Rust core alone acts on that).
            const oscairn_run_block run{nullptr,0,outs,2,frames,48,events.data(),uint32_t(events.size()),tags.data(),params.data(),paramCount,&beat,panicInput,panicNotes,listening,parallel,deadlines,transportWake,
                                        timing,hasDeadline?s->clock+1'000'000'000:0,worth,live.data(),live.data(),&s->nextId,&s->drops,s->midi.data(),&s->midiCount};
            oscairn_plugins calls=plugins;calls.context=s.get();
            s->log.push_back(rust_core_run_block(s->now.run,&run,&calls));
        }
        auto& x=*side;
        for(size_t i=0;i<x.log.size();++i)tape.see(x.log[i]);
        totals.calls+=x.log.size();x.log.clear();
        for(const auto& o:x.out)tape.see(o.data(),o.size()*sizeof(float));
        tape.see(x.midiCount);tape.see(x.midi.data(),x.midiCount*sizeof(oscairn_midi));
        totals.midi+=x.midiCount;x.midiCount=0;
        tape.see(x.drops);
        tape.see(x.nextId);
        tape.see(x.harmonies);
        for(size_t i=0;i<x.activities.size();++i)tape.see(x.activities[i].bytes);
        for(size_t r=0;r<x.routes.size();++r)
        {
            uint8_t pedals[256];uint64_t held[2];
            rust_core_route_pedals(x.routes[r],pedals);rust_core_route_held_keys(x.routes[r],held);
            tape.see(rust_core_route_live(x.routes[r]));tape.see(pedals);tape.see(held);
        }
        tape.block();
        ++totals.blocks;
    }
    ++totals.plans;
}
}

int main(int argc,char** argv)
{
    vectors::Args args(argc,argv,150,1500,"engine-run-diff","core/engine/tests/run_diff.cpp (oscairn-run-diff-test record)");
    for(uint32_t p=0;p<args.plans;++p)run(p,args.blocks,args.tape);
    const char* outcome=args.done(7); // a curve's shapes: exp, log1p, pow
    std::printf("engine run differential: %llu plans (%llu replaced), %llu blocks, %llu plugin calls, %llu MIDI messages: %s\n",
                (unsigned long long)totals.plans,(unsigned long long)totals.replaced,(unsigned long long)totals.blocks,(unsigned long long)totals.calls,(unsigned long long)totals.midi,outcome);
    return 0;
}
