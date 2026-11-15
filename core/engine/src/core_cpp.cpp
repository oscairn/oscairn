// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine core (engine_core.h) in C++, around NoteRoute.h: the reference the Rust implementation
// is held to by the differential test, and the one OSCAIRN_ENGINE_CORE=cpp builds the engine with.
#include "engine_core.h"
#include "NoteRoute.h"
#include "event_channel.h"

#include <cstddef>
#include <cstring>

using oscairn::Graph;
using oscairn::NoteRoute;
using oscairn::NoteZones;
using Event = kick::clap::Event;

struct oscairn_route : NoteRoute { using NoteRoute::NoteRoute; };

// The layouts both implementations share with graph.h and note_zone.h.
static_assert(sizeof(oscairn_event)==sizeof(Event) && alignof(oscairn_event)==alignof(Event));
static_assert(sizeof(oscairn_zones)==sizeof(NoteZones) && offsetof(oscairn_zones,count)==offsetof(NoteZones,count));
#define OSCAIRN_SAME(a,b) static_assert(offsetof(oscairn::NoteZone,a)==offsetof(oscairn_zone,b));
OSCAIRN_SAME(lowVelocity,low_velocity) OSCAIRN_SAME(channels,channels) OSCAIRN_SAME(transpose,transpose) OSCAIRN_SAME(curve,curve)
OSCAIRN_SAME(fixedVelocity,fixed_velocity) OSCAIRN_SAME(sources,sources) OSCAIRN_SAME(maxNotes,max_notes) OSCAIRN_SAME(part,part)
OSCAIRN_SAME(reserve,reserve) OSCAIRN_SAME(priority,priority) OSCAIRN_SAME(pedals,pedals)
#undef OSCAIRN_SAME
static_assert(sizeof(oscairn::NoteZone)==sizeof(oscairn_zone));
#define OSCAIRN_SAME(a,b) static_assert(offsetof(Graph::Activity,a)==offsetof(oscairn_activity,b));
OSCAIRN_SAME(sounding,sounding) OSCAIRN_SAME(quiet,quiet) OSCAIRN_SAME(age,age) OSCAIRN_SAME(asleep,asleep) OSCAIRN_SAME(rest,rest)
OSCAIRN_SAME(perNote,per_note) OSCAIRN_SAME(seedRest,seed_rest) OSCAIRN_SAME(seedNote,seed_note) OSCAIRN_SAME(stolen,stolen)
OSCAIRN_SAME(hardChokes,hard_chokes) OSCAIRN_SAME(stolenForDevice,stolen_for_device) OSCAIRN_SAME(laneLimit,lane_limit)
OSCAIRN_SAME(laneTook,lane_took) OSCAIRN_SAME(badOutput,bad_output) OSCAIRN_SAME(badRun,bad_run) OSCAIRN_SAME(stranded,stranded)
OSCAIRN_SAME(ownVoices,own_voices) OSCAIRN_SAME(honorsVolume,honors_volume) OSCAIRN_SAME(perNoteScale,per_note_scale)
OSCAIRN_SAME(voiceLimit,voice_limit) OSCAIRN_SAME(overLimit,over_limit) OSCAIRN_SAME(stolenForLoss,faded_for_loss) OSCAIRN_SAME(lostBlocks,lost) OSCAIRN_SAME(stolenForDeadline,faded_for_deadline)
#undef OSCAIRN_SAME
static_assert(sizeof(Graph::Activity)==sizeof(oscairn_activity));

struct oscairn_notes
{
    struct Source { NoteRoute* route; NoteZones zones; bool draining; std::vector<NoteRoute*> peers; int16_t channel; };
    struct Context { int16_t channel; Graph::Activity* activity; };
    struct Step
    {
        Graph::Activity* activity; std::vector<Source> sources; std::vector<Context> contexts; uint32_t lane; bool inProcess, lateLane, budgeted=false;
        std::array<float,5> recent{}; uint8_t recentAt=0, recentN=0; // its last blocks' times (typical)
    };
    std::vector<Step> steps;
    oscairn::VoiceBudget budget;
    oscairn_block timing{};
    bool budgeting=false;
    float total=0;
};

namespace
{
const Event& event(const oscairn_event* e) { return *reinterpret_cast<const Event*>(e); }
Graph::Activity* activity(oscairn_activity* a) { return reinterpret_cast<Graph::Activity*>(a); }
auto emitter(oscairn_emit emit,void* context) { return [=](const Event& e){return !emit || emit(context,reinterpret_cast<const oscairn_event*>(&e));}; }
// The instance's NOTE_END for a context's connection: only its channel's, matched as its own notes were sent.
bool endedOn(Event& e,int16_t channel) noexcept
{
    auto* h=reinterpret_cast<clap_event_header_t*>(e.bytes.data());
    if(channel<0 || h->space_id!=CLAP_CORE_EVENT_SPACE_ID || h->type!=CLAP_EVENT_NOTE_END || h->size!=sizeof(clap_event_note_t))return true;
    auto& n=*reinterpret_cast<clap_event_note_t*>(h);
    if(n.channel>=0 && n.channel!=channel)return false;
    n.channel=-1;return true;
}
}

