// SPDX-License-Identifier: AGPL-3.0-or-later
// The differential test of a plan's block (engine_core.h's run_*): the Rust implementation against its
// C++ reference (Graph::process and beginBlock as they were), each over its own notes, audio and routes,
// driven by the same random plans and the same modeled plugins, and compared after every block: every
// call either makes on the plugins (in order, with what it hands them: the merged events, voice
// limits, listener events, deadlines), the graph's audio out, MIDI out, event drops, note IDs, every
// route's state, every activity and the parts' harmonies. Plans are replaced under it (retired
// connections, stranded plugins, taps gone, gains carried). The plugins are modeled, the same for both:
// what each sends is a function of what it was handed (notes on and off as an arpeggiator's, NOTE_ENDs,
// parameter-target lane events, MIDI), blocks lost, late, reset, a clock that advances with each call.
// The Rust core's guard rules the reference lacks get no input (as engine-core-diff): no missed blocks,
// no audio links, the callback's slack never low.
//   oscairn-run-diff-test [plans] [blocks]
#include "../src/engine_core.h"
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

struct Api
{
    const char* name;
    decltype(&rust_core_route_new) routeNew; decltype(&rust_core_route_free) routeFree; decltype(&rust_core_route_live) routeLive;
    decltype(&rust_core_route_pedals) routePedals; decltype(&rust_core_route_held_keys) heldKeys;
    decltype(&rust_core_notes_new) notesNew; decltype(&rust_core_notes_free) notesFree;
    decltype(&rust_core_audio_new) audioNew; decltype(&rust_core_audio_free) audioFree; decltype(&rust_core_audio_buffers) buffers;
    decltype(&rust_core_run_new) runNew; decltype(&rust_core_run_free) runFree; decltype(&rust_core_run_adopt) adopt; decltype(&rust_core_run_block) block;
};
#define OSCAIRN_RUN_TABLE(p) Api{#p,p##route_new,p##route_free,p##route_live,p##route_pedals,p##route_held_keys,p##notes_new,p##notes_free,\
    p##audio_new,p##audio_free,p##audio_buffers,p##run_new,p##run_free,p##run_adopt,p##run_block}
const Api apis[2]{OSCAIRN_RUN_TABLE(cpp_core_),OSCAIRN_RUN_TABLE(rust_core_)};

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

// A plan, the same for both sides.
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

// One implementation's side: its routes, activities, notes, audio and run; the modeled plugins' state.
struct Side
{
    const Api& api;
    std::vector<oscairn_route*> routes;
    std::array<Activity,hosts*3> activities; // a host's, then its two contexts'
    std::array<oscairn_harmony_t,8> harmonies{};
    struct Built { oscairn_notes* notes=nullptr; oscairn_audio* audio=nullptr; oscairn_run* run=nullptr; };
    Built now, was;
    std::vector<uint64_t> log; // every call on the plugins, hashed with what it carried
    std::vector<uint64_t> trace; // each call's kind and step, to say where two logs part
    std::vector<std::vector<Event>> handed; // each call's events, for the report
    int64_t clock=1'000'000'000;
    uint64_t drops=0; uint32_t nextId=0, midiCount=0; std::array<oscairn_midi,1024> midi{};
    std::vector<std::vector<Event>> sent; // per step, what its plugin sent this block
    std::vector<uint32_t> hostOf;
    std::array<std::vector<float>,2> out{std::vector<float>(frames),std::vector<float>(frames)};
    Rng rng{1}; const Plan* plan=nullptr; uint32_t block=0;
    explicit Side(const Api& a):api(a) {}
    void free(Built& b) { if(b.run)api.runFree(b.run);if(b.audio)api.audioFree(b.audio);if(b.notes)api.notesFree(b.notes);b={}; }
    ~Side() { free(now);free(was);for(auto* r:routes)api.routeFree(r); }
    oscairn_route* route(uint32_t r) { while(routes.size()<=r)routes.push_back(api.routeNew(1,uint16_t(routes.size()%3?1:0),routes.size()%2?64:0));return routes[r]; }
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
        b.notes=api.notesNew(steps.data(),uint32_t(steps.size()),3,budgeted.get());
        b.audio=api.audioNew(audio.data(),uint32_t(audio.size()),outs.data(),uint32_t(outs.size()),0,2,frames);
        std::vector<oscairn_harmony_source> harmonies;
        for(size_t h=0;h<p.harmonies.size();++h)harmonies.push_back({route(p.harmonies[h].route),&p.harmonies[h].zones,p.harmonies[h].input,p.harmonies[h].root,p.harmonies[h].scale,&this->harmonies[h]});
        std::vector<oscairn_tap> taps;
        for(const auto& t:p.taps)taps.push_back({t.step,route(t.route),&t.zones,t.ports,t.channel});
        std::vector<oscairn_control> controls;
        for(size_t c=0;c<p.paths.size();++c)controls.push_back({p.paths[c].data(),uint32_t(p.paths[c].size()),p.controls[c].first,p.controls[c].second});
        b.run=api.runNew(b.notes,b.audio,steps.data(),budgeted.get(),uint32_t(steps.size()),p.waves.data(),uint32_t(p.waves.size()),harmonies.data(),uint32_t(harmonies.size()),
                         taps.data(),uint32_t(taps.size()),controls.data(),uint32_t(controls.size()));
        if(now.run && replace)api.adopt(b.run,now.run,true,240,midi.data(),&midiCount,&drops);
        free(was);was=now;now=b; // the plan replaced is kept a block, as Graph retires it
        plan=&p;sent.assign(p.steps.size(),{});
    }
};

