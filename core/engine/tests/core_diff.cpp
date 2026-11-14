// SPDX-License-Identifier: AGPL-3.0-or-later
// The differential test of the engine core (engine_core.h): the Rust implementation against its C++
// reference, driven through the same C interface by the same random plans and event streams, and
// every result compared bit for bit: the events each step is sent, its overflow, each route's notes,
// pedals and held keys, every activity counter, the voice limits, what taps and harmonies route,
// what a plan change's retired routes send, and the pedals each part takes by its receive setting. The plugins' side is modeled here, the same for
// both: block times from the notes each holds, NOTE_ENDs after a tail (one in 13 never), lost and
// reset blocks, silence for the note-end watchdog, a plugin that gives up its own voices. The Rust core's rules the C++
// reference does not have (RFC 0045's guard past 12a: lost blocks, the chain after a step, the callback's slack) get
// no input here: no block reported lost, no audio links, no slack; the guard's own test (guard.cpp) covers them.
//   oscairn-core-diff-test [plans] [blocks]
#include "../src/engine_core.h"
#include "oscairn/graph.h"
#include <clap/clap.h>
#include <algorithm>
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
constexpr uint32_t frames=128,capacity=kick::clap::Host::maxProcessEvents;

struct Core // one implementation's entry points
{
    const char* name;
    decltype(&rust_core_route_new) routeNew; decltype(&rust_core_route_free) routeFree; decltype(&rust_core_route_live) routeLive;
    decltype(&rust_core_route_pedals) routePedals; decltype(&rust_core_route_held_keys) routeHeldKeys; decltype(&rust_core_route_retire) routeRetire;
    decltype(&rust_core_route_let_go) routeLetGo; decltype(&rust_core_route_events) routeEvents; decltype(&rust_core_notes_new) notesNew;
    decltype(&rust_core_notes_free) notesFree; decltype(&rust_core_notes_begin) notesBegin; decltype(&rust_core_notes_route) notesRoute;
    decltype(&rust_core_notes_finish) notesFinish; decltype(&rust_core_notes_voice_limit) notesVoiceLimit; decltype(&rust_core_notes_end) notesEnd;
};
#define OSCAIRN_CORE_TABLE(p) Core{#p,p##route_new,p##route_free,p##route_live,p##route_pedals,p##route_held_keys,p##route_retire,p##route_let_go,\
    p##route_events,p##notes_new,p##notes_free,p##notes_begin,p##notes_route,p##notes_finish,p##notes_voice_limit,p##notes_end}
const Core cores[2]{OSCAIRN_CORE_TABLE(cpp_core_),OSCAIRN_CORE_TABLE(rust_core_)};

struct Rng
{
    uint64_t s;
    uint64_t next() { s^=s<<13;s^=s>>7;s^=s<<17;return s; }
    uint32_t below(uint32_t n) { return uint32_t(next()%n); }
    double unit() { return double(next()>>11)/double(1ull<<53); }
};
template<class T> Event from(const T& x) { Event e{};std::memcpy(e.bytes,&x,sizeof x);return e; }
Event note(uint16_t type,uint32_t time,int16_t channel,int16_t key,double velocity,int32_t id=-1,int16_t port=0)
{
    clap_event_note_t n{};n.header={sizeof n,time,CLAP_CORE_EVENT_SPACE_ID,type,0};
    n.note_id=id;n.port_index=port;n.channel=channel;n.key=key;n.velocity=velocity;return from(n);
}
Event midi(uint32_t time,uint8_t status,uint8_t a,uint8_t b,uint16_t port=0)
{
    clap_event_midi_t m{};m.header={sizeof m,time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};
    m.port_index=port;m.data[0]=status;m.data[1]=a;m.data[2]=b;return from(m);
}
Event expression(uint32_t time,int32_t id,int16_t channel,int16_t key,int32_t which,double value)
{
    clap_event_note_expression_t x{};x.header={sizeof x,time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_EXPRESSION,0};
    x.expression_id=which;x.note_id=id;x.port_index=0;x.channel=channel;x.key=key;x.value=value;return from(x);
}
Event parameter(uint32_t time,int32_t id,int16_t channel,int16_t key,bool modulation)
{
    clap_event_param_value_t v{};v.header={sizeof v,time,CLAP_CORE_EVENT_SPACE_ID,uint16_t(modulation?CLAP_EVENT_PARAM_MOD:CLAP_EVENT_PARAM_VALUE),0};
    v.param_id=7;v.note_id=id;v.port_index=0;v.channel=channel;v.key=key;v.value=.25;return from(v);
}