extern "C" {
oscairn_route* cpp_core_route_new(uint32_t ports,uint16_t clapPorts,uint32_t fadeFrames)
{
    auto* r=new oscairn_route(ports);
    for(uint32_t p=0;p<r->clap.size();++p)r->clap[p]=clapPorts>>p&1;
    r->fadeFrames=fadeFrames;return r;
}
void cpp_core_route_free(oscairn_route* route) { delete route; }
uint32_t cpp_core_route_live(const oscairn_route* route) { return route->live(); }
void cpp_core_route_pedals(const oscairn_route* route,uint8_t pedals[256]) { std::memcpy(pedals,route->pedal.data(),256); }
void cpp_core_route_held_keys(const oscairn_route* route,uint64_t keys[2]) { keys[0]=route->heldKeys[0];keys[1]=route->heldKeys[1]; }
bool cpp_core_route_retire(oscairn_route* route,const uint8_t remaining[256],oscairn_emit emit,void* context)
{
    return route->retire(emitter(emit,context),[&](size_t port,int channel){return remaining[port*16+size_t(channel)];});
}
void cpp_core_route_let_go(oscairn_route* route,oscairn_emit emit,void* context)
{
    auto send=emitter(emit,context);
    for(const auto& n:route->active())if(n.used)send(route->release(n,0,false));
    for(int p=0;p<16;++p)for(int c=0;c<16;++c)if(route->pedal[p][c]>=64)send(Event::midi(uint8_t(0xb0|c),64,0));
    route->clear();
}
void cpp_core_route_events(oscairn_route* route,const oscairn_zones* zones,const oscairn_event* events,uint32_t count,const uint8_t* tags,bool skipEnds,
                           uint32_t* nextId,oscairn_emit emit,void* context,uint64_t* held)
{
    const auto send=emitter(emit,context);
    for(uint32_t i=0;i<count;++i)
    {
        const auto* h=event(&events[i]).header();
        if(h->space_id!=CLAP_CORE_EVENT_SPACE_ID || (skipEnds && h->type==CLAP_EVENT_NOTE_END))continue;
        route->route(event(&events[i]),*nextId,send,reinterpret_cast<const NoteZones*>(zones),tags?tags[i]:oscairn::untaggedSource);
        if(held){held[2*i]=route->heldKeys[0];held[2*i+1]=route->heldKeys[1];}
    }
}

oscairn_notes* cpp_core_notes_new(const oscairn_step* steps,uint32_t count,uint32_t lanes,bool* budgeted)
{
    auto* notes=new oscairn_notes;
    for(uint32_t s=0;s<count;++s)
    {
        const auto& d=steps[s];
        oscairn_notes::Step step{activity(d.activity),{},{},d.lane,d.in_process,d.late_lane};
        for(uint32_t i=0;i<d.source_count;++i)
        {
            const auto& c=d.sources[i];
            step.sources.push_back({c.route,*reinterpret_cast<const NoteZones*>(c.zones),c.draining,{c.peers,c.peers+c.peer_count},c.channel});
        }
        for(uint32_t i=0;i<d.context_count;++i)step.contexts.push_back({d.contexts[i].channel,activity(d.contexts[i].activity)});
        notes->steps.push_back(std::move(step));
    }
    // The shared budget: every note connection, grouped by part (a MIDI-dialect plugin's too, which a note-off ends).
    auto& entries=notes->budget.entries;
    for(auto& step:notes->steps)if(step.activity)for(auto& source:step.sources)
        {
            const uint32_t part=source.zones.count?source.zones.zones[0].part:0;
            uint32_t group=uint32_t(entries.size());bool rest=true;
            for(size_t i=0;i<entries.size();++i)
            {
                const auto* z=entries[i].zones;
                if(part && group==entries.size() && z->count && z->zones[0].part==part && entries[i].draining==source.draining)group=entries[i].group;
                if(entries[i].cost==step.activity)rest=false;
            }
            entries.push_back({source.route,&source.zones,step.activity,group,rest,source.draining,step.lane});
            step.budgeted=true;
        }
    notes->budget.groups.resize(entries.size());
    notes->budget.lanes.resize(std::max<size_t>(1,lanes));
    for(uint32_t s=0;s<count;++s)budgeted[s]=notes->steps[s].budgeted;
    return notes;
}
void cpp_core_notes_free(oscairn_notes* notes) { delete notes; }

void cpp_core_notes_begin(oscairn_notes* notes,const oscairn_block* block)
{
    // Each lane's limit: the budget set (RFC 0025's share of an audio core) but never more than loadMargin
    // of the time its blocks have before they are missed (RFC 0045's load guard).
    const auto& t=notes->timing=*block;
    notes->budgeting=t.budget!=0;
    if(!notes->budgeting)return;
    constexpr float loadMargin=.8f; // the limit's share of a lane's time, for the wake-ups, copies and the machine's own around its blocks
    notes->total=(t.budget>0?t.budget:loadMargin*t.period)*t.cores;
    for(auto& lane:notes->budget.lanes){lane.limit=t.budget>0?t.budget:INFINITY;lane.miss=INFINITY;lane.took=lane.othersTook=0;}
    for(const auto& step:notes->steps)
    {
        auto& lane=notes->budget.lanes[step.lane];
        const float time=step.lateLane?t.period:step.inProcess?t.deadline:t.allowance;
        lane.miss=std::min(lane.miss,time);lane.limit=std::min(lane.limit,loadMargin*time);
    }
}

uint32_t cpp_core_notes_route(oscairn_notes* notes,uint32_t s,oscairn_key* keys,uint32_t count,uint32_t frames,uint32_t* nextId,oscairn_event* out,
                              uint32_t capacity,int16_t* heard,bool* lost)
{
    auto& step=notes->steps[s];
    std::sort(keys,keys+count,[](const oscairn_key& a,const oscairn_key& b){
        if(a.time!=b.time)return a.time<b.time;
        if(a.after_notes!=b.after_notes)return !a.after_notes;
        return a.order<b.order;
    });
    uint32_t merged=0;
    int16_t channel=-1; // the connection's into a shared engine's context, whose channel its events take
    auto emit=[&](const auto& e){
        if(merged==capacity)return false;
        if(heard)heard[merged]=int16_t(channel<0?-1:oscairn::channelOf(e));
        reinterpret_cast<Event&>(out[merged++])=channel<0?e:oscairn::onChannel(e,channel);return true;};
    // Stolen notes' fade steps go in among the events, at their times.
    auto fades=[&](uint32_t until){bool ok=true;for(auto& source:step.sources){channel=source.channel;ok=source.route->fade(until,emit) && ok;}channel=-1;return ok;};
    bool overflow=*lost;
    if(!overflow)for(uint32_t k=0;k<count;++k)
    {
        const auto& key=keys[k];
        if(!fades(std::min(key.time,frames))){overflow=true;break;}
        auto* source=key.source>=0 && size_t(key.source)<step.sources.size()?&step.sources[size_t(key.source)]:nullptr;
        channel=source && !key.after_notes?source->channel:-1;
        const auto& e=event(key.event);
        const bool sent=!source || key.time>=frames?emit(e):key.after_notes?source->route->route(e,*nextId,emit):
            source->route->route(e,*nextId,emit,&source->zones,key.tag,source->draining,source->peers,&notes->budget);
        channel=-1;
        if(!sent){overflow=true;break;}
    }
    if(!overflow && !fades(frames))overflow=true;
    for(auto& source:step.sources)source.route->advance(frames);
    *lost=overflow;
    if(!overflow)return merged;
    for(auto& source:step.sources)source.route->clear(); // a lost release: the host resets the plugin in its own dialect
    return 0;
}

void cpp_core_notes_finish(oscairn_notes* notes,uint32_t s,const oscairn_finish* f)
{
    auto& step=notes->steps[s];
    if(f->reset)for(auto& source:step.sources)source.route->clear();
    // The notes sounding through each of a shared engine's contexts' connections, and through every node (for its cost per voice, RFC 0025).
    for(auto& c:step.contexts)
    {
        uint32_t sounding=0;for(const auto& source:step.sources)if(source.channel==c.channel)sounding+=source.route->sounding();
        c.activity->sounding.store(sounding,std::memory_order_relaxed);
    }
    if(step.activity)
    {
        uint32_t sounding=0;for(const auto& source:step.sources)sounding+=source.route->sounding();
        step.activity->sounding.store(sounding,std::memory_order_relaxed);
    }
    if(notes->budgeting)
    {
        // What a lane's limit is held against: each step's usual block, the median of its last five (graph.cpp).
        step.recent[step.recentAt]=f->took;step.recentAt=uint8_t((step.recentAt+1)%step.recent.size());
        step.recentN=uint8_t(std::min<size_t>(step.recentN+1u,step.recent.size()));
        auto v=step.recent;std::nth_element(v.begin(),v.begin()+step.recentN/2,v.begin()+step.recentN);
        const float raw=f->took,usual=v[step.recentN/2];
        auto& lane=notes->budget.lanes[step.lane];
        lane.took+=usual;
        if(!step.budgeted)lane.othersTook+=usual;
        else
        {
            uint32_t live=0;for(const auto& source:step.sources)live+=source.route->live();
            // A plugin reporting no voices while notes live, or whose block was not rendered, is counted by its notes; one
            // giving up its own voices is judged against its limit so even when its blocks are lost.
            const uint32_t voices=f->voices && f->processed?f->voices:live;
            auto& a=*step.activity;
            a.overLimit=a.voiceLimit && voices>a.voiceLimit?a.overLimit+1:0;
            if(f->processed)
            {
                a.ownVoices=f->own_voices;a.honorsVolume=f->honors_volume;
                // A block past twice both the usual and what its notes are estimated to cost now is held up from outside
                // (new notes raise the estimate with them, a real overload the usual within three blocks): the larger taken.
                const float expected=std::max(usual,a.restNow()+a.noteNow()/a.perNoteScale*float(a.ownVoices?voices:live));
                const float took=a.perNote.load(std::memory_order_relaxed)>=0 && raw>2*expected?expected:raw;
                if(!a.ownVoices)a.observe(took,live);
                else
                {
                    // Its voices, each costed alike, a note at its share of them, and each note's
                    // loudness in place of its velocity (by its ID, else its channel and key).
                    if(f->voice_cost>0 && a.perNote.load(std::memory_order_relaxed)<0)a.seedNote.store(float(f->voice_cost),std::memory_order_relaxed);
                    a.observe(took,voices);a.perNoteScale=live?std::max(1.f,float(voices)/float(live)):1.f;
                    for(uint32_t i=0;i<f->note_count;++i)for(auto& source:step.sources)for(auto& slot:source.route->active())
                        if(slot.used && (f->notes[i].note_id>=0?slot.id==f->notes[i].note_id:(source.channel<0?slot.address.channel:source.channel)==f->notes[i].channel && slot.key==f->notes[i].key))
                            slot.velocity=f->notes[i].loudness;
                }
            }
        }
    }
    for(uint32_t i=0;i<f->sent_count;++i)for(auto& source:step.sources){auto e=event(&f->sent[i]);if(endedOn(e,source.channel))source.route->ended(e);}
    // Silent for noteEndQuiet (a shared engine: its context's bus), its released notes no plugin ended are let go.
    // shortcut: silence is the node's, not the note's, so one sounding release keeps a dead one; per-voice loudness if that is seen.
    for(auto& source:step.sources)if(source.route->top){
        const Graph::Activity* a=step.activity;
        if(!a)for(const auto& c:step.contexts)if(c.channel==source.channel)a=c.activity;
        if(a && a->quiet.load(std::memory_order_relaxed)>=notes->timing.quiet_after)source.route->expire();
    }
}

uint32_t cpp_core_notes_voice_limit(const oscairn_notes* notes,uint32_t s,uint32_t voices,uint32_t* contexts,uint32_t count)
{
    const auto& step=notes->steps[s];
    if(!step.activity)return 0;
    // The voices of the notes it holds unreleased or released, not given up: the step's, or a context's (its channel).
    auto admitted=[&](const int16_t* channel){
        uint32_t n=0;
        for(const auto& source:step.sources)if(!channel || source.channel==*channel)for(const auto& slot:source.route->active())n+=slot.used && !slot.stolen;
        return std::ceil(float(n)*step.activity->perNoteScale);};
    const float all=admitted(nullptr);
    const uint32_t limit=notes->budget.voiceLimit(step.lane,*step.activity,voices,all);
    for(uint32_t c=0;c<count && c<step.contexts.size();++c)
    {
        const float own=admitted(&step.contexts[c].channel);
        contexts[c]=!limit || own==0?0:uint32_t(std::max(1.f,own+std::floor((float(limit)-all)*own/all)));
    }
    return limit;
}

void cpp_core_notes_end(oscairn_notes* notes,float)
{
    if(!notes->budgeting)return;
    auto& budget=notes->budget;
    for(uint32_t i=0;i<budget.lanes.size();++i){auto& lane=budget.lanes[i];lane.others=lane.othersTook;budget.measured(i,lane.took);}
    budget.device(notes->total);
    for(const auto& step:notes->steps)if(step.activity) // for the timing report
    {
        const auto& lane=budget.lanes[step.lane];
        step.activity->laneLimit.store(lane.limit,std::memory_order_relaxed);step.activity->laneTook.store(lane.took,std::memory_order_relaxed);
    }
}
}
