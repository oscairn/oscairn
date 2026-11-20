// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0046 step 5: the owner's scenario through real workers, in every placement and in process. A piano part holding 30
// notes under the sustain pedal and an expensive lead holding two keys, each a voice fixture giving up its own voices and
// reporting its least important (draft 2; its voices on channel 1 cost what a lead's do, OSCAIRN_FIXTURE_CHANNEL_US), on a
// real-time thread paced at the period. The owner's rule: a voice goes only where taking it frees the time that is short.
//   - Same thread (pool with one process, in process): the lane over its time gives up piano voices, the lead none.
//   - Apart (a worker per plugin, chain with an insert after the lead, pool with two processes), the device out of core
//     time (OSCAIRN_AUDIO_CORES=1 and a budget under the lanes' sum, each lane within its own): piano voices go, the lead's
//     none.
//   - Apart, the lead's own lane over its time (three struck and let go, then two held): the lead gives up its own voices
//     and keeps its held keys; the piano, on another lane, loses nothing.
//   - A piano without the report (no oscairn.voice-budget, the host's notes), apart, the device short: its notes give way,
//     the lead's do not.
//   - Pool with one process, 2,000 blocks, a new piano note every 25: the lead never touched, no part silent, and no more
//     given up than the first excess and the new notes bring (no flapping).
// In every case no part goes silent and every plugin keeps to its caps. A claim a slow machine could fail by itself is
// checked only when the guard counted no lost block on the part it is about; that the piano loses nothing for the lead's lane,
// also only when it kept its 30 notes until the lead played (under ThreadSanitizer a piano voice costs some 30 us, not 15, and
// its own lane gives up some 10 before the lead plays: the machine, not the lead). Usage: voice-fixture (OSCAIRN_PLUGIN_WORKER
// set, OSCAIRN_WORKER_BUDGET_US the allowance).
//   oscairn-voice-scenario-test
#include "oscairn/graph.h"
#include "oscairn/realtime.h"
#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
using namespace oscairn;
using kick::clap::Event;