// Random events, small key and channel ranges so notes collide; floods that fill the 512 slots.
void events(Rng& rng,uint32_t block,std::vector<Event>& out,std::vector<uint8_t>& tags)
{
    const bool flood=block%997<12;
    const uint32_t n=flood?48:rng.below(7);
    for(uint32_t i=0;i<n;++i)
    {
        const uint32_t t=rng.below(rng.below(20)?frames:frames+8); // now and then past the block (passed on as is)
        const int16_t c=int16_t(rng.below(4)),k=int16_t(36+rng.below(flood?30:14));
        const int32_t id=rng.below(3)?-1:int32_t(rng.below(20));
        switch(flood?0:rng.below(19))
        {
            case 0: case 1: case 2: case 3: out.push_back(note(CLAP_EVENT_NOTE_ON,t,c,k,rng.below(8)?rng.unit():1.0,id)); break;
            case 4: case 5: case 6: out.push_back(note(CLAP_EVENT_NOTE_OFF,t,rng.below(8)?c:-1,rng.below(8)?k:-1,rng.unit(),id)); break;
            case 7: out.push_back(note(CLAP_EVENT_NOTE_CHOKE,t,c,rng.below(4)?k:-1,0,id)); break;
            case 8: out.push_back(expression(t,id,rng.below(6)?c:-1,rng.below(6)?k:-1,int32_t(rng.below(3)?0:rng.below(7)),rng.unit()*2)); break;
            case 9: case 10: out.push_back(midi(t,uint8_t(0x90|c),uint8_t(k),uint8_t(rng.below(4)?1+rng.below(127):0))); break;
            case 11: out.push_back(midi(t,uint8_t(0x80|c),uint8_t(k),uint8_t(rng.below(128)))); break;
            case 12: out.push_back(midi(t,uint8_t(0xb0|c),(const uint8_t[]){64,64,66,67}[rng.below(4)],(const uint8_t[]){0,37,63,64,127}[rng.below(5)])); break; // the pedals
            case 13: out.push_back(midi(t,uint8_t(0xb0|c),uint8_t(rng.below(2)?120:123),0)); break;
            case 14: out.push_back(rng.below(2)?note(CLAP_EVENT_NOTE_ON,t,c,k,2.0):note(CLAP_EVENT_NOTE_ON,t,c,k,.5,-1,1)); break; // invalid; another port
            case 15: out.push_back(midi(t,uint8_t(0xd0|c),uint8_t(rng.below(128)),0)); break; // channel pressure
            case 16: out.push_back(midi(t,uint8_t(0xa0|c),uint8_t(k),uint8_t(rng.below(128)))); break; // key pressure
            case 17: out.push_back(parameter(t,rng.below(2)?id:-1,rng.below(3)?c:-1,rng.below(2)?k:-1,rng.below(2))); break;
            default: out.push_back(midi(t,uint8_t(0xb0|c),1,uint8_t(rng.below(128)))); break; // a controller: passed on
        }
        tags.push_back(uint8_t(rng.below(4)?rng.below(3):0xff));
    }
}

// An activity at a fixed address, its padding zeroed, so two can be compared byte for byte.
struct Activity
{
    alignas(oscairn::Graph::Activity) unsigned char bytes[sizeof(oscairn::Graph::Activity)]{};
    Activity(float seedRest,float seedNote) { auto* a=new(bytes) oscairn::Graph::Activity;a->seedRest=seedRest;a->seedNote=seedNote; }
    oscairn::Graph::Activity& get() { return *std::launder(reinterpret_cast<oscairn::Graph::Activity*>(bytes)); }
    oscairn_activity* core() { return reinterpret_cast<oscairn_activity*>(bytes); }
};

// One implementation's side of a plan: its routes, activities and notes; the plugins' side kept per side.
struct Side
{
    const Core& core;
    std::vector<oscairn_route*> routes; // per source, then the tap's and the harmony's
    std::vector<std::unique_ptr<Activity>> activities; // per step, then per context
    oscairn_notes* notes=nullptr;
    std::vector<bool> budgeted;
    struct End { uint32_t due; Event event; };
    std::vector<std::vector<End>> pending; // per step
    uint32_t nextId=0,released=0;
    explicit Side(const Core& c):core(c) {}
    ~Side() { core.notesFree(notes);for(auto* r:routes)core.routeFree(r); }
};

