// SPDX-License-Identifier: AGPL-3.0-or-later
// A plan's block (engine_core.h's run_*) in C++: Graph::process and beginBlock as they were before the
// Rust core, their PluginHost calls through oscairn_plugins; the reference the Rust implementation
// (core/engine/rust/src/run.rs) is held to by engine-run-diff, and the one OSCAIRN_ENGINE_CORE=cpp
// builds the engine with.
#include "engine_core.h"
#include "event_channel.h"
#include "oscairn/graph.h"
#include "oscairn/harmony.h"
#include <oscairn-clap/beat.h>
#include <oscairn-clap/harmony.h>
#include <oscairn-clap/param-target.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <vector>

using oscairn::Graph;
using Event = kick::clap::Event;
namespace
{
constexpr uint8_t untaggedSource=0xff;
const oscairn_event* core(const Event* e) { return reinterpret_cast<const oscairn_event*>(e); }
const Event* events(const oscairn_event* e) { return reinterpret_cast<const Event*>(e); }
Graph::Activity* activity(oscairn_activity* a) { return reinterpret_cast<Graph::Activity*>(a); }
template<class Emit> bool emitTo(void* emit,const oscairn_event* e) { return (*static_cast<Emit*>(emit))(*events(e)); }
std::atomic<uint64_t>& drops(uint64_t* d) { return *reinterpret_cast<std::atomic<uint64_t>*>(d); }

// A routed event as a MIDI 1.0 channel message on `channel` (1..16; 0: its own): notes,
// pressure, and MIDI events' own bytes. False for anything else.
bool toMidi(const Event& e,uint8_t channel,oscairn_midi& m) noexcept
{
    const auto* h=e.header();
    if(h->space_id!=CLAP_CORE_EVENT_SPACE_ID)return false;
    auto on=[&](int own){return uint8_t(channel?channel-1:std::clamp(own,0,15));};
    auto seven=[](double v){return uint8_t(std::lround(std::clamp(v,0.0,1.0)*127));};
    auto set=[&](uint8_t a,uint8_t b,uint8_t c,uint8_t size){m.bytes[0]=a;m.bytes[1]=b;m.bytes[2]=c;m.size=size;};
    if((h->type==CLAP_EVENT_NOTE_ON || h->type==CLAP_EVENT_NOTE_OFF || h->type==CLAP_EVENT_NOTE_CHOKE) && h->size==sizeof(clap_event_note_t))
    {
        const auto& n=*reinterpret_cast<const clap_event_note_t*>(h);
        if(n.key<0 || n.key>127)return false;
        const bool down=h->type==CLAP_EVENT_NOTE_ON;
        set(uint8_t((down?0x90:0x80)|on(n.channel)),uint8_t(n.key),down?std::max<uint8_t>(seven(n.velocity),1):seven(n.velocity),3);
        return true;
    }
    if(h->type==CLAP_EVENT_NOTE_EXPRESSION && h->size==sizeof(clap_event_note_expression_t))
    {
        const auto& x=*reinterpret_cast<const clap_event_note_expression_t*>(h);
        if(x.expression_id!=CLAP_NOTE_EXPRESSION_PRESSURE || x.key>127)return false;
        if(x.key<0)set(uint8_t(0xd0|on(x.channel)),seven(x.value),0,2);
        else set(uint8_t(0xa0|on(x.channel)),uint8_t(x.key),seven(x.value),3);
        return true;
    }
    if(h->type==CLAP_EVENT_MIDI && h->size==sizeof(clap_event_midi_t))
    {
        const auto& x=*reinterpret_cast<const clap_event_midi_t*>(h);
        const auto kind=x.data[0]&0xf0;
        if(kind<0x80 || kind>=0xf0 || x.data[1]>127 || x.data[2]>127)return false;
        set(uint8_t(kind|on(x.data[0]&15)),x.data[1],x.data[2],kind==0xc0 || kind==0xd0?2:3);
        return true;
    }
    return false;
}
}

