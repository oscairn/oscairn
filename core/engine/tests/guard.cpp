// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045's no-glitch guard: notes give way before blocks are lost, whatever would lose them.
//   losses: an instrument (100 us a voice, eight notes, within its own lane's limit) into an effect in a worker of
//     its own whose block grows with its input's level, past its allowance: the effect's blocks are lost, and the
//     instrument upstream gives notes up until they are not.
//   chain: an instrument (90 us a voice, ten notes) into an effect of 700 us into a bus effect of 600 us, each in a
//     worker of its own, waiting to the callback's deadline: the instrument's lane leaves room for the chain after
//     it, so its notes give way though each lane is within its own time.
//   deadline: instruments in the engine's process, each well within its lane, while the callback has almost no time
//     left after its block (load from outside the audio): notes give way across the lanes; with time to spare, none.
// Usage: voice-fixture [--no-timing|--timing] [losses|chain|deadline] (OSCAIRN_PLUGIN_WORKER set). --no-timing checks only that
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
uint64_t gaveUp(const Graph& g,NodeId n){const auto c=g.noteCost(n);return c.stolen+c.stolenForDevice+c.hardChokes+c.stolenForLoss;}
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

struct Deadline { uint64_t tight=0,ample=0; };
Deadline late(const char* plugin){
    const std::string worker=std::getenv("OSCAIRN_PLUGIN_WORKER")?std::getenv("OSCAIRN_PLUGIN_WORKER"):"";
    unsetenv("OSCAIRN_PLUGIN_WORKER"); // in the engine's process: one lane, within its time
    setenv("OSCAIRN_FIXTURE_VOICE_US","20",1);
    Deadline r;
    for(const bool tight:{true,false}){
        Graph g(48000,frames);
        const auto a=g.add(plugin),b=g.add(plugin);
        for(uint32_t p=0;p<2;++p){NoteZone z;z.channels=uint16_t(1u<<p);z.part=p+1;g.connectNotes(graphInput,p?b:a,{z});g.connectAudio(p?b:a,graphOutput);}
        g.commit();g.setVoiceBudget(-1);
        play(g,400,[&](uint32_t block){auto e=held(block,6,0);for(auto& x:held(block,6,1))e.push_back(x);return e;},
             tight?std::chrono::microseconds(100):std::chrono::duration_cast<std::chrono::microseconds>(deadline));
        (tight?r.tight:r.ample)=gaveUp(g,a)+gaveUp(g,b);
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
    if(only.empty() || only=="deadline"){
    const auto d=late(argv[1]);
    std::printf("deadline: notes given up with 100 us left after each block %llu, with the callback's whole time %llu\n",(unsigned long long)d.tight,(unsigned long long)d.ample);
    check(d.tight>0,"notes give way when the callback's time runs out");
    check(d.ample==0,"none while it has time to spare");
    }
    return 0;
}catch(const std::exception& e){std::fprintf(stderr,"guard: %s\n",e.what());return 1;}}