struct Source { uint32_t route; oscairn_zones zones{}; int16_t channel=-1; bool draining=false; std::vector<uint32_t> peers; };
struct StepPlan { uint32_t lane=0; bool inProcess=false,lateLane=false,ownVoices=false,honors=false; std::vector<Source> sources; std::vector<int16_t> contexts; float rest=0,perNote=0,seedRest=0; };

oscairn_zone zone(Rng& rng)
{
    oscairn_zone z{};z.low_key=uint8_t(36+rng.below(8));z.high_key=uint8_t(44+rng.below(20));z.low_velocity=rng.below(4)?0:.2;z.high_velocity=1;
    z.channels=uint16_t(rng.below(2)?0xFFFF:1+rng.below(15));z.transpose=int8_t(int(rng.below(5))-2);z.curve=rng.below(3)?1:2;z.fixed_velocity=rng.below(6)?0:.6;
    z.sources=rng.below(8)?~0ull:0;z.max_notes=uint16_t(rng.below(3)?0:1+rng.below(6));z.part=rng.below(3);z.reserve=uint16_t(rng.below(4));z.priority=uint8_t(rng.below(11));
    z.pedals=uint8_t(rng.below(2)?0:rng.below(64)); // each pedal's receive: all, none or own (3 also reads as all)
    return z;
}

[[noreturn]] void fail(const char* what,uint64_t seed,uint32_t block,int step)
{
    std::printf("MISMATCH rust_core_ against cpp_core_: %s, plan %llu, block %u, step %d\n",what,(unsigned long long)seed,block,step);std::exit(1);
}
bool same(const void* a,const void* b,size_t n) { return !std::memcmp(a,b,n); }

struct Totals { uint64_t blocks=0,events=0,merged=0,plans=0,replaced=0; } totals;