struct oscairn_run
{
    oscairn_notes* notes; oscairn_audio* audio;
    struct Harmony
    {
        oscairn_route* route; oscairn::NoteZones zones; bool input=false; uint8_t root=0; uint16_t scale=1; oscairn_harmony_t* last;
        std::array<Event,32> events; uint32_t count=0;
        std::array<std::array<uint64_t,2>,kick::clap::Host::maxBlockEvents> keys{}; // the keys held after each input event
    };
    std::vector<Harmony> harmonies;
    struct Control { std::vector<Graph::ControlCurve> path; double origin; bool modulate; };
    std::vector<Control> controls;
    struct Tap { int step; oscairn_route* route; oscairn::NoteZones zones; uint64_t ports; uint8_t channel; };
    std::vector<Tap> taps;
    struct Step
    {
        const void* host; uint32_t id; Graph::Activity* activity; int harmony=-1;
        struct NoteSource { int step; oscairn_route* route; oscairn::NoteZones zones; bool draining; int16_t channel; };
        std::vector<NoteSource> noteSources;
        std::array<Event,kick::clap::Host::maxBlockEvents> releases; uint32_t releaseCount=0; bool releaseOverflow=false;
        std::vector<oscairn_lane_route> routes;
        std::vector<int16_t> paramNotes; std::vector<Event> paramIn; uint32_t paramCount=0;
        std::vector<Event> merged; uint32_t mergedCount=0;
        bool processed=false, started=false; float cost=0; int64_t handed=0;
        bool budgeted=false; uint64_t missed=0;
        std::array<oscairn_voice_budget_context_t,OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> voiceLimits{}; uint32_t voiceLimit=0, voiceContexts=0;
        struct Context { uint32_t id; int16_t channel; Graph::Activity* activity; };
        std::vector<Context> contexts;
        std::vector<Event> contextIn; std::vector<int16_t> heardOn;
    };
    std::vector<Step> steps;
    std::vector<uint32_t> waves, local; std::vector<float> after;
    std::vector<oscairn_key> keys;
};