namespace
{
constexpr uint32_t frames=128;
const char* const effect="org.oscairn.test.costly-effect";
int failures=0;
void check(bool ok,const std::string& what){if(!ok){std::printf("FAIL: %s\n",what.c_str());++failures;}}

enum class Mode { Plugin, Chain, PoolTogether, PoolApart, InProcess };
const char* name(Mode m){switch(m){case Mode::Plugin:return "plugin";case Mode::Chain:return "chain";case Mode::PoolTogether:return "pool, one process";case Mode::PoolApart:return "pool, two processes";default:return "in process";}}
enum class Case { SameThread, Device, OwnLane, NoReport, Held };

struct Seen
{
    uint32_t pianoBefore=0,pianoLeft=0,leadLeft=0,fewestPiano=UINT32_MAX,fewestLead=UINT32_MAX;
    uint64_t pianoGave=0,leadGave=0,heldOff=0,mismatch=0,pianoLost=0,leadLost=0,pianoDeadline=0,leadDeadline=0; bool apart=false;
    bool calm() const { return pianoLost+leadLost+pianoDeadline+leadDeadline==0; } // the machine lost no block on either part
};
uint64_t gave(const Graph::NoteCost& c){return c.stolen+c.stolenForDevice+c.hardChokes+c.stolenForLoss+c.stolenForDeadline;}
// The voices a node sounds: its report where it gives one, else the notes the host holds for it.
uint32_t voices(Graph& g,NodeId n){double cost;const oscairn_voice_budget_note_t* notes;uint32_t count;return g.host(n).voiceBudget()?g.host(n).voiceReport(cost,notes,count):g.sounding(n);}

Seen run(const char* fixture,const char* worker,Mode mode,Case c){
    PlacementPolicy policy;policy.worker=mode==Mode::InProcess?"":worker;
    policy.hosts=mode==Mode::PoolTogether?1:mode==Mode::PoolApart?2:0;policy.chain=mode==Mode::Chain;
    setPlacementPolicy(policy);
    // A piano voice 15 us a block, a lead voice 180 (in the engine's process, whose lane has the callback's time, 37.5 and
    // 450): 30 pedaled and two held, 810 us, past a lane's limit (80 % of a 1,000 us allowance); five lead voices, 900.
    const bool here=mode==Mode::InProcess;
    setenv("OSCAIRN_FIXTURE_VOICE_US",here?"37.5":"15",1);setenv("OSCAIRN_FIXTURE_CHANNEL_US",here?"1:450":"1:180",1);
    setenv("OSCAIRN_FIXTURE_SUSTAIN","1",1);setenv("OSCAIRN_FIXTURE_DECAY_S","8",1);setenv("OSCAIRN_FIXTURE_EFFECT_US","20",1);
    const bool device=c==Case::Device || c==Case::NoReport;
    if(device)setenv("OSCAIRN_AUDIO_CORES","1",1);
    Graph g(48000,frames);
    // Chain: the lead's insert joins its worker (placementGroups, as a Session places a Performance's plugins).
    static int loads=0;const auto groups=placementGroups({"piano","lead","insert"},{{"lead","insert"}},"scenario-"+std::to_string(++loads)+"/");
    auto add=[&](const char* node,const char* id,bool report){
        if(report)setenv("OSCAIRN_FIXTURE_VOICE_BUDGET","1",1);else unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");
        const PlacementScope scope(groups.at(node));
        return id?g.add(fixture,id):g.add(fixture);};
    const auto piano=add("piano",nullptr,c!=Case::NoReport),lead=add("lead",nullptr,true);
    const NodeId insert=mode==Mode::Chain?add("insert",effect,false):NodeId{};
    for(const char* v:{"OSCAIRN_FIXTURE_VOICE_US","OSCAIRN_FIXTURE_CHANNEL_US","OSCAIRN_FIXTURE_SUSTAIN","OSCAIRN_FIXTURE_DECAY_S","OSCAIRN_FIXTURE_EFFECT_US","OSCAIRN_FIXTURE_VOICE_BUDGET","OSCAIRN_AUDIO_CORES"})unsetenv(v);
    for(uint32_t p=0;p<2;++p){NoteZone z;z.channels=uint16_t(1u<<p);z.part=p+1;z.priority=uint8_t(p?10:8);g.connectNotes(graphInput,p?lead:piano,{z});}
    g.connectAudio(piano,graphOutput);
    if(mode==Mode::Chain){g.connectAudio(lead,insert);g.connectAudio(insert,graphOutput);}else g.connectAudio(lead,graphOutput);
    g.commit();
    // The device's time: one core, a budget each lane is within (piano 450 us, lead 360) but not their sum (810).
    g.setVoiceBudget(device?700:-1);
    Seen seen;seen.apart=g.host(piano).lane()!=g.host(lead).lane() || mode==Mode::Plugin || mode==Mode::Chain;
    const uint32_t blocks=c==Case::Held?2000:400;
    std::exception_ptr error;
    std::thread([&]{
        worker::makeRealtime(frames/48000.0*1e6);
        try{
            std::vector<float> left(frames),right(frames);float* out[]{left.data(),right.data()};
            const auto period=std::chrono::microseconds(frames*1'000'000/48000);
            auto next=std::chrono::steady_clock::now();
            for(uint32_t b=0;b<blocks;++b){
                std::vector<Event> e;
                // The pedal down; 30 piano notes struck and let go, one each two blocks, softer as they go.
                if(b==10)e.push_back(Event::midi(0xb0,64,127));
                if(b>=20 && b<80 && b%2==0)e.push_back(Event::note(true,30+int(b-20)/2,.9-.01*(b-20)/2,0,0));
                if(b>=21 && b<81 && b%2==1)e.push_back(Event::note(false,30+int(b-21)/2,0,0,0));
                // The lead from block 100: its own lane over, three struck and let go first; then two keys held.
                const uint32_t first=c==Case::OwnLane?106:100;
                if(c==Case::OwnLane && b>=100 && b<106 && b%2==0){e.push_back(Event::note(true,90+int(b-100),.8,0,1));e.push_back(Event::note(false,90+int(b-100),0,1,1));}
                if(b==first || b==first+2)e.push_back(Event::note(true,80+int(b-first),.8,0,1));
                if(c==Case::Held && b>=200 && b%25==0){const int k=70+int(b/25%10);e.push_back(Event::note(true,k,.9,0,0));e.push_back(Event::note(false,k,0,2,0));}
                g.process(out,frames,nullptr,e.data(),uint32_t(e.size()));
                if(b==99)seen.pianoBefore=voices(g,piano);
                if(b>=first+10){seen.fewestPiano=std::min(seen.fewestPiano,voices(g,piano));seen.fewestLead=std::min(seen.fewestLead,voices(g,lead));}
                if(b+1==blocks){seen.pianoLeft=voices(g,piano);seen.leadLeft=voices(g,lead);} // the voice reports are the audio thread's
                next+=period;std::this_thread::sleep_until(next);
            }
        }catch(...){error=std::current_exception();}
    }).join();
    if(error)std::rethrow_exception(error);
    const auto cp=g.noteCost(piano),cl=g.noteCost(lead);
    seen.pianoGave=gave(cp);seen.leadGave=gave(cl);
    seen.heldOff=cp.heldOff+cl.heldOff;seen.mismatch=cp.importanceMismatch+cl.importanceMismatch;
    seen.pianoLost=cp.lostBlocks;seen.leadLost=cl.lostBlocks;seen.pianoDeadline=cp.stolenForDeadline;seen.leadDeadline=cl.stolenForDeadline;
    return seen;
}
}

