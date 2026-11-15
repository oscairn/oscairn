// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045's no-glitch guard: notes give way before blocks are lost, whatever would lose them.
//   losses: an instrument (100 us a voice, eight notes, within its own lane's limit) into an effect in a worker of
//     its own whose block grows with its input's level, past its allowance: the effect's blocks are lost, and the
//     instrument upstream gives notes up until they are not.
//   chain: an instrument (90 us a voice, ten notes) into an effect of 700 us into a bus effect of 600 us, each in a
//     worker of its own, waiting to the callback's deadline: the instrument's lane leaves room for the chain after
//     it, so its notes give way though each lane is within its own time.
//   asleep: the chain's effects asleep on silence after the notes end: the instrument's lane still leaves room for
//     them at their cost awake, so the first notes into it are costed before they wake.
//   burst: eight notes in one block on an idle lane into a plugin giving up its own voices (oscairn.voice-budget), its
//     cost per voice learned (seeded) so the eight fit the lane with little room left: all eight sound; spaced, the
//     same; with the fixture really too slow for eight, its limit falls until they fit. A plugin without the
//     extension: the eight sound, none given up.
//   limit-first: the losses case with the instrument giving up its own voices: its limit is lowered and it gives up
//     its own, the host's notes untouched; one that ignores its limit has notes faded from outside after it.
//   starved: two parts giving up their own voices, each well within its own lane, sending to a bus effect in a worker
//     of its own that loses blocks (load not of their making): voices go within a few blocks of the first loss,
//     off the parts' limits, and keep going while losses go on; a limit lowered does not grow straight back with the
//     time the voices given up free, to bring the losses back (RFC 0045, 12k).
//   deadline: instruments in the engine's process, each well within its lane, while the callback reaches its deadline
//     after each block (load from outside the audio): notes give way across the lanes; with a heavy but healthy block
//     (little left, the default threshold 0) or the whole time to spare, none; with a profile's threshold of 900 us
//     (OSCAIRN_DEADLINE_SLACK_US), that little left is too little and notes give way.
// Usage: voice-fixture [--no-timing|--timing] [losses|chain|asleep|deadline|burst|limit-first|starved] (OSCAIRN_PLUGIN_WORKER set). --no-timing checks only that
// notes give way; a name runs that case alone.
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
static void check(bool ok,const std::string& why){if(!ok)throw std::runtime_error(why);}