// The plugins: what a step's plugin does with the events it is handed, a function of them and the block alone.
struct Model
{
    static Side& side(void* c) { return *static_cast<Side*>(c); }
    static void log(void* c,uint64_t what,uint32_t s,const void* p=nullptr,size_t n=0) { auto& x=side(c);x.log.push_back(hash(p,n,hash(&s,4,what)));x.trace.push_back(what<<32|s);x.handed.emplace_back();if(what==1 || what==18 || what==19)x.handed.back().assign(static_cast<const Event*>(p),static_cast<const Event*>(p)+n/sizeof(Event));x.clock+=1000+int64_t((x.log.size()*7919)%50000)+(x.log.size()%97?0:3'000'000); }
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
        for(uint32_t i=0,m=r.below(40)?r.below(4):600;i<m;++i)out.push_back(lane(r.below(frames),r.below(3),r.below(2),r.unit()*(r.below(4)?2:6)-(r.below(4)?1:3),r.below(4)?-1:int32_t(r.below(30)),int16_t(r.below(4)),int16_t(r.below(4)?-1:60)));
        if(!r.below(5))out.push_back(midi(r.below(frames),uint8_t(0xB0|r.below(16)),uint8_t(r.below(128)),uint8_t(r.below(128))));
        if(!r.below(7))out.push_back(note(CLAP_EVENT_NOTE_ON,r.below(frames),int16_t(r.below(16)),int16_t(36+r.below(40)),r.unit()));
        auto* o=x.api.buffers(x.plan==nullptr?nullptr:x.now.audio,int32_t(s),true);
        for(uint32_t ch=0;ch<2;++ch)for(uint32_t f=0;f<frames;++f)o[ch][f]=float(r.unit()-.5)*(r.below(9)?1.f:0.f);
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
    [](void* c,uint32_t s,uint32_t limit,const oscairn_voice_budget_context_t* p,uint32_t n){Model::log(c,15,s,p,n*sizeof *p);Model::log(c,15,s,&limit,4);},
    [](void* c,uint32_t s,uint32_t k){Model::log(c,16,s,&k,4);return 7+k;},
    [](void* c,uint32_t s,uint32_t k,oscairn_emit emit,void* e){Model::log(c,17,s,&k,4);auto r=Model::rng(c,s,17+k);for(uint32_t i=0,m=r.below(3);i<m;++i){const auto p=param(0,r.below(5),r.unit());emit(e,&p);}},
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
            case 0: case 1: case 2: out.push_back(note(CLAP_EVENT_NOTE_ON,t,c,k,rng.unit(),rng.below(3)?-1:int32_t(rng.below(20))));break;
            case 3: case 4: out.push_back(note(CLAP_EVENT_NOTE_OFF,t,c,k,0));break;
            case 5: out.push_back(midi(t,uint8_t(0xB0|c),64,uint8_t(rng.below(2)?0:127)));break;
            case 6: out.push_back(midi(t,uint8_t(0xD0|c),uint8_t(rng.below(128)),0));break;
            default: out.push_back(midi(t,uint8_t(0x90|c),uint8_t(k),uint8_t(rng.below(128))));break;
        }
        tags.push_back(uint8_t(rng.below(3)?rng.below(2):0xff));
    }
}