extern "C" {
oscairn_run* cpp_core_run_new(oscairn_notes* notes,oscairn_audio* audio,const oscairn_step* steps,const bool* budgeted,uint32_t count,const uint32_t* waves,uint32_t waveCount,
                              const oscairn_harmony_source* harmonies,uint32_t harmonyCount,const oscairn_tap* taps,uint32_t tapCount,const oscairn_control* controls,uint32_t controlCount)
{
    auto* run=new oscairn_run{};run->notes=notes;run->audio=audio;
    auto zones=[](const oscairn_zones* z){oscairn::NoteZones out;std::memcpy(&out,z,sizeof out);return out;};
    for(uint32_t i=0;i<harmonyCount;++i){const auto& h=harmonies[i];auto& x=run->harmonies.emplace_back();x.route=h.route;x.zones=zones(h.zones);x.input=h.input;x.root=h.root;x.scale=h.scale;x.last=h.last;}
    for(uint32_t i=0;i<controlCount;++i){const auto& c=controls[i];run->controls.push_back({{reinterpret_cast<const Graph::ControlCurve*>(c.path),reinterpret_cast<const Graph::ControlCurve*>(c.path)+c.count},c.origin,c.modulate});}
    for(uint32_t i=0;i<tapCount;++i)run->taps.push_back({taps[i].step,taps[i].route,zones(taps[i].zones),taps[i].ports,taps[i].channel});
    run->steps.resize(count);
    for(uint32_t i=0;i<count;++i)
    {
        const auto& s=steps[i];auto& step=run->steps[i];
        step.host=s.host;step.id=s.id;step.activity=activity(s.activity);step.harmony=s.harmony;step.budgeted=budgeted[i];
        for(uint32_t k=0;k<s.source_count;++k){const auto& c=s.sources[k];step.noteSources.push_back({c.from,c.route,zones(c.zones),c.draining,c.channel});}
        step.routes.assign(s.lane_routes,s.lane_routes+s.lane_route_count);
        for(uint32_t k=0;k<s.context_count;++k)step.contexts.push_back({s.contexts[k].id,s.contexts[k].channel,activity(s.contexts[k].activity)});
        if(s.lanes_in){step.paramIn.resize(kick::clap::Host::maxBlockEvents);step.paramNotes.resize(kick::clap::Host::maxBlockEvents);}
        if(s.context_count){step.contextIn.resize(kick::clap::Host::maxBlockEvents);step.heardOn.resize(kick::clap::Host::maxProcessEvents);}
        step.merged.resize(kick::clap::Host::maxProcessEvents); // maxBlockEvents notes, plus the beat and up to 32 harmony changes
    }
    run->waves.assign(waves,waves+waveCount);
    run->keys.reserve(kick::clap::Host::maxProcessEvents); // as many as merged holds (every other key is capped at its size): never reallocating
    run->local.resize(count);
    run->after.resize(waveCount);
    return run;
}

void cpp_core_run_free(oscairn_run* run) { delete run; }

void cpp_core_run_adopt(oscairn_run* next,oscairn_run* current,bool ramp,uint32_t rampFrames,oscairn_midi* midi,uint32_t* midiCount,uint64_t* eventDrops)
{
    for(auto& old:current->steps)for(auto& step:next->steps)if(old.host==step.host)
        for(auto& source:old.noteSources)
            if(std::none_of(step.noteSources.begin(),step.noteSources.end(),[&](const auto& s){return s.route==source.route;}))
            {
                // Its pedals go to what the routes staying hold.
                std::array<uint8_t,256> remaining{},pedals;
                for(const auto& r:step.noteSources){cpp_core_route_pedals(r.route,pedals.data());for(size_t i=0;i<256;++i)remaining[i]=std::max(remaining[i],pedals[i]);}
                auto emit=[&](const Event& e){
                    if(step.releaseCount==step.releases.size())return false;
                    step.releases[step.releaseCount++]=oscairn::onChannel(e,source.channel);return true;};
                step.releaseOverflow|=!cpp_core_route_retire(source.route,remaining.data(),&emitTo<decltype(emit)>,&emit);
            }
    // A plugin the new plan no longer runs (removed, replaced) is sent no releases for the notes it
    // holds: its voices stop where they are. One an undo brings back chokes them first, in its first block.
    auto stranded=[](oscairn_run::Step& step,bool set){
        bool was=false;
        if(step.contexts.empty())was=step.activity && step.activity->stranded.exchange(set,std::memory_order_relaxed);
        else for(auto& c:step.contexts)was|=c.activity->stranded.exchange(set,std::memory_order_relaxed); // the nodes' own: the engine's goes with them
        return was;};
    for(auto& old:current->steps)
        if(std::none_of(next->steps.begin(),next->steps.end(),[&](const auto& s){return s.host==old.host;}) &&
           std::any_of(old.noteSources.begin(),old.noteSources.end(),[](const auto& s){return cpp_core_route_live(s.route)!=0;}))
            stranded(old,true);
    for(auto& step:next->steps)if(stranded(step,false)){
        if(step.releaseCount==step.releases.size()){step.releaseOverflow=true;continue;}
        auto choke=Event::note(false,-1,0,0,-1,-1);reinterpret_cast<clap_event_header_t*>(choke.bytes.data())->type=CLAP_EVENT_NOTE_CHOKE;
        step.releases[step.releaseCount++]=choke;}
    // A tap gone: note offs for the notes it holds, and its pedals released.
    for(auto& old:current->taps)
        if(std::none_of(next->taps.begin(),next->taps.end(),[&](const auto& t){return t.route==old.route;}))
        {
            auto send=[&](const Event& e){
                oscairn_midi m{};if(!toMidi(e,old.channel,m))return true;
                if(*midiCount==Graph::maxMidiOut){drops(eventDrops).fetch_add(1,std::memory_order_relaxed);return true;}
                m.ports=old.ports;midi[(*midiCount)++]=m;return true;};
            cpp_core_route_let_go(old.route,&emitTo<decltype(send)>,&send);
        }
    // A played change (commit(true)): a route in both plans ramps from the gain it has now to its new one.
    if(ramp)cpp_core_audio_carry(next->audio,current->audio,rampFrames);
}

bool cpp_core_run_block(oscairn_run* run,const oscairn_run_block* b,const oscairn_plugins* p)
{
    const uint32_t frames=b->frames,eventCount=b->count;
    const Event* const blockEvents=events(b->events);
    const uint8_t* const sources=b->tags;
    const bool panicInput=b->panic_input,panicNotes=b->panic_notes,listener=b->listening;
    void* const cx=p->context;
    auto& eventDrops=drops(b->drops);
    auto& nextNoteId=*b->next_id;
    auto* const plan=run;
    auto stepOf=[&](const oscairn_run::Step& step){return uint32_t(&step-plan->steps.data());};
    auto outputEvents=[&](uint32_t step,uint32_t& count){return events(p->sent(cx,step,&count));};
    cpp_core_audio_begin(plan->audio,b->inputs,b->ni,b->outputs,b->no,frames);
    const Event& beatEvent=*events(b->beat);
    // RFC 0033: each part's harmony at sample 0, then each change its input's keys make.
    for(auto& h:plan->harmonies)
    {
        auto add=[&](uint32_t time,const std::array<uint64_t,2>& keys){
            oscairn_harmony_t x{};x.key_root=h.root;x.scale=h.scale;
            x.held[0]=keys[0];x.held[1]=keys[1];
            oscairn::recognizeChord(x);
            if(h.count && !std::memcmp(&x,h.last,sizeof x))return;
            *h.last=x;
            oscairn_event_harmony_t e{};
            e.header={sizeof(e),time,kick::clap::Host::harmonySpace,OSCAIRN_EVENT_HARMONY,0};
            e.harmony=x;
            // shortcut: past 32 changes in one block the last slot takes each later one, at its time; raise the capacity if played input ever needs more.
            if(h.count==h.events.size())--h.count;
            h.events[h.count++]=Event::from(e);};
        h.count=0;
        std::array<uint64_t,2> keys;cpp_core_route_held_keys(h.route,keys.data());add(0,keys);
        if(!h.input)continue;
        cpp_core_route_events(h.route,reinterpret_cast<const oscairn_zones*>(&h.zones),b->events,eventCount,sources,false,&nextNoteId,nullptr,nullptr,h.keys[0].data());
        for(uint32_t i=0;i<eventCount;++i)if(blockEvents[i].header()->space_id==CLAP_CORE_EVENT_SPACE_ID)add(std::min(blockEvents[i].header()->time,frames-1),h.keys[i]);
    }
    bool ok = true;
    // A step in three parts (RFC 0034): its inputs, mixed and merged, and what it sent, routed
    // on, stay on this thread in plan order; only the plugin's rendering moves.
    auto prepare=[&](oscairn_run::Step& step)
    {
        const bool inputRecovery=(panicNotes && !step.noteSources.empty()) || (panicInput && std::any_of(step.noteSources.begin(),step.noteSources.end(),[](const auto& s){return s.step<0;}));
        cpp_core_audio_mix(plan->audio,stepOf(step),nullptr,frames,b->live_from,b->live_to);

        // Notes in: every source's events this block, merged in time order (by the engine core).
        plan->keys.clear();
        uint32_t order = 0;
        auto key=[&](const Event& e,uint32_t time=0,int16_t source=-1,uint8_t tag=untaggedSource,bool afterNotes=false){
            plan->keys.push_back({time,order++,core(&e),source,tag,afterNotes});};
        key(beatEvent); // oscairn.beat, first at sample 0
        if(step.harmony>=0) // the part's harmony, each before the notes at its sample
        {
            const auto& h=plan->harmonies[(size_t)step.harmony];
            for(uint32_t i=0;i<h.count;++i)key(h.events[i],h.events[i].header()->time);
        }
        uint64_t totalEvents=step.paramCount+step.releaseCount;
        bool overflow=step.releaseOverflow || inputRecovery;
        // The beat and the part's harmony ride with the notes in their own room (see
        // maxProcessEvents): a block that fills all maxBlockEvents note slots still fits.
        for(uint32_t i=0;i<step.releaseCount;++i)key(step.releases[i]);
        if(step.contexts.empty())for(uint32_t i=0;i<b->param_count;++i)if(b->params[i].node==step.id)
        {
            ++totalEvents;
            if(plan->keys.size()<step.merged.size())key(*events(&b->params[i].event));
            else overflow=true;
        }
        // A shared engine's contexts: each one's parameters, queued or set, on its own channel.
        uint32_t contextCount=0;
        for(auto& context:step.contexts)
        {
            auto add=[&](const Event& e){
                ++totalEvents;
                if(contextCount<step.contextIn.size() && plan->keys.size()<step.merged.size()){step.contextIn[contextCount]=oscairn::onChannel(e,context.channel);key(step.contextIn[contextCount++]);}
                else overflow=true;
                return true;};
            for(uint32_t i=0;i<b->param_count;++i)if(b->params[i].node==context.id)add(*events(&b->params[i].event));
            p->parameters(cx,stepOf(step),uint32_t(&context-step.contexts.data()),&emitTo<decltype(add)>,&add);
        }
        for( uint32_t i = 0; i < step.paramCount; ++i ) {
            if(plan->keys.size()<step.merged.size())
                key(step.paramIn[i],step.paramIn[i].header()->time,step.paramNotes[i],untaggedSource,step.paramNotes[i]>=0);
            else overflow=true;
        }
        for(int16_t c=0;c<int16_t(step.noteSources.size());++c)
        {
            const int source=step.noteSources[size_t(c)].step;
            const Event* list  = blockEvents;
            uint32_t     count = eventCount;
            if( source >= 0 )
                list = outputEvents(uint32_t(source),count);
            for( uint32_t i = 0; i < count; ++i )
                if( list[i].header()->space_id == CLAP_CORE_EVENT_SPACE_ID && list[i].header()->type != CLAP_EVENT_NOTE_END ) // note ends belong to their destination
                {
                    ++totalEvents;
                    if(plan->keys.size()<step.merged.size())
                        key(list[i],list[i].header()->time,c,source<0 && sources?sources[i]:untaggedSource);
                    else overflow=true;
                }
        }
        // Routed through its connections, with stolen notes' fade steps among them; a lost release must reset the
        // destination in its own dialect (the core empties its ledgers).
        step.mergedCount=cpp_core_notes_route(plan->notes,stepOf(step),plan->keys.data(),uint32_t(plan->keys.size()),frames,&nextNoteId,
            reinterpret_cast<oscairn_event*>(step.merged.data()),uint32_t(step.merged.size()),step.heardOn.empty()?nullptr:step.heardOn.data(),&overflow);
        if(overflow){eventDrops.fetch_add(totalEvents,std::memory_order_relaxed);p->panic(cx,stepOf(step));}
    };
    // oscairn.voice-budget (RFC 0018 step 4): a plugin that gives up its own voices gets, between
    // blocks, the voices the budget leaves it, with its part's reserve and priority for the
    // contexts it plays (one entry, for the selected ones), when any of it changed.
    auto limitVoices=[&](oscairn_run::Step& step)
    {
        if(!step.budgeted || !step.activity->ownVoices)return;
        double cost;const oscairn_voice_budget_note_t* notes;uint32_t count;
        const uint32_t voices=p->voices(cx,stepOf(step),&cost,&notes,&count);
        // One entry for the selected contexts, or a shared engine's: each of its contexts' own, with
        // its part's reserve and priority, under the sound its channel selects (RFC 0021 slice 3).
        std::array<oscairn_voice_budget_context_t,OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> parts{};uint32_t entries=0;
        auto part=[&](int16_t channel,uint32_t context){
            auto& q=parts[entries++];q.context=context;
            for(const auto& source:step.noteSources)if(source.channel==channel && !source.draining && source.zones.count){q.reserve=source.zones.zones[0].reserve;q.priority=source.zones.zones[0].priority;break;}};
        if(step.contexts.empty())part(-1,OSCAIRN_VOICE_BUDGET_SELECTED);
        else for(const auto& c:step.contexts)if(entries<parts.size())part(c.channel,p->sound(cx,stepOf(step),uint32_t(&c-step.contexts.data())));
        // Each sound context's own limit too: one sound's tails not given up for another's notes while the instance keeps within its own.
        std::array<uint32_t,OSCAIRN_VOICE_BUDGET_MAX_CONTEXTS> own{};
        const uint32_t limit=cpp_core_notes_voice_limit(plan->notes,stepOf(step),voices,own.data(),step.contexts.empty()?0:entries);
        if(!step.contexts.empty())for(uint32_t i=0;i<entries;++i)parts[i].limit=own[i];
        if(limit==step.voiceLimit && entries==step.voiceContexts && !std::memcmp(parts.data(),step.voiceLimits.data(),entries*sizeof parts[0]))return;
        step.voiceLimit=limit;step.voiceLimits=parts;step.voiceContexts=entries;step.activity->voiceLimit=limit;
        p->set_limits(cx,stepOf(step),limit,parts.data(),entries);
    };
    // RFC 0045, sleeping plugins: a block its plugin would sleep through, not called (no core events for it, its input
    // quiet, the transport as it went, its host saying nothing else is pending): silence, rendered.
    auto rest=[&](oscairn_run::Step& step){
        const bool events=std::any_of(step.merged.begin(),step.merged.begin()+step.mergedCount,[](const Event& e){return e.header()->space_id==CLAP_CORE_EVENT_SPACE_ID;});
        if(b->transport_wake || events || !cpp_core_audio_quiet(plan->audio,stepOf(step),frames) || !p->rest(cx,stepOf(step),frames))return false;
        cpp_core_audio_silence(plan->audio,stepOf(step),frames);step.processed=true;
        return true;};
    auto start=[&](oscairn_run::Step& step){step.handed=p->now(cx);return p->start(cx,stepOf(step),core(step.merged.data()),step.mergedCount);};
    auto render=[&](oscairn_run::Step& step){step.processed=p->render(cx,stepOf(step),core(step.merged.data()),step.mergedCount);};
    const bool budgeting=b->timing.budget!=0;
    cpp_core_notes_begin(plan->notes,&b->timing);
    auto finish=[&](oscairn_run::Step& step)
    {
        const uint32_t s=stepOf(step);
        if(listener && step.contexts.empty())p->listen(cx,step.id,core(step.merged.data()),step.mergedCount,false);
        else if(listener)for(const auto& c:step.contexts)for(uint32_t i=0;i<step.mergedCount;++i) // a shared engine's: each node what came on its channel, as it came
            if(const int channel=oscairn::channelOf(step.merged[i]);channel<0 || channel==c.channel)
            {
                if(step.heardOn[i]<0){p->listen(cx,c.id,core(&step.merged[i]),1,false);continue;}
                const auto heard=oscairn::onChannel(step.merged[i],step.heardOn[i]);p->listen(cx,c.id,core(&heard),1,false);
            }
        const bool processed=step.processed;
        // The output guard, its meters, and its and its contexts' quiet, asleep and age.
        cpp_core_audio_finish(plan->audio,s,frames,processed,processed && p->asleep(cx,s),b->fade);
        // A busy control gate can discard a release; reset when rendering resumes. A worker's late
        // or busy block lost nothing (eventsKept), so its notes go on (RFC 0045). A host that
        // reset the plugin on its own ended every note it was sent, with no NOTE_END: a shared
        // engine's, every context's.
        const bool lost=!processed && !p->kept(cx,s);
        if(lost)p->panic(cx,s);
        const bool reset=lost || p->was_reset(cx,s);
        ok &= processed;
        if(listener && processed){uint32_t sent=0;const auto* list=outputEvents(s,sent);p->listen(cx,step.id,core(list),sent,true);}
        // The engine core: its notes emptied if the plugin lost them; the notes sounding through it counted (for its
        // cost per voice, RFC 0025); its block's time against its lane's limit and its cost per note measured, the
        // plugin's own voice report when it gives up its own; its NOTE_ENDs; and the note-end watchdog.
        oscairn_finish rendered{};rendered.took=budgeting?float(p->took(cx,s)):0.f;rendered.processed=processed;rendered.reset=reset;
        // RFC 0045's guard: a block of its played late or silenced since its last (a worker past its allowance).
        const uint64_t missed=p->missed(cx,s);rendered.missed=missed>step.missed;step.missed=missed;
        if(budgeting && step.budgeted && processed)
        {
            rendered.voices=p->voices(cx,s,&rendered.voice_cost,&rendered.notes,&rendered.note_count);
            rendered.own_voices=p->own_voices(cx,s);rendered.honors_volume=p->honors_volume(cx,s);
        }
        rendered.sent=p->sent(cx,s,&rendered.sent_count);
        cpp_core_notes_finish(plan->notes,s,&rendered);
        step.paramCount = 0;step.releaseCount=0;step.releaseOverflow=false;

        // This step's param-target lane events become parameter events for their
        // targets, which come later in this block.
        if( !step.routes.empty() )
        {
            uint32_t    count  = 0;
            const auto* output = outputEvents(s,count);
            for( uint32_t i = 0; i < count; ++i )
            {
                const auto* h = output[i].header();
                if( h->space_id != kick::clap::Host::paramTargetSpace || h->type != OSCAIRN_EVENT_PARAM_TARGET )
                    continue;
                const auto& e     = *reinterpret_cast<const oscairn_event_param_target_t*>( h );
                for( const auto& r : step.routes ) // a lane may reach several parameters, through a control
                {
                    if( r.lane != e.lane )
                        continue;
                    const auto* route = &r;
                    auto& target = plan->steps[(size_t)route->target];
                    // Per-note lane addresses belong to the source's corresponding note route.
                    if(e.note_id>=0 && route->notes<0){eventDrops.fetch_add(1,std::memory_order_relaxed);continue;}
                    const int16_t notes = e.note_id>=0?route->notes:int16_t(-1);
                    // A target's block full of lane points (maxBlockEvents): the newest replaces the last
                    // one waiting for the same parameter, so its stream thins but ends at its latest
                    // value; each point lost is counted, and the target is never reset for it.
                    auto place = [&]( const Event& event ) {
                        if( target.paramCount < target.paramIn.size() )
                        {
                            target.paramNotes[target.paramCount] = notes;
                            target.paramIn[target.paramCount++] = event;
                            return;
                        }
                        const auto& a = *reinterpret_cast<const clap_event_param_value_t*>( event.header() ); // a value and a modulation alike up to their number
                        for( uint32_t k = target.paramCount; k-- > 0; )
                        {
                            const auto& c = *reinterpret_cast<const clap_event_param_value_t*>( target.paramIn[k].header() );
                            if( c.header.type == a.header.type && c.param_id == a.param_id && c.note_id == a.note_id && c.port_index == a.port_index && c.channel == a.channel && c.key == a.key )
                            {
                                target.paramIn[k] = event;
                                target.paramNotes[k] = notes;
                                break;
                            }
                        }
                        eventDrops.fetch_add( 1, std::memory_order_relaxed );
                    };
                    const double range = route->maximum - route->minimum;
                    // Through a control: the path from the control's value to the parameter's fraction.
                    const auto* control = route->control >= 0 ? &plan->controls[(size_t)route->control] : nullptr;
                    auto follow = [&]( double x ) {
                        const size_t n = control->path.size();
                        for( size_t k = 0; k < n; ++k )
                            x = control->modulate && k + 1 == n ? control->path[k].shift + control->path[k].point( x ) : control->path[k].next( x );
                        return x;
                    };
                    // The control's value from the lane: a SET event's, or a MODULATE event's amount from where it is.
                    const double x = !control ? 0 : std::clamp( e.mode == OSCAIRN_PARAM_TARGET_SET ? e.value : control->origin + std::clamp( e.value, -1.0, 1.0 ), 0.0, 1.0 );
                    if( control ? !control->modulate : e.mode == OSCAIRN_PARAM_TARGET_SET )
                    {
                        clap_event_param_value_t v{};
                        v.header     = { sizeof( v ), h->time, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_VALUE, 0 };
                        v.param_id   = route->param;
                        v.note_id    = e.note_id;
                        v.port_index = e.port_index;
                        v.channel    = route->channel >= 0 ? route->channel : e.channel;
                        v.key        = e.key;
                        v.value      = std::fma( control ? follow( x ) : std::clamp( e.value, 0.0, 1.0 ), range, route->minimum );
                        if( route->stepped )
                            v.value = std::round( v.value );
                        place( Event::from( v ) );
                    }
                    else
                    {
                        clap_event_param_mod_t m{};
                        m.header     = { sizeof( m ), h->time, CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_PARAM_MOD, 0 };
                        m.param_id   = route->param;
                        m.note_id    = e.note_id;
                        m.port_index = e.port_index;
                        m.channel    = route->channel >= 0 ? route->channel : e.channel;
                        m.key        = e.key;
                        m.amount     = ( control ? follow( x ) : std::clamp( e.value, -1.0, 1.0 ) ) * range;
                        place( Event::from( m ) );
                    }
                }
            }
        }
    };
    // RFC 0045: no wait on a worker past the callback's deadline (realtime.h); with step 6, each wave's
    // workers wait to it less what the waves after theirs took in the last block, the costliest of each.
    const int64_t callbackEnds=b->deadline;
    const bool deadlines=callbackEnds && b->deadlines;
    p->deadline(cx,callbackEnds,deadlines);
    auto less=[&](uint32_t wave){return callbackEnds-int64_t(plan->after[wave])*1000;};
    // The reserves also bound the wait for a block a call late (RFC 0045's extra block), always. At most half a
    // period: one long block in a later wave (a preemption) must not take every earlier wave's wait away.
    if(float later=0;callbackEnds)
        for(size_t w=plan->waves.size();w-->0;)
        {
            plan->after[w]=std::min(later,b->timing.period/2);
            float most=0;
            for(uint32_t i=w?plan->waves[w-1]:0;i<plan->waves[w];++i)most=std::max(most,plan->steps[i].cost);
            later+=most;
        }
    auto micros=[&](int64_t ns){return float(ns)/1000.f;}; // as duration<float,std::micro> of steady_clock's nanoseconds
    if(!b->parallel)
        for(auto& step:plan->steps){prepare(step);limitVoices(step);if(!rest(step))render(step);finish(step);}
    else for(uint32_t first=0,wave=0;const auto end:plan->waves)
    {
        // A wave: each plugin in a worker starts as soon as its inputs are ready; the others
        // render meanwhile, here and on the audio helpers; then every one finishes, in plan order.
        uint32_t locals=0;
        if(deadlines)p->deadline(cx,less(wave),true);
        for(uint32_t i=first;i<end;++i){auto& step=plan->steps[i];prepare(step);limitVoices(step);if(rest(step)){step.started=false;continue;}step.started=start(step);if(!step.started)plan->local[locals++]=i;}
        struct Job
        {
            oscairn_run* plan;const oscairn_plugins* p;
            static void render(void* context,uint32_t i) noexcept
            {
                const auto& j=*static_cast<Job*>(context);auto& step=j.plan->steps[j.plan->local[i]];
                const int64_t began=j.p->now(j.p->context);
                step.processed=j.p->render(j.p->context,j.plan->local[i],core(step.merged.data()),step.mergedCount);
                step.cost=float(j.p->now(j.p->context)-began)/1000.f;
            }
        } job{plan,p};
        // The helpers only when, by the last block, the wave leaves enough besides its largest
        // plugin to pay for waking them; otherwise all here.
        float total=0,largest=0;
        for(uint32_t k=0;k<locals;++k){const float c=plan->steps[plan->local[k]].cost;total+=c;largest=std::max(largest,c);}
        if(total-largest>=b->worth)p->helpers(cx,locals,&Job::render,&job);
        else for(uint32_t k=0;k<locals;++k)Job::render(&job,k);
        // RFC 0045: plugins a block late last, so waiting for their blocks in flight overlaps the wave's others, and
        // only to the callback's deadline less what the waves after this one took in the last block: the plugins
        // after them keep their time.
        for(const bool late:{false,true})for(uint32_t i=first;i<end;++i)if(auto& step=plan->steps[i];step.started && p->late(cx,i)==late)
        {
            if(late && callbackEnds)p->deadline(cx,less(wave),deadlines);
            step.processed=p->finish(cx,i);
            // A block given up on took the time it was allowed, not what the worker needs: the last kept. A block a call
            // late costs the waves before it nothing: its wait for the block in flight is what their reserve bounds, and
            // counted, a slow one would take the earlier waves' waits away, their blocks then missed (and so on).
            if(step.processed)step.cost=late?0.f:micros(p->now(cx)-step.handed);
        }
        if(callbackEnds)p->deadline(cx,deadlines?less(wave):callbackEnds,deadlines);
        for(uint32_t i=first;i<end;++i)finish(plan->steps[i]);
        first=end;++wave;
    }
    p->deadline(cx,0,false);

    // MIDI for external outputs (setMidiTaps), from the input or a node's note outputs.
    for(auto& tap:plan->taps)
    {
        const Event* list=blockEvents;uint32_t count=eventCount;
        if(tap.step>=0)list=outputEvents(uint32_t(tap.step),count);
        auto emit=[&](const Event& e){
            oscairn_midi m{};if(!toMidi(e,tap.channel,m))return true;
            if(*b->midi_count==Graph::maxMidiOut){eventDrops.fetch_add(1,std::memory_order_relaxed);return true;}
            m.time=std::min(e.header()->time,frames-1);m.ports=tap.ports;b->midi[(*b->midi_count)++]=m;return true;};
        cpp_core_route_events(tap.route,reinterpret_cast<const oscairn_zones*>(&tap.zones),core(list),count,tap.step<0?sources:nullptr,true,&nextNoteId,
                              &emitTo<decltype(emit)>,&emit,nullptr);
    }
    cpp_core_audio_mix(plan->audio,uint32_t(plan->steps.size()),b->outputs,frames,b->live_from,b->live_to);
    // The lanes judged, the callback's time left once the block is collected (RFC 0045's guard), the timing report kept.
    const float slack=callbackEnds?micros(callbackEnds-p->now(cx)):NAN;
    cpp_core_notes_end(plan->notes,slack);
    return ok;
}
}