void run(uint64_t seed,uint32_t blocks)
{
    Rng rng{seed*0x9E3779B97F4A7C15ull+1};
    // The plan: steps on up to three lanes, note connections with zones, parts sharing note limits, a
    // shared engine's contexts, one plugin giving up its own voices.
    std::vector<StepPlan> plan(1+rng.below(6));
    std::vector<uint32_t> clap,fade;
    uint32_t sourceCount=0;
    for(auto& st:plan)
    {
        st.lane=rng.below(3);st.inProcess=!rng.below(4);st.lateLane=!rng.below(6);st.rest=20+float(rng.below(300));st.perNote=1+float(rng.below(80));st.seedRest=rng.below(6)?st.rest:-1;
        if(!rng.below(5)){st.contexts={0,1};}
        const uint32_t n=rng.below(5)?1+rng.below(2):0; // an effect takes no notes
        for(uint32_t i=0;i<n;++i)
        {
            Source s{sourceCount++};
            s.zones.count=rng.below(4)?1+rng.below(2):0;
            for(size_t z=0;z<s.zones.count;++z)s.zones.zones[z]=zone(rng);
            if(s.zones.count==2){s.zones.zones[1]=s.zones.zones[0];s.zones.zones[1].transpose=12;s.zones.zones[1].channels=0xFFFF;}
            s.draining=!rng.below(8);
            if(!st.contexts.empty())s.channel=st.contexts[i%2];
            st.sources.push_back(s);
            clap.push_back(rng.below(5)?(rng.below(4)?1u:3u):0u);fade.push_back((const uint32_t[]){0,64,256,1000}[rng.below(4)]);
        }
    }
    auto& own=plan[rng.below(uint32_t(plan.size()))];own.ownVoices=!rng.below(3);
    if(own.ownVoices && !rng.below(2) && !own.sources.empty() && own.contexts.empty()){own.contexts={0,1};for(size_t i=0;i<own.sources.size();++i)own.sources[i].channel=int16_t(i%2);}
    for(auto& st:plan)st.honors=rng.below(2);
    for(auto& st:plan)for(auto& s:st.sources)if(s.zones.count && s.zones.zones[0].part && !s.draining) // a part's connections share its note limit
        for(auto& o:plan)for(auto& p:o.sources)if(p.zones.count && p.zones.zones[0].part==s.zones.zones[0].part && !p.draining)s.peers.push_back(p.route);
    const float budget=(const float[]){0,-1,150,400,900}[rng.below(5)],audioCores=float(1+rng.below(3));
    oscairn_zones tapZones{};tapZones.count=rng.below(2);tapZones.zones[0]=zone(rng);
    const uint32_t endTail=1+rng.below(40);

    std::unique_ptr<Side> sides[2]{std::make_unique<Side>(cores[0]),std::make_unique<Side>(cores[1])};
    for(auto& side:sides)
    {
        for(uint32_t r=0;r<sourceCount;++r)side->routes.push_back(side->core.routeNew(1,uint16_t(clap[r]),fade[r]));
        side->routes.push_back(side->core.routeNew(16,0,0));side->routes.push_back(side->core.routeNew(16,0,0)); // a tap's, a harmony's
        for(auto& st:plan)side->activities.push_back(std::make_unique<Activity>(st.seedRest,st.perNote));
        for(auto& st:plan)for(size_t c=0;c<st.contexts.size();++c)side->activities.push_back(std::make_unique<Activity>(-1,-1));
        side->pending.resize(plan.size());
    }
    auto build=[&](Side& side){
        std::vector<std::vector<oscairn_source>> sources(plan.size());std::vector<std::vector<oscairn_context>> contexts(plan.size());
        std::vector<std::vector<oscairn_route*>> peers(sourceCount);
        std::vector<oscairn_step> steps;size_t context=plan.size();
        for(size_t i=0;i<plan.size();++i)
        {
            for(auto& s:plan[i].sources)
            {
                for(auto p:s.peers)peers[s.route].push_back(side.routes[p]);
                sources[i].push_back({side.routes[s.route],&s.zones,peers[s.route].data(),uint32_t(peers[s.route].size()),s.channel,s.draining});
            }
            for(auto c:plan[i].contexts)contexts[i].push_back({c,side.activities[context++]->core()});
            steps.push_back({side.activities[i]->core(),sources[i].data(),uint32_t(sources[i].size()),contexts[i].data(),uint32_t(contexts[i].size()),plan[i].lane,plan[i].inProcess,plan[i].lateLane,nullptr,0});
        }
        auto budgeted=std::make_unique<bool[]>(plan.size());
        side.core.notesFree(side.notes);
        side.notes=side.core.notesNew(steps.data(),uint32_t(steps.size()),3,budgeted.get());
        side.budgeted.assign(budgeted.get(),budgeted.get()+plan.size());
    };
    for(auto& side:sides)build(*side);
    if(sides[0]->budgeted!=sides[1]->budgeted)fail("budgeted steps",seed,0,-1);

    std::vector<Event> input;std::vector<uint8_t> tags;
    struct Out { std::vector<Event> merged=std::vector<Event>(capacity);std::vector<int16_t> heard=std::vector<int16_t>(capacity);std::vector<Event> sent;std::vector<uint64_t> held; } out[2];
    auto collect=[](void* v,const Event* e){static_cast<std::vector<Event>*>(v)->push_back(*e);return true;};
    for(uint32_t b=0;b<blocks;++b)
    {
        input.clear();tags.clear();events(rng,b,input,tags);
        totals.events+=input.size();++totals.blocks;
        const oscairn_block timing{budget,2666.67f,2666.67f*.85f,rng.below(2)?1333.33f:1000.f,audioCores,uint64_t(rng.below(3)?96000:256)};
        // Each step's events: the beat, a release or two, parameter lane events through a source, every source's input.
        std::vector<oscairn_key> keys;std::vector<Event> extra;extra.reserve(16);
        const bool lostNow=!rng.below(300); // a block that did not fit (keys past the merged room, a lost release)
        for(auto& side:sides)side->core.notesBegin(side->notes,&timing);
        for(size_t s=0;s<plan.size();++s)
        {
            keys.clear();extra.clear();uint32_t order=0;
            extra.push_back(note(CLAP_EVENT_NOTE_OFF,0,0,0,0)); // stands in for the beat: passed on as is
            keys.push_back({0,order++,&extra.back(),-1,0xff,false});
            for(uint32_t i=0,n=rng.below(3);i<n && !plan[s].sources.empty();++i)
            {
                const uint32_t time=rng.below(frames);
                extra.push_back(parameter(time,int32_t(rng.below(20)),int16_t(rng.below(4)),int16_t(rng.below(2)?36+rng.below(14):-1),rng.below(2)));
                keys.push_back({time,order++,&extra.back(),int16_t(rng.below(uint32_t(plan[s].sources.size()))),0xff,true});
            }
            for(int16_t c=0;c<int16_t(plan[s].sources.size());++c)
                for(size_t i=0;i<input.size();++i)
                {
                    uint32_t time;std::memcpy(&time,input[i].bytes+4,4);
                    keys.push_back({time,order++,&input[i],c,tags[i],false});
                }
            bool overflow[2]{lostNow,lostNow};uint32_t n[2];
            for(int k=0;k<2;++k)
            {
                auto copy=keys; // each sorts its own
                n[k]=sides[k]->core.notesRoute(sides[k]->notes,uint32_t(s),copy.data(),uint32_t(copy.size()),frames,&sides[k]->nextId,out[k].merged.data(),capacity,
                                               plan[s].contexts.empty()?nullptr:out[k].heard.data(),&overflow[k]);
            }
            if(n[0]!=n[1] || overflow[0]!=overflow[1] || !same(out[0].merged.data(),out[1].merged.data(),n[0]*sizeof(Event)))fail("events sent",seed,b,int(s));
            if(!plan[s].contexts.empty() && !same(out[0].heard.data(),out[1].heard.data(),n[0]*2))fail("channels heard",seed,b,int(s));
            totals.merged+=n[0];
            // The plugin: its time from the notes it holds, its NOTE_ENDs, sometimes a lost block, a reset, silence.
            const bool processed=rng.below(50),reset=!rng.below(400),silent=!rng.below(3),spike=!rng.below(200);
            const uint32_t loudness=rng.below(100);
            for(int k=0;k<2;++k)
            {
                auto& side=*sides[k];
                uint32_t live=0;for(auto& src:plan[s].sources)live+=side.core.routeLive(side.routes[src.route]);
                auto& a=side.activities[s]->get();
                a.quiet.store(silent?a.quiet.load()+frames:0);
                for(size_t c=0;c<plan[s].contexts.size();++c){auto& x=side.activities[plan.size()+c]->get();x.quiet.store(silent?x.quiet.load()+frames:0);}
                auto& sent=out[k].sent;sent.clear();
                auto& p=side.pending[s];size_t keep=0;
                for(auto& x:p)if(x.due<=b)sent.push_back(x.event);else p[keep++]=x;
                p.resize(keep);
                if(!loudness)sent.push_back(note(CLAP_EVENT_NOTE_END,0,-1,int16_t(36+b%30),0,-1,-1)); // one naming only a key
                std::vector<oscairn_voice_budget_note_t> voices;
                for(uint32_t i=0;i<n[k];++i)
                {
                    clap_event_note_t x;std::memcpy(&x,out[k].merged[i].bytes,sizeof x);
                    if(x.header.space_id!=CLAP_CORE_EVENT_SPACE_ID || x.header.size!=sizeof x)continue;
                    if(x.header.type==CLAP_EVENT_NOTE_ON && voices.size()<8)voices.push_back({voices.size()%2?-1:x.note_id,x.port_index,x.channel,x.key,2,float(loudness+i)/100}); // by ID, or by channel and key
                    if((x.header.type!=CLAP_EVENT_NOTE_OFF && x.header.type!=CLAP_EVENT_NOTE_CHOKE) || x.note_id<0 || ++side.released%13==0)continue;
                    p.push_back({b+(x.header.type==CLAP_EVENT_NOTE_CHOKE?1:endTail),note(CLAP_EVENT_NOTE_END,0,x.channel,x.key,0,x.note_id,x.port_index)});
                }
                const float took=(plan[s].rest+plan[s].perNote*float(live))*(spike?3.f:1.f);
                const oscairn_finish f{took,processed,reset,false,plan[s].ownVoices,plan[s].honors,2*live,plan[s].perNote*.5,voices.data(),uint32_t(voices.size()),sent.data(),uint32_t(sent.size())};
                side.core.notesFinish(side.notes,uint32_t(s),&f);
                if(plan[s].ownVoices)a.voiceLimit=side.core.notesVoiceLimit(side.notes,uint32_t(s),2*live);
            }
            if(plan[s].ownVoices && sides[0]->activities[s]->get().voiceLimit!=sides[1]->activities[s]->get().voiceLimit)fail("voice limit",seed,b,int(s));
        }
        // A MIDI tap and a harmony: the input through each one's route alone.
        for(int k=0;k<2;++k)
        {
            auto& side=*sides[k];
            out[k].sent.clear();out[k].held.assign(2*input.size(),0);
            side.core.routeEvents(side.routes[sourceCount],&tapZones,input.data(),uint32_t(input.size()),tags.data(),true,&side.nextId,collect,&out[k].sent,nullptr);
            side.core.routeEvents(side.routes[sourceCount+1],&tapZones,input.data(),uint32_t(input.size()),nullptr,false,&side.nextId,nullptr,nullptr,out[k].held.data());
        }
        if(out[0].sent.size()!=out[1].sent.size() || !same(out[0].sent.data(),out[1].sent.data(),out[0].sent.size()*sizeof(Event)))fail("tap events",seed,b,-1);
        if(out[0].held!=out[1].held)fail("harmony keys",seed,b,-1);
        for(auto& side:sides)side->core.notesEnd(side->notes,NAN);
        // Every route's state and every activity, compared.
        for(size_t r=0;r<sides[0]->routes.size();++r)
        {
            uint8_t p[2][256];uint64_t h[2][2];
            for(int k=0;k<2;++k){sides[k]->core.routePedals(sides[k]->routes[r],p[k]);sides[k]->core.routeHeldKeys(sides[k]->routes[r],h[k]);}
            if(sides[0]->core.routeLive(sides[0]->routes[r])!=sides[1]->core.routeLive(sides[1]->routes[r]) || !same(p[0],p[1],256) || !same(h[0],h[1],16))fail("route state",seed,b,int(r));
        }
        for(size_t i=0;i<sides[0]->activities.size();++i)
            if(!same(sides[0]->activities[i]->bytes,sides[1]->activities[i]->bytes,sizeof(Activity::bytes)))
            {
                for(size_t k=0;k<sizeof(Activity::bytes);++k)if(sides[0]->activities[i]->bytes[k]!=sides[1]->activities[i]->bytes[k]){std::printf("first byte differing: %zu\n",k);break;}
                fail("activity",seed,b,int(i));
            }
        if(sides[0]->nextId!=sides[1]->nextId)fail("note IDs",seed,b,-1);
        // Now and then the plan is replaced: a connection leaves (retired, its pedals to those staying), the tap goes, the rest go on.
        if(b%500==499 && sourceCount)
        {
            const uint32_t gone=rng.below(sourceCount);bool ok[2];
            for(int k=0;k<2;++k)
            {
                auto& side=*sides[k];
                uint8_t remaining[256]{},pedals[256];
                for(uint32_t r=0;r<sourceCount;++r)if(r!=gone){side.core.routePedals(side.routes[r],pedals);for(int i=0;i<256;++i)remaining[i]=std::max(remaining[i],pedals[i]);}
                out[k].sent.clear();
                ok[k]=side.core.routeRetire(side.routes[gone],remaining,collect,&out[k].sent);
                side.core.routeLetGo(side.routes[sourceCount],collect,&out[k].sent);
                build(side);
            }
            if(ok[0]!=ok[1] || out[0].sent.size()!=out[1].sent.size() || !same(out[0].sent.data(),out[1].sent.data(),out[0].sent.size()*sizeof(Event)))fail("retired routes",seed,b,-1);
            ++totals.replaced;
        }
    }
    ++totals.plans;
}
}

int main(int argc,char** argv)
{
    const uint32_t plans=argc>1?uint32_t(std::atoi(argv[1])):60,blocks=argc>2?uint32_t(std::atoi(argv[2])):3000;
    for(uint32_t p=0;p<plans;++p)run(p,blocks);
    std::printf("engine core differential: %llu plans (%llu replaced), %llu blocks, %llu input events, %llu events sent: rust_core_ and cpp_core_ identical\n",
                (unsigned long long)totals.plans,(unsigned long long)totals.replaced,(unsigned long long)totals.blocks,(unsigned long long)totals.events,(unsigned long long)totals.merged);
    return 0;
}