namespace {
constexpr uint32_t frames=128;
const auto period=std::chrono::microseconds(frames*1'000'000/48000);
const auto deadline=std::chrono::nanoseconds(int64_t(double(std::chrono::nanoseconds(period).count())*(1-worker::mixingReserve)));
uint64_t gaveUp(const Graph& g,NodeId n){const auto c=g.noteCost(n);return c.stolen+c.stolenForDevice+c.hardChokes+c.stolenForLoss+c.stolenForDeadline;}
const char* const effect="org.oscairn.test.costly-effect";

// Plays `blocks` blocks on a real-time thread paced at the period, `notes` first; slack: the callback's time left
// after its block (its deadline set so), or none for a host without one.
template<class Notes> void play(Graph& g,uint32_t blocks,Notes&& notes,std::chrono::microseconds slack=std::chrono::microseconds(-1)){
    std::exception_ptr error;
    std::thread([&]{
        worker::makeRealtime(frames/48000.0*1e6);
        try{
            std::vector<float> left(frames),right(frames);float* out[]{left.data(),right.data()};
            auto next=std::chrono::steady_clock::now();
            for(uint32_t block=0;block<blocks;++block){
                const auto events=notes(block);
                const auto start=std::chrono::steady_clock::now();
                if(slack.count()>=0)worker::callbackDeadline=start+slack;
                else if(slack.count()==-2)worker::callbackDeadline=start+deadline;
                g.process(out,frames,nullptr,events.data(),uint32_t(events.size()));
                worker::callbackDeadline={};
                next+=period;std::this_thread::sleep_until(next);
            }
        }catch(...){error=std::current_exception();}
    }).join();
    if(error)std::rethrow_exception(error);
}
std::vector<Event> held(uint32_t block,uint32_t count,int channel=0){
    std::vector<Event> e;
    if(block>=20 && block<20+2*count && block%2==0)e.push_back(Event::note(true,48+int(block-20)/2*3,1,0,channel));
    return e;
}

struct Losses { uint64_t gaveUp=0,lostEarly=0,lostLate=0,restarts=0; };
Losses losses(const char* plugin){
    setenv("OSCAIRN_FIXTURE_VOICE_US","100",1);
    Graph g(48000,frames);
    const auto instrument=g.add(plugin);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","1700",1);setenv("OSCAIRN_FIXTURE_EFFECT_LEVEL","0.12",1);
    const auto fx=g.add(plugin,effect); // its settings stay for a worker restarted
    g.connectNotes(graphInput,instrument);g.connectAudio(instrument,fx);g.connectAudio(fx,graphOutput);
    g.commit();g.setVoiceBudget(-1);
    Losses r;uint64_t at200=0,at500=0;
    play(g,800,[&](uint32_t block){
        if(block==200)at200=g.host(fx).statistics().deadlineMisses;
        if(block==500)at500=g.host(fx).statistics().deadlineMisses;
        return held(block,8);});
    r.gaveUp=gaveUp(g,instrument);r.lostEarly=at200;r.lostLate=g.host(fx).statistics().deadlineMisses-at500;r.restarts=g.host(fx).statistics().restarts;
    unsetenv("OSCAIRN_FIXTURE_EFFECT_US");unsetenv("OSCAIRN_FIXTURE_EFFECT_LEVEL");
    return r;
}

struct Chain { uint64_t gaveUp=0; double limit=0; };
Chain chain(const char* plugin){
    setenv("OSCAIRN_WORKER_DEADLINE","1",1);setenv("OSCAIRN_FIXTURE_VOICE_US","90",1);
    Graph g(48000,frames);
    const auto instrument=g.add(plugin);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","700",1);const auto insert=g.add(plugin,effect);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","600",1);const auto bus=g.add(plugin,effect);
    unsetenv("OSCAIRN_FIXTURE_EFFECT_US");unsetenv("OSCAIRN_WORKER_DEADLINE"); // shortcut: a worker restarted would lose its cost; the case needs none
    g.connectNotes(graphInput,instrument);g.connectAudio(instrument,insert);g.connectAudio(insert,bus);g.connectAudio(bus,graphOutput);
    g.commit();g.setVoiceBudget(-1);
    play(g,600,[&](uint32_t block){return held(block,10);},std::chrono::microseconds(-2));
    return {gaveUp(g,instrument),g.noteCost(instrument).laneLimit};
}

// Eight notes from silence into one in-process instrument (its cost per voice seeded at `seed` us, really `real`),
// all in one block or a block apart: the plugin's own voices sounding 40 blocks later, and the host's notes given up.
struct Burst { uint32_t voices=0; uint64_t gaveUp=0; };
Burst burst(const char* plugin,bool own,double seed,double real,bool spaced){
    const std::string worker=std::getenv("OSCAIRN_PLUGIN_WORKER")?std::getenv("OSCAIRN_PLUGIN_WORKER"):"";
    unsetenv("OSCAIRN_PLUGIN_WORKER");
    setenv("OSCAIRN_FIXTURE_VOICE_US",std::to_string(real).c_str(),1);if(own)setenv("OSCAIRN_FIXTURE_VOICE_BUDGET","1",1);
    setenv("OSCAIRN_FIXTURE_REPORT_US",std::to_string(seed).c_str(),1); // the cost it reports per voice: the learned one
    Graph g(48000,frames);
    const auto a=g.add(plugin);
    unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");unsetenv("OSCAIRN_FIXTURE_REPORT_US");if(!worker.empty())setenv("OSCAIRN_PLUGIN_WORKER",worker.c_str(),1);
    g.connectNotes(graphInput,a);g.connectAudio(a,graphOutput);
    g.commit();g.setVoiceBudget(-1);g.seedNoteCost(a,20,seed);
    std::vector<float> left(frames),right(frames);float* out[]{left.data(),right.data()};
    for(uint32_t block=0;block<(spaced?200u:60u);++block){
        std::vector<Event> e;
        for(int i=0;i<8;++i)if(block==10+(spaced?uint32_t(i)*20:0))e.push_back(Event::note(true,48+i*2,1,0,0));
        g.process(out,frames,nullptr,e.data(),uint32_t(e.size()));
    }
    double cost;const oscairn_voice_budget_note_t* notes;uint32_t count;
    return {own?g.host(a).voiceReport(cost,notes,count):g.sounding(a),gaveUp(g,a)};
}

