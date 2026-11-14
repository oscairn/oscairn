// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0045's load guard: with no budget set the graph still gives notes up before a worker's block
// is missed. Two parts, each a voice fixture in a worker of its own (each a lane), on a real-time
// thread paced at the period: part A holds more notes than its worker's allowance has time for,
// part B a few. Without the guard A's worker misses blocks or is restarted as too slow (its notes
// lost); with it A's notes give way (by the budget's order), A's worker is not restarted, misses
// (almost) none once the first are given up and its blocks fit the allowance, and B, on its own
// lane, loses nothing. The same with a CPU budget set as the standalone app's profiles set it (75% of a
// block, more than the allowance): the allowance still bounds the lane. Usage: voice-fixture
// [--no-timing] (OSCAIRN_PLUGIN_WORKER set, OSCAIRN_FIXTURE_VOICE_US the cost of a voice,
// OSCAIRN_WORKER_BUDGET_US the allowance). --no-timing checks only that notes give way where they should.
#include "oscairn/graph.h"
#include "oscairn/realtime.h"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <stdexcept>
#include <string>
#include <thread>
#include <vector>
using namespace oscairn;
using kick::clap::Event;
static void check(bool ok,const std::string& why){if(!ok)throw std::runtime_error(why);}

namespace {
constexpr uint32_t frames=128,blocks=600,settle=300; // misses counted from block `settle` on
struct Result { uint64_t misses=0,stolen=0,chokes=0,bStolen=0,restarts=0; double last=0; };
Result play(const char* plugin,double budget){
    Graph g(48000,frames);
    const auto a=g.add(plugin),b=g.add(plugin);
    for(uint32_t p=0;p<2;++p){NoteZone z;z.channels=uint16_t(1u<<p);z.part=p+1;g.connectNotes(graphInput,p?b:a,{z});g.connectAudio(p?b:a,graphOutput);}
    g.commit();g.setVoiceBudget(budget);
    std::vector<float> left(frames),right(frames);float* out[]{left.data(),right.data()};
    const auto period=std::chrono::microseconds(frames*1'000'000/48000);
    auto next=std::chrono::steady_clock::now();uint64_t before=0;
    for(uint32_t block=0;block<blocks;++block){
        std::vector<Event> events;
        if(block>=20 && block<60 && block%2==0)events.push_back(Event::note(true,40+int(block/2),block<40?.3:1,0,0)); // A: 20 notes, the first ten quiet
        if(block==21 || block==23)events.push_back(Event::note(true,70+int(block),1,0,1));                           // B: two
        g.process(out,frames,nullptr,events.data(),uint32_t(events.size()));
        if(block==settle)before=g.host(a).statistics().deadlineMisses;
        next+=period;std::this_thread::sleep_until(next);
    }
    Result r;r.misses=g.host(a).statistics().deadlineMisses-before;r.restarts=g.host(a).statistics().restarts;r.last=g.host(a).statistics().lastMicroseconds;
    const auto c=g.noteCost(a);r.stolen=c.stolen;r.chokes=c.hardChokes;r.bStolen=g.noteCost(b).stolen+g.noteCost(b).hardChokes;
    return r;
}
}

int main(int argc,char** argv){try{
    if(argc<2)return 2;
    const bool timing=!(argc>2 && !std::strcmp(argv[2],"--no-timing"));
    auto render=[&](double budget){
        Result r;std::exception_ptr error;
        std::thread([&]{worker::makeRealtime(frames/48000.0*1e6);try{r=play(argv[1],budget);}catch(...){error=std::current_exception();}}).join();
        if(error)std::rethrow_exception(error);
        return r;
    };
    const auto open=render(0),guarded=render(-1),budgeted=render(.75*frames/48000.0*1e6); // instrument reliability's budget, 2,000 us
    for(const auto* r:{&open,&guarded,&budgeted})
        std::printf("%s: A missed %llu blocks after block %u, restarted %llu times, last block %.0f us; A gave up %llu notes (%llu choked), B %llu\n",r==&open?"no guard":r==&guarded?"guard":"guard, 75% budget",(unsigned long long)r->misses,settle,(unsigned long long)r->restarts,r->last,
            (unsigned long long)r->stolen,(unsigned long long)r->chokes,(unsigned long long)r->bStolen);
    if(timing)check(open.misses>50 || open.restarts>0,"without the guard A's worker misses blocks, or is restarted as too slow and its notes are lost");
    for(const auto* r:{&guarded,&budgeted}){
        const std::string with=r==&guarded?"with the guard":"with a 75% budget";
        check(r->stolen+r->chokes>0,with+" A's notes give way");
        check(r->bStolen==0,with+" B's notes, on a lane of their own, are not given up for A's");
        if(!timing)continue;
        check(r->restarts==0 && r->misses<=5,with+" A's worker is never restarted and misses (almost) no block once its notes have given way: "+std::to_string(r->restarts)+" restarts, "+std::to_string(r->misses)+" missed");
        check(r->last>0 && r->last<=1000,with+" A's blocks fit its allowance: "+std::to_string(r->last)+" us");
    }
    return 0;
}catch(const std::exception& e){std::fprintf(stderr,"load-guard: %s\n",e.what());return 1;}}
