// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "oscairn/graph.h"
#include "oscairn/plugin_host.h"
#include "oscairn/midi_input.h"
#include "oscairn/note_zone.h"
#include <array>
#include <algorithm>
#include <climits>
#include <span>

namespace oscairn
{
// One audio-thread ledger per connection; shared by successive compiled plans.
// Destination IDs separate otherwise identical notes from different graph sources.
struct NoteRoute
{
    using Event=kick::clap::Event;
    // address: the note as its source sent it, which releases and expressions match; key: the
    // key it plays here, after its zone's transpose; zone: which zone let it through, or -1;
    // source: where it entered the graph, so another device's release of the same key leaves it.
    // stolen: given up to keep within the connection's note limit, fading then choked, waiting for
    // the plugin to end it. volume: its last volume expression; fadeAt: when its next fade step
    // is due in this block, or -1; faded: the steps sent.
    struct Note { clap_event_note_t address{}; int32_t id=-1; bool used=false,held=false,midi=false,stolen=false; int16_t key=-1; int8_t zone=-1; uint8_t source=untaggedSource;
        double volume=1; int32_t fadeAt=-1; uint8_t faded=0;
        bool voiceEnded=false; // its voice ended (NOTE_END) while the key was held
        float velocity=0; }; // as it was sent on, or the plugin's report of its loudness (oscairn.voice-budget), for the budget's quietest first
    static constexpr uint8_t fadeSteps=8;
    std::array<Note,kick::clap::Host::maxBlockEvents> notes{};
    std::array<std::array<uint8_t,16>,16> pedal{};
    std::array<bool,16> clap{};
    uint32_t ports=0;
    uint32_t fadeFrames=0,fading=0; // a stolen note's fade, 0 to choke at once; notes fading
    // The keys its notes hold (bit k of word k/64), each key's held notes counted: kept as notes are
    // held, lifted and let go (hold, lift, drop), so a harmony (RFC 0033) reads them without a scan.
    std::array<uint64_t,2> heldKeys{};
    std::array<uint16_t,128> heldCount{};
    // Each pedal's (sustain, sostenuto, soft) last value from each source by channel, the untagged last: a
    // route with zones sends the largest of those its receive takes (NoteZones::takesPedal).
    std::array<std::array<std::array<uint8_t,maxSources+1>,16>,pedalClasses> pedalsBy{};
    std::array<std::array<uint8_t,16>,pedalClasses> pedalsSent{}; // each pedal's value last sent on by channel
    // One past the last slot used (slots are taken lowest first): the scans of used slots stop there.
    uint16_t top=0;
    std::span<Note> active() noexcept {return {notes.data(),top};}
    std::span<const Note> active() const noexcept {return {notes.data(),top};}
    explicit NoteRoute(uint32_t ports):ports(ports) {} // a MIDI destination: MIDI's notes on every port
    explicit NoteRoute(const PluginHost& host,uint32_t fadeFrames=0):ports(host.noteInputPorts()),fadeFrames(fadeFrames) {for(uint32_t p=0;p<clap.size();++p)clap[p]=host.usesClapNotes(p);}
    void clear() noexcept {for(auto& n:active())n.used=false;for(auto& p:pedal)p.fill(0);pedalsBy={};pedalsSent={};fading=0;heldKeys={};heldCount={};top=0;}
    void hold(Note& n) noexcept {n.held=true;if(n.key>=0 && n.key<128 && !heldCount[n.key]++)heldKeys[n.key/64]|=uint64_t(1)<<(n.key%64);}
    void lift(Note& n) noexcept {if(n.used && n.held && n.key>=0 && n.key<128 && !--heldCount[n.key])heldKeys[n.key/64]&=~(uint64_t(1)<<(n.key%64));n.held=false;}
    void drop(Note& n) noexcept {lift(n);n.used=false;while(top && !notes[top-1].used)--top;}
    // Whether an event from `source` may release or shape a note: untagged events reach every note.
    static bool from(const Note& n,uint8_t source) noexcept {return source==untaggedSource || n.source==source;}
    static bool matches(const Note& n,int32_t id,int16_t port,int16_t channel,int16_t key) noexcept
    {return n.used && (id<0 || id==n.address.note_id) && (port<0 || port==n.address.port_index) &&
        (channel<0 || channel==n.address.channel) && (key<0 || key==n.address.key);}
    Event release(const Note& n,uint32_t time,bool choke) const noexcept
    {
        auto e=n.address;e.header.time=time;e.header.type=choke?CLAP_EVENT_NOTE_CHOKE:CLAP_EVENT_NOTE_OFF;
        e.key=n.key;e.note_id=clap[e.port_index]?n.id:-1;e.velocity=0;return Event::from(e);
    }
    // Notes still sounding by the player's hand: keys down, or released under the sustain pedal.
    uint32_t sounding() const noexcept
    {uint32_t count=0;for(const auto& n:active())if(n.used && (n.held || pedal[n.address.port_index&15][n.address.channel&15]>=64))++count;return count;}
    void ended(const Event& e) noexcept
    {
        if(e.header()->space_id!=CLAP_CORE_EVENT_SPACE_ID || e.header()->type!=CLAP_EVENT_NOTE_END || e.header()->size!=sizeof(clap_event_note_t))return;
        const auto& end=*reinterpret_cast<const clap_event_note_t*>(e.header());
        for(auto& n:active())if(n.used && (end.note_id<0 || n.id==end.note_id) &&
            (end.port_index<0 || n.address.port_index==end.port_index) &&
            (end.channel<0 || n.address.channel==end.channel) && (end.key<0 || n.key==end.key))
            {
                // NOTE_END says a voice ended, not that the key went up: a mono synth gliding one voice
                // ends a new note's ID at once. A held key keeps its slot, so its release still goes out.
                if(n.held)n.voiceEnded=true;else drop(n);
            }
    }
    // A stolen note fades (RFC 0018): its volume expression falls in steps over fadeFrames, for
    // plugins that do not fade a choke themselves, then it is choked. Sends the steps due before
    // `until`, in time order across notes.
    template<class Emit> bool fade(uint32_t until,Emit&& emit) noexcept
    {
        bool ok=true;
        while(fading) {
            Note* next=nullptr;fading=0;
            for(auto& n:active())if(n.used && n.fadeAt>=0){++fading;if(!next || n.fadeAt<next->fadeAt)next=&n;}
            if(!next || uint32_t(next->fadeAt)>=until)break;
            const auto at=uint32_t(next->fadeAt);
            if(next->faded==fadeSteps)
            {
                next->fadeAt=-1;ok=emit(release(*next,at,true)) && ok;
                // A MIDI note's choke goes out as its note-off, its end here unless the pedal holds it.
                if(!clap[next->address.port_index] && pedal[next->address.port_index&15][next->address.channel&15]<64)drop(*next);
                continue;
            }
            clap_event_note_expression_t v{};v.header={sizeof(v),at,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_EXPRESSION,0};
            v.expression_id=CLAP_NOTE_EXPRESSION_VOLUME;v.note_id=next->id;v.port_index=next->address.port_index;
            v.channel=next->address.channel;v.key=next->key;v.value=next->volume*(fadeSteps-1-next->faded)/fadeSteps;
            ++next->faded;next->fadeAt+=int32_t(fadeFrames/fadeSteps);
            ok=emit(Event::from(v)) && ok;
        }
        return ok;
    }
    // The notes it holds until their plugin ends them, released and fading ones included.
    uint32_t live() const noexcept {uint32_t count=0;for(const auto& n:active())count+=n.used;return count;}
    // The note-end watchdog: a released note whose plugin sends no NOTE_END (none, or one lost) would hold
    // its slot, a voice to the budget, for ever. Called once its plugin has been silent a while, which a
    // release still sounding is not: those slots are freed. A key down, a pedal holding it (a MIDI note's
    // release waits for the pedal) or a fade under way keeps its slot.
    void expire() noexcept
    {for(auto& n:active())if(n.used && !n.held && n.fadeAt<0 && clap[n.address.port_index] && pedal[n.address.port_index&15][n.address.channel&15]<64)drop(n);}
    // Chokes a note at `at` without (more of) a fade, at the budget's hard limit.
    void choke(Note& n,uint32_t at) noexcept
    {
        if(n.stolen && n.fadeAt>=0)n.fadeAt=std::min(n.fadeAt,int32_t(at));
        else {n.stolen=true;lift(n);n.fadeAt=int32_t(at);++fading;}
        n.faded=fadeSteps;
    }
    // Gives a note up from elsewhere (a peer's note limit, the budget): it fades from `at`, or is choked there;
    // a MIDI note, which no volume reaches, is released there.
    void steal(Note& n,uint32_t at) noexcept
    {n.stolen=true;lift(n);n.fadeAt=int32_t(at);n.faded=fadeFrames && clap[n.address.port_index]?0:fadeSteps;++fading;}
    // A block of `frames` done: fades carry on from the next block's start.
    void advance(uint32_t frames) noexcept
    {if(fading)for(auto& n:active())if(n.used && n.fadeAt>=0)n.fadeAt=std::max(0,n.fadeAt-int32_t(frames));}
    template<class Emit,class RemainingPedal> bool retire(Emit&& emit,RemainingPedal&& remainingPedal) noexcept
    {
        bool ok=true;
        std::array<std::array<bool,16>,16> panicChannels{};
        for(auto& n:active())if(n.used) {
            if(clap[n.address.port_index])ok=emit(release(n,0,true)) && ok;
            else panicChannels[n.address.port_index][n.address.channel]=true;
        }
        // MIDI cannot choke one pedal-held voice; silence its channel rather than strand it.
        for(size_t p=0;p<pedal.size();++p)for(int c=0;c<16;++c) {
            if(panicChannels[p][c])ok=emit(Event::midi(0xb0|c,120,0,0,p)) && ok;
            if(pedal[p][c])ok=emit(Event::midi(0xb0|c,64,remainingPedal(p,c),0,p)) && ok;
        }
        clear();return ok;
    }
    template<class Emit> bool route(Event e,uint32_t& nextId,Emit&& emit,const NoteZones* zones=nullptr,uint8_t source=untaggedSource,bool draining=false,std::span<NoteRoute* const> peers={}) noexcept
    {return route(e,nextId,emit,[](const Note&,uint32_t){},zones,source,draining,peers);}
    // zones, when given, decide which notes pass and how they change (note_zone.h); each zone
    // that accepts a note plays its own copy. source: where the event entered the graph.
    // draining: a connection being retired (RFC 0021's seamless switching) takes no new notes;
    // releases, expressions and the pedal still reach the notes it holds. peers: the routes sharing
    // the note limit (a part's lanes), this one included; none: this one alone.
    template<class Emit,class End> bool route(Event e,uint32_t& nextId,Emit&& emit,End&& end,const NoteZones* zones=nullptr,uint8_t source=untaggedSource,bool draining=false,std::span<NoteRoute* const> peers={}) noexcept
    {
        if(!e.valid())return emit(e);
        const auto* h=e.header();
        if(h->space_id!=CLAP_CORE_EVENT_SPACE_ID)return emit(e);
        bool midi=false;
        if(h->type==CLAP_EVENT_MIDI && h->size==sizeof(clap_event_midi_t))
        {
            auto m=*reinterpret_cast<const clap_event_midi_t*>(h);
            if(m.port_index>=ports || m.port_index>=16)return emit(e);
            const auto kind=m.data[0]&0xf0,channel=m.data[0]&15;
            if(const int f=functionOf(uint8_t(kind),m.data[1]);f>=0 && zones && zones->count && !zones->takesFunction(f,source))return true; // a controller this part's receive setting does not take
            // Releases, chokes and a sustain pedal-up pass any zone: they end only notes this
            // route played, which zones since changed may no longer pass; one for a note never
            // played here matches nothing. A pedal-down still reaches only the zones' channels.
            const bool release=kind==0x80 || (kind==0x90 && !m.data[2]) || (kind==0xb0 && (m.data[1]==120 || m.data[1]==123 || (m.data[1]==64 && m.data[2]<64)));
            if(const int p=kind==0xb0?pedalOf(m.data[1]):-1;zones && zones->count && p>=0)
            {
                // A pedal reaches a part by its receive setting, whichever device its notes come from (a pedal-down
                // only on its zones' channels); it goes on as the largest of the sources taken, when that differs from
                // what went on last. A pedal-up from any source counts: a receive edited while a pedal is down lets go
                // of it with that pedal's release.
                bool channelTaken=false;for(size_t z=0;z<zones->count;++z)channelTaken|=(zones->zones[z].channels>>channel&1)!=0;
                if(m.data[2]>=64 && (!zones->takesPedal(m.data[1],source) || !channelTaken))return true;
                auto& by=pedalsBy[size_t(p)][channel];
                by[std::min<size_t>(source,maxSources)]=m.data[2];
                uint8_t most=0;for(size_t s=0;s<by.size();++s)if(zones->takesPedal(m.data[1],uint8_t(s)))most=std::max(most,by[s]);
                if(most==pedalsSent[size_t(p)][channel])return true;
                pedalsSent[size_t(p)][channel]=most;m.data[2]=most;reinterpret_cast<clap_event_midi_t*>(e.bytes.data())->data[2]=m.data[2];h=e.header();
            }
            else if(zones && zones->count && kind>=0x80 && kind<0xf0 && !release && !zones->passes(channel,source))return true; // another zone's channel or device
            else if(kind==0xb0 && pedalOf(m.data[1])>=0 && !(zones && zones->count)) // no zones: it goes on as it came, kept for zones set later
            {
                const auto p=size_t(pedalOf(m.data[1]));
                pedalsBy[p][channel][std::min<size_t>(source,maxSources)]=m.data[2];pedalsSent[p][channel]=m.data[2];
            }
            if(kind==0x90 || kind==0x80) {
                e=Event::note(kind==0x90 && m.data[2],m.data[1],m.data[2]/127.0,h->time,channel,m.port_index);midi=true;h=e.header();
            } else if(kind==0xb0 && m.data[1]==64) {
                pedal[m.port_index][channel]=m.data[2];
                if(!clap[m.port_index] && m.data[2]<64)for(auto& n:active())
                    if(n.used && !n.held && n.address.port_index==m.port_index && n.address.channel==channel){end(n,h->time);drop(n);}
                return emit(e);
            } else if(kind==0xb0 && (m.data[1]==120 || m.data[1]==123)) {
                auto n=Event::note(false,-1,0,h->time,channel,m.port_index);
                if(m.data[1]==120)reinterpret_cast<clap_event_header_t*>(n.bytes.data())->type=CLAP_EVENT_NOTE_CHOKE;
                return route(n,nextId,emit,end,zones,source,draining,peers);
            } else if(kind==0xa0 || kind==0xd0) {
                Event pressure;
                if(!midiToClap(m.data,kind==0xd0?2:3,h->time,pressure))return emit(e);
                reinterpret_cast<clap_event_note_expression_t*>(pressure.bytes.data())->port_index=m.port_index;
                return route(pressure,nextId,emit,end,zones,source,draining,peers);
            } else return emit(e);
        }
        if(h->type==CLAP_EVENT_NOTE_ON && h->size==sizeof(clap_event_note_t))
        {
            if(draining)return true;
            const auto n=*reinterpret_cast<const clap_event_note_t*>(h);
            if(n.port_index>=16 || static_cast<uint32_t>(n.port_index)>=ports)return emit(e);
            // A note limit: before a note past it, the oldest released note goes, else the oldest
            // held one, sparing the highest and lowest held keys (melody and bass) while another
            // can go; it fades, then is choked. Counted across the peers, the notes they can choke:
            // only where the plugin takes CLAP notes, as MIDI cannot choke one note.
            const uint16_t limit=zones && zones->count && clap[n.port_index]?zones->zones[0].maxNotes:0;
            NoteRoute* const self[]{this};
            const auto routes=peers.empty()?std::span<NoteRoute* const>(self):peers;
            auto makeRoom=[&]()->bool {
                if(!limit)return true;
                for(;;) {
                    Note* victim=nullptr;NoteRoute* owner=nullptr;size_t playing=0,held=0;int16_t low=INT16_MAX,high=-1;
                    for(const auto* r:routes)for(const auto& slot:r->active())if(slot.used && !slot.stolen && r->clap[slot.address.port_index] && slot.held){++held;low=std::min(low,slot.key);high=std::max(high,slot.key);}
                    auto spared=[&](const Note& n){return n.held && held>2 && (n.key==low || n.key==high);};
                    for(auto* r:routes)for(auto& slot:r->active())if(slot.used && !slot.stolen && r->clap[slot.address.port_index]) {
                        ++playing;
                        if(!victim || (spared(*victim) && !spared(slot)) || (spared(*victim)==spared(slot) &&
                           ((victim->held && !slot.held) || (victim->held==slot.held && slot.id<victim->id)))){victim=&slot;owner=r;}
                    }
                    if(playing<limit || !victim)return true;
                    if(owner!=this){owner->steal(*victim,0);continue;} // its step sends the fade, from its block's start
                    victim->stolen=true;lift(*victim);
                    if(fadeFrames){victim->fadeAt=int32_t(h->time);++fading;}
                    else if(!emit(release(*victim,h->time,true)))return false;
                }
            };
            auto play=[&](int8_t zone,int16_t key,double velocity) {
                if(!makeRoom())return false;
                // Full: a held note whose voice already ended gives its slot up, as before NOTE_END kept it,
                // but its key's release goes out first (a mono synth's gliding voice stops on it, whatever
                // the pedal: the normal key-up emits it regardless too); refused, the slot stays.
                if(std::none_of(notes.begin(),notes.end(),[](const Note& x){return !x.used;}))
                    for(auto& slot:notes)if(slot.used && slot.voiceEnded){if(emit(release(slot,h->time,false)))drop(slot);break;}
                // Still full: the oldest released note gives its slot up. Its release went out; only
                // its NOTE_END is missing (a plugin that sends none, or one lost), and without this
                // every later note would be refused and the plugin panicked.
                if(std::none_of(notes.begin(),notes.end(),[](const Note& x){return !x.used;})) {
                    Note* oldest=nullptr;
                    for(auto& slot:notes)if(slot.used && !slot.held && (!oldest || slot.id<oldest->id))oldest=&slot;
                    if(oldest){end(*oldest,h->time);drop(*oldest);}
                }
                for(auto& slot:notes)if(!slot.used) {
                    if(nextId>INT32_MAX)return false;
                    slot={n,static_cast<int32_t>(nextId++),true,false,midi,false,key,zone,source};slot.velocity=float(velocity);hold(slot);top=std::max(top,uint16_t(&slot-notes.data()+1));
                    auto copy=n;copy.key=key;copy.velocity=velocity;copy.note_id=clap[n.port_index]?slot.id:-1;return emit(Event::from(copy));
                }
                return false;
            };
            if(!zones || !zones->count)return play(-1,n.key,n.velocity);
            bool ok=true;
            for(size_t z=0;z<zones->count;++z)if(const auto& zone=zones->zones[z];zone.accepts(n.key,n.velocity,n.channel,source))
                if(const auto key=n.key+zone.transpose;key>=0 && key<=127)ok=play(int8_t(z),int16_t(key),zone.shape(n.velocity)) && ok;
            return ok; // no zone took it: dropped, and so is its release

        }
        if((h->type==CLAP_EVENT_NOTE_OFF || h->type==CLAP_EVENT_NOTE_CHOKE) && h->size==sizeof(clap_event_note_t))
        {
            const auto n=*reinterpret_cast<const clap_event_note_t*>(h);const bool choke=h->type==CLAP_EVENT_NOTE_CHOKE;
            // A repeated MIDI key releases its oldest note, in each zone that played it.
            std::array<Note*,NoteZones::capacity+1> oldest{};
            if(midi)for(auto& slot:active())if(slot.held && slot.midi && from(slot,source) && matches(slot,-1,n.port_index,n.channel,n.key))
                if(auto& o=oldest[size_t(slot.zone+1)];!o || slot.id<o->id)o=&slot;
            bool ok=true,sent=false;
            for(auto& slot:active())if(!slot.stolen && from(slot,source) && matches(slot,n.note_id,n.port_index,n.channel,n.key) && (!midi || &slot==oldest[size_t(slot.zone+1)])) {
                // A note whose voice already ended needs only the key's release, once: not one per slot.
                if(slot.voiceEnded && sent){drop(slot);continue;}
                sent=true;
                lift(slot);
                const bool sustained=pedal[slot.address.port_index][slot.address.channel]>=64;
                auto output=release(slot,h->time,choke);
                auto* off=reinterpret_cast<clap_event_note_t*>(output.bytes.data());off->velocity=n.velocity;off->header.flags=h->flags;
                ok=emit(output) && ok;
                if(slot.voiceEnded)drop(slot); // its end was the plugin's own, already reported
                else if(choke || (!clap[slot.address.port_index] && !sustained)){end(slot,h->time);drop(slot);}
            }
            return ok;
        }
        if(h->type==CLAP_EVENT_NOTE_EXPRESSION && h->size==sizeof(clap_event_note_expression_t))
        {
            const auto expression=*reinterpret_cast<const clap_event_note_expression_t*>(h);bool ok=true;
            const bool volume=expression.expression_id==CLAP_NOTE_EXPRESSION_VOLUME;
            for(auto& slot:active())if(from(slot,source) && matches(slot,expression.note_id,expression.port_index,expression.channel,expression.key)) {
                if(volume && slot.stolen)continue; // its fade owns its volume
                if(volume)slot.volume=expression.value;
                auto out=e;auto& copy=*reinterpret_cast<clap_event_note_expression_t*>(out.bytes.data()); // the event's own bytes: a struct copy leaves its padding indeterminate
                copy.note_id=clap[slot.address.port_index]?slot.id:-1;
                copy.port_index=slot.address.port_index;copy.channel=slot.address.channel;copy.key=slot.key;
                ok=emit(out) && ok;
            }
            return ok;
        }
        if((h->type==CLAP_EVENT_PARAM_VALUE && h->size==sizeof(clap_event_param_value_t)) ||
           (h->type==CLAP_EVENT_PARAM_MOD && h->size==sizeof(clap_event_param_mod_t))) {
            // Both CLAP parameter event structs have the same note-address fields.
            auto remap=[&]<class T>(const T& parameter) {
                if(parameter.note_id<0 && parameter.key<0)return emit(e);
                bool ok=true;
                for(const auto& slot:active())if(matches(slot,parameter.note_id,parameter.port_index,parameter.channel,parameter.key)) {
                    auto out=e;auto& copy=*reinterpret_cast<T*>(out.bytes.data()); // as above: no struct copy, whose padding is indeterminate
                    copy.note_id=clap[slot.address.port_index]?slot.id:-1;
                    copy.port_index=slot.address.port_index;copy.channel=slot.address.channel;copy.key=slot.key;
                    ok=emit(out) && ok;
                }
                return ok;
            };
            if(h->type==CLAP_EVENT_PARAM_VALUE)return remap(*reinterpret_cast<const clap_event_param_value_t*>(h));
            return remap(*reinterpret_cast<const clap_event_param_mod_t*>(h));
        }
        if(h->type==CLAP_EVENT_NOTE_END)return true; // consumed by the owning destination, never forwarded
        return emit(e);
    }
};
}