// The losses case with the instrument giving up its own voices (ignoring its limit, or not): its notes the host
// still holds, the voices it sounds, and the notes faded from outside.
struct Limited { uint32_t held=0,voices=0; uint64_t faded=0; };
Limited limited(const char* plugin,bool ignore){
    setenv("OSCAIRN_FIXTURE_VOICE_US","100",1);setenv("OSCAIRN_FIXTURE_VOICE_BUDGET","1",1);if(ignore)setenv("OSCAIRN_FIXTURE_IGNORE_LIMIT","1",1);
    Graph g(48000,frames);
    const auto instrument=g.add(plugin);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","1700",1);setenv("OSCAIRN_FIXTURE_EFFECT_LEVEL","0.12",1);
    const auto fx=g.add(plugin,effect);
    g.connectNotes(graphInput,instrument);g.connectAudio(instrument,fx);g.connectAudio(fx,graphOutput);
    g.commit();g.setVoiceBudget(-1);
    play(g,800,[&](uint32_t block){return held(block,8);});
    unsetenv("OSCAIRN_FIXTURE_EFFECT_US");unsetenv("OSCAIRN_FIXTURE_EFFECT_LEVEL");unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");unsetenv("OSCAIRN_FIXTURE_IGNORE_LIMIT");
    double cost;const oscairn_voice_budget_note_t* notes;uint32_t count;
    const auto c=g.noteCost(instrument);
    return {g.sounding(instrument),g.host(instrument).voiceReport(cost,notes,count),c.stolen+c.stolenForLoss+c.hardChokes+c.stolenForDeadline};
}

// Two own-voices parts into a bus effect losing blocks: the block of its first loss, the voices taken off the parts'
// limits by then plus 10 blocks, and the effect's losses from block 500 to 800.
struct Starved { uint32_t first=0; uint64_t early=0,lateLost=0; };
Starved starved(const char* plugin){
    setenv("OSCAIRN_FIXTURE_VOICE_US","40",1);setenv("OSCAIRN_FIXTURE_VOICE_BUDGET","1",1);
    Graph g(48000,frames);
    const auto a=g.add(plugin),b=g.add(plugin);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","1600",1);setenv("OSCAIRN_FIXTURE_EFFECT_LEVEL","0.4",1);
    const auto bus=g.add(plugin,effect);
    for(uint32_t p=0;p<2;++p){const auto n=p?b:a;NoteZone z;z.channels=uint16_t(1u<<p);z.part=p+1;g.connectNotes(graphInput,n,{z});g.connectAudio(n,bus);}
    g.connectAudio(bus,graphOutput);
    g.commit();g.setVoiceBudget(-1);
    Starved r;uint64_t at500=0,mark=0;
    play(g,800,[&](uint32_t block){
        const uint64_t lost=g.host(bus).statistics().deadlineMisses;
        if(!r.first && lost)r.first=block;
        if(r.first && block==r.first+10)r.early=g.noteCost(a).stolenForLoss+g.noteCost(b).stolenForLoss+g.noteCost(a).stolenForDeadline+g.noteCost(b).stolenForDeadline;
        if(block==500)at500=lost;
        mark=lost;
        auto e=held(block,8,0);for(auto& x:held(block,8,1))e.push_back(x);return e;});
    unsetenv("OSCAIRN_FIXTURE_EFFECT_US");unsetenv("OSCAIRN_FIXTURE_EFFECT_LEVEL");unsetenv("OSCAIRN_FIXTURE_VOICE_BUDGET");
    r.lateLost=mark-at500;
    return r;
}