int main(int argc,char** argv){try{
    const char* worker=std::getenv("OSCAIRN_PLUGIN_WORKER");
    if(argc<2 || !worker)return 2;
    std::printf("%-20s %-26s %7s %7s %7s %7s %6s %7s %5s\n","placement","case","piano","piano-","lead","lead-","silent","heldOff","lost");
    auto measure=[&](Mode m,Case c,const char* what){
        const Seen s=run(argv[1],worker,m,c);
        std::printf("%-20s %-26s %3u>%-3u %7llu %7u %7llu %6s %7llu %2llu/%-2llu\n",name(m),what,s.pianoBefore,s.pianoLeft,(unsigned long long)s.pianoGave,s.leadLeft,(unsigned long long)s.leadGave,
            s.fewestPiano==0 || s.fewestLead==0?"yes":"no",(unsigned long long)s.heldOff,(unsigned long long)s.pianoLost,(unsigned long long)s.leadLost);
        // Always: no part silent, every plugin kept to its caps.
        check(s.fewestPiano>=1 && s.fewestLead>=1,std::string(name(m))+", "+what+": no part silent");
        check(s.mismatch==0,std::string(name(m))+", "+what+": every plugin kept to its caps");
        return s;};
    // A claim a slow machine could fail by itself (its own lost blocks reach the guard's rules), checked only when the
    // guard counted no lost block and gave nothing up for the callback's time on the part it is about (developer guide,
    // Timing tests).
    auto premised=[&](bool premise,bool ok,const std::string& what){
        if(premise)check(ok,what);else std::printf("%s: the machine lost blocks on that part, or slowed it past its lane: not checked\n",what.c_str());};
    auto leadKept=[&](Mode m,const Seen& s,const char* what){
        check(s.pianoGave>=1,std::string(name(m))+", "+what+": the piano's voices give way");
        premised(s.calm(),s.leadGave==0 && s.leadLeft==2,std::string(name(m))+", "+what+": the lead keeps both keys, gives up nothing");};
    for(const Mode m:{Mode::PoolTogether,Mode::InProcess})
        leadKept(m,measure(m,Case::SameThread,"same thread over"),"same thread over");
    for(const Mode m:{Mode::Plugin,Mode::Chain,Mode::PoolApart}){
        const auto d=measure(m,Case::Device,"device out of core time");
        check(d.apart,std::string(name(m))+": the parts on lanes of their own");
        leadKept(m,d,"device out of core time");
        const auto o=measure(m,Case::OwnLane,"lead's own lane over");
        check(o.leadGave>=1,std::string(name(m))+": the lead gives up its own voices for its own lane");
        premised(o.pianoLost==0 && o.pianoDeadline==0 && o.pianoBefore==30,o.pianoGave==0,std::string(name(m))+": the piano, on another lane, loses nothing for the lead's");
        premised(o.leadLost==0,o.leadLeft>=2,std::string(name(m))+": the lead keeps its held keys");
    }
    leadKept(Mode::Plugin,measure(Mode::Plugin,Case::NoReport,"piano without the report"),"piano without the report");
    {
        const auto h=measure(Mode::PoolTogether,Case::Held,"2,000 blocks, new notes");
        // The first excess (some 12 voices) and at most two for each of the 72 new notes.
        premised(h.calm(),h.leadGave==0 && h.leadLeft==2 && h.pianoGave<=30+2*72,"held 2,000 blocks: the lead never touched, no flapping: "+std::to_string(h.pianoGave)+" piano voices given up");
    }
    std::printf(failures?"FAIL: %d\n":"ok: the owner's scenario in every placement\n",failures);
    return failures?1:0;
}catch(const std::exception& e){std::fprintf(stderr,"voice-scenario: %s\n",e.what());return 1;}}