[[noreturn]] void fail(const char* what,uint64_t seed,uint32_t block,size_t at=0)
{
    std::printf("MISMATCH rust_core_ against cpp_core_: %s, plan %llu, block %u (%zu)\n",what,(unsigned long long)seed,block,at);std::exit(1);
}

struct Totals { uint64_t plans=0,blocks=0,replaced=0,calls=0,midi=0; } totals;

void run(uint64_t seed,uint32_t blocks)
{
    Rng rng{seed*0x9E3779B97F4A7C15ull+7};
    uint32_t routes=0;
    std::vector<std::unique_ptr<Plan>> plans;plans.push_back(std::make_unique<Plan>(randomPlan(rng,routes)));
    std::unique_ptr<Side> sides[2]{std::make_unique<Side>(apis[0]),std::make_unique<Side>(apis[1])};
    for(auto& s:sides)s->build(*plans.back(),false);
    std::vector<Event> events;std::vector<uint8_t> tags;
    std::array<float,256> live;live.fill(1.f);
    for(uint32_t b=0;b<blocks;++b)
    {
        if(b && !rng.below(200))
        {
            const bool played=rng.below(3)!=0;
            plans.push_back(std::make_unique<Plan>(played?change(rng,*plans.back(),routes):randomPlan(rng,routes)));
            for(auto& s:sides)s->build(*plans.back(),true);
            ++totals.replaced;
        }
        input(rng,events,tags);
        Event beat{};beat.bytes[0]=40;
        std::array<oscairn_param,3> params{};uint32_t paramCount=rng.below(4);
        for(uint32_t i=0;i<paramCount;++i){const auto& st=plans.back()->steps[rng.below(uint32_t(plans.back()->steps.size()))];params[i]={rng.below(3)?100+st.host:1000+st.host*3+rng.below(2),param(0,rng.below(4),rng.unit())};}
        for(uint32_t slot=1;slot<4;++slot)if(!rng.below(5))live[slot]=float(rng.unit());
        const auto& p=*plans.back();
        const bool panicInput=!rng.below(150),panicNotes=!rng.below(300),listening=rng.below(2),parallel=rng.below(4)!=0,hasDeadline=rng.below(2),deadlines=rng.below(2),transportWake=!rng.below(8);
        const float worth=rng.below(2)?0.f:30.f;
        const oscairn_block timing{p.budget,2666.67f,2266.67f,rng.below(2)?1333.33f:1000.f,1+float(rng.below(3)),uint64_t(rng.below(3)?96000:256),0};
        for(auto& s:sides)
        {
            s->block=b;
            float* outs[2]{s->out[0].data(),s->out[1].data()};
            // The callback's deadline a second away: its slack never runs low (the Rust core alone acts on that).
            const oscairn_run_block run{nullptr,0,outs,2,frames,48,events.data(),uint32_t(events.size()),tags.data(),params.data(),paramCount,&beat,panicInput,panicNotes,listening,parallel,deadlines,transportWake,
                                        timing,hasDeadline?s->clock+1'000'000'000:0,worth,live.data(),live.data(),&s->nextId,&s->drops,s->midi.data(),&s->midiCount};
            oscairn_plugins calls=plugins;calls.context=s.get();
            s->log.push_back(s->api.block(s->now.run,&run,&calls));
        }
        auto& x=*sides[0];auto& y=*sides[1];
        if(x.log.size()!=y.log.size() || x.log!=y.log)
        {
            size_t at=0;while(at<x.log.size() && at<y.log.size() && x.log[at]==y.log[at])++at;
            for(size_t i=at>3?at-3:0;i<at+3 && i<x.trace.size() && i<y.trace.size();++i)std::printf("  call %zu: cpp %llu/%llu rust %llu/%llu\n",i,(unsigned long long)(x.trace[i]>>32),(unsigned long long)(x.trace[i]&0xffffffff),(unsigned long long)(y.trace[i]>>32),(unsigned long long)(y.trace[i]&0xffffffff));
            for(int k=0;k<2;++k){auto& z=k?y:x;std::printf(" %s:",k?"rust":"cpp");for(auto& e:z.handed[at]){clap_event_header_t h;std::memcpy(&h,e.bytes,sizeof h);std::printf(" [%u %u t%u",h.space_id,h.type,h.time);if(h.space_id==0 && h.type<=3){clap_event_note_t n;std::memcpy(&n,e.bytes,sizeof n);std::printf(" id%d c%d k%d",n.note_id,n.channel,n.key);}if(h.type==5||h.type==6){clap_event_param_value_t q;std::memcpy(&q,e.bytes,sizeof q);std::printf(" p%u c%d v%g",q.param_id,q.channel,q.value);}std::printf("]");}std::printf("\n");}
            std::printf(" budget %g lossy %d steps %zu\n",double(p.budget),int(p.lossy),p.steps.size());fail("calls on the plugins",seed,b,at);
        }
        totals.calls+=x.log.size();x.log.clear();y.log.clear();x.trace.clear();y.trace.clear();x.handed.clear();y.handed.clear();
        if(x.out!=y.out)fail("audio out",seed,b);
        if(x.midiCount!=y.midiCount || std::memcmp(x.midi.data(),y.midi.data(),x.midiCount*sizeof(oscairn_midi)))fail("MIDI out",seed,b);
        totals.midi+=x.midiCount;x.midiCount=y.midiCount=0;
        if(x.drops!=y.drops)fail("event drops",seed,b);
        if(x.nextId!=y.nextId)fail("note IDs",seed,b);
        if(std::memcmp(x.harmonies.data(),y.harmonies.data(),sizeof x.harmonies))fail("harmonies",seed,b);
        for(size_t i=0;i<x.activities.size();++i)if(std::memcmp(x.activities[i].bytes,y.activities[i].bytes,sizeof(Activity)))fail("activity",seed,b,i);
        for(size_t r=0;r<x.routes.size();++r)
        {
            uint8_t pa[256],pb[256];uint64_t ha[2],hb[2];
            x.api.routePedals(x.routes[r],pa);y.api.routePedals(y.routes[r],pb);x.api.heldKeys(x.routes[r],ha);y.api.heldKeys(y.routes[r],hb);
            if(x.api.routeLive(x.routes[r])!=y.api.routeLive(y.routes[r]) || std::memcmp(pa,pb,256) || std::memcmp(ha,hb,16))fail("route state",seed,b,r);
        }
        ++totals.blocks;
    }
    ++totals.plans;
}
}

int main(int argc,char** argv)
{
    const uint32_t plans=argc>1?uint32_t(std::atoi(argv[1])):150,blocks=argc>2?uint32_t(std::atoi(argv[2])):1500;
    for(uint32_t p=0;p<plans;++p)run(p,blocks);
    std::printf("engine run differential: %llu plans (%llu replaced), %llu blocks, %llu plugin calls, %llu MIDI messages: rust_core_ and cpp_core_ identical\n",
                (unsigned long long)totals.plans,(unsigned long long)totals.replaced,(unsigned long long)totals.blocks,(unsigned long long)totals.calls,(unsigned long long)totals.midi);
    return 0;
}