// The chain's case, its notes released and the effects asleep: the instrument lane's limit then.
double asleep(const char* plugin){
    setenv("OSCAIRN_WORKER_DEADLINE","1",1);setenv("OSCAIRN_FIXTURE_VOICE_US","90",1);
    Graph g(48000,frames);
    const auto instrument=g.add(plugin);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","700",1);const auto insert=g.add(plugin,effect);
    setenv("OSCAIRN_FIXTURE_EFFECT_US","600",1);const auto bus=g.add(plugin,effect);
    unsetenv("OSCAIRN_FIXTURE_EFFECT_US");unsetenv("OSCAIRN_WORKER_DEADLINE");
    g.connectNotes(graphInput,instrument);g.connectAudio(instrument,insert);g.connectAudio(insert,bus);g.connectAudio(bus,graphOutput);
    g.commit();g.setVoiceBudget(-1);
    play(g,1200,[&](uint32_t block){
        if(block==200){std::vector<Event> e;for(int i=0;i<3;++i)e.push_back(Event::note(false,48+i*3,0,0,0));return e;}
        return held(block,3);},std::chrono::microseconds(-2));
    return g.noteCost(instrument).laneLimit;
}

struct Deadline { uint64_t past=0,heavy=0,ample=0,raised=0; };
Deadline late(const char* plugin){
    const std::string worker=std::getenv("OSCAIRN_PLUGIN_WORKER")?std::getenv("OSCAIRN_PLUGIN_WORKER"):"";
    unsetenv("OSCAIRN_PLUGIN_WORKER"); // in the engine's process: one lane, within its time
    setenv("OSCAIRN_FIXTURE_VOICE_US","20",1);
    Deadline r;
    for(int run=0;run<4;++run){
        if(run==3)setenv("OSCAIRN_DEADLINE_SLACK_US","900",1);
        Graph g(48000,frames);
        unsetenv("OSCAIRN_DEADLINE_SLACK_US");
        const auto a=g.add(plugin),b=g.add(plugin);
        for(uint32_t p=0;p<2;++p){NoteZone z;z.channels=uint16_t(1u<<p);z.part=p+1;g.connectNotes(graphInput,p?b:a,{z});g.connectAudio(p?b:a,graphOutput);}
        g.commit();g.setVoiceBudget(-1);
        play(g,400,[&](uint32_t block){auto e=held(block,6,0);for(auto& x:held(block,6,1))e.push_back(x);return e;},
             run==0?std::chrono::microseconds(0):run==2?std::chrono::duration_cast<std::chrono::microseconds>(deadline):std::chrono::microseconds(1000));
        (run==0?r.past:run==1?r.heavy:run==2?r.ample:r.raised)=gaveUp(g,a)+gaveUp(g,b);
    }
    if(!worker.empty())setenv("OSCAIRN_PLUGIN_WORKER",worker.c_str(),1);
    return r;
}
}

int main(int argc,char** argv){try{
    if(argc<2)return 2;
    const bool timing=!(argc>2 && !std::strcmp(argv[2],"--no-timing"));
    const std::string only=argc>3?argv[3]:"";
    if(only.empty() || only=="losses"){
    const auto l=losses(argv[1]);
    std::printf("losses: the instrument gave up %llu notes; the effect lost %llu blocks by block 200, %llu from block 500 to 800, was restarted %llu times\n",
        (unsigned long long)l.gaveUp,(unsigned long long)l.lostEarly,(unsigned long long)l.lostLate,(unsigned long long)l.restarts);
    check(l.gaveUp>0,"blocks lost after an instrument give its notes up");
    if(timing)check(l.lostLate<=5,"once its notes have given way, the effect after it loses (almost) no block: "+std::to_string(l.lostLate));
    }
    if(only.empty() || only=="chain"){
    const auto c=chain(argv[1]);
    std::printf("chain: the instrument gave up %llu notes, its lane's limit %.0f us\n",(unsigned long long)c.gaveUp,c.limit);
    check(c.gaveUp>0,"an instrument's notes give way for the chain after it");
    check(c.limit>0 && c.limit<.8*(std::chrono::duration<double,std::micro>(deadline).count()-1300)+50,"its lane's limit leaves room for the chain after it: "+std::to_string(c.limit));
    }
    if(only.empty() || only=="asleep"){
    const double limit=asleep(argv[1]);
    std::printf("asleep: the instrument lane's limit with the chain after it asleep %.0f us\n",limit);
    check(limit>0 && limit<.8*(std::chrono::duration<double,std::micro>(deadline).count()-1300)+50,"an effect asleep counts at its cost awake: "+std::to_string(limit));
    }
    if(only.empty() || only=="burst"){
    const auto whole=burst(argv[1],true,150,10,false),spaced=burst(argv[1],true,150,10,true),slow=burst(argv[1],true,150,300,false),plain=burst(argv[1],false,150,10,false);
    std::printf("burst: eight notes in a block %u voices, spaced %u, really too slow %u; without the extension %u notes sound, %llu given up\n",
        whole.voices,spaced.voices,slow.voices,plain.voices,(unsigned long long)plain.gaveUp);
    check(whole.voices==8,"a chord from silence sounds whole where the lane's time allows it: "+std::to_string(whole.voices));
    check(spaced.voices==8,"spaced notes the same: "+std::to_string(spaced.voices));
    check(slow.voices>0 && slow.voices<8,"really too slow, its limit falls: "+std::to_string(slow.voices));
    check(plain.voices==8 && plain.gaveUp==0,"a plugin without the extension: every note sounds");
    }
    if(only.empty() || only=="limit-first"){
    const auto own=limited(argv[1],false),ignoring=limited(argv[1],true);
    std::printf("limit-first: keeping to its limit, the host holds %u notes, it sounds %u voices, %llu given up by the guard; ignoring it: %u, %u, %llu\n",
        own.held,own.voices,(unsigned long long)own.faded,ignoring.held,ignoring.voices,(unsigned long long)ignoring.faded);
    // The host fades a note of one keeping to its limit only once that limit is down to a voice and losses go on.
    check(own.voices<own.held && own.held>=6,"lost blocks lower the plugin's limit: it gives up its own voices first, the host's notes kept");
    check(ignoring.held+4<=own.held,"a plugin that ignores its limit has notes faded from outside");
    }
    if(only.empty() || only=="starved"){
    const auto st=starved(argv[1]);
    std::printf("starved: the bus effect's first lost block %u, voices given up within 10 blocks of it %llu, blocks lost from 500 to 800 %llu\n",st.first,(unsigned long long)st.early,(unsigned long long)st.lateLost);
    check(st.first>0,"the bus effect loses blocks");
    check(st.early>0,"voices go within a few blocks of the first loss");
    if(timing)check(st.lateLost<=5,"and keep going until losses stop: "+std::to_string(st.lateLost));
    }
    if(only.empty() || only=="deadline"){
    const auto d=late(argv[1]);
    std::printf("deadline: notes given up with the deadline reached %llu, with little left %llu, with the whole time %llu, with little left and a 900 us threshold %llu\n",
        (unsigned long long)d.past,(unsigned long long)d.heavy,(unsigned long long)d.ample,(unsigned long long)d.raised);
    check(d.past>0,"notes give way when the callback reaches its deadline");
    check(d.heavy==0 && d.ample==0,"none while it keeps within it, however little is left");
    check(d.raised>0,"a profile's threshold gives notes up with that little left");
    }
    return 0;
}catch(const std::exception& e){std::fprintf(stderr,"guard: %s\n",e.what());return 1;}}
