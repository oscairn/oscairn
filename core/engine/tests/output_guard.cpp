// SPDX-License-Identifier: AGPL-3.0-or-later
// The engine's output guard: a plugin's block holding 1e9 is not heard (faded out from the last good samples,
// nothing past +120 dBFS reaching the mix) and is counted on its node, while a loud block (15, +23.5 dBFS) and
// one carrying data (4,095) pass; the plugin plays on, a worker unrestarted. Many such blocks in a row show as a run. NaN and Inf
// never reach the mix either: the CLAP host stops the plugin at the first (kick-clap's "nonfinite audio faults
// and silences instance"), before the guard. In the engine's process and in a worker. Also the check's cost per
// block, on a copy of its loop. Usage: voice-fixture [worker].
#include "oscairn/graph.h"
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <vector>
using namespace oscairn;
static void check(bool ok,const std::string& why){if(!ok)throw std::runtime_error(why);}

namespace {
constexpr uint32_t frames=128;
struct Result { float worst=0,loud=0,data=0,after=0; bool finite=true; Graph::OutputGuard guard; uint64_t restarts=0; uint32_t longestRun=0; };
Result play(const char* plugin,const char* output,uint32_t blocks){
    setenv("OSCAIRN_FIXTURE_OUTPUT",output,1);
    Graph g(48000,frames);
    const auto a=g.add(plugin);
    unsetenv("OSCAIRN_FIXTURE_OUTPUT");
    g.connectNotes(graphInput,a);g.connectAudio(a,graphOutput);g.commit();
    std::vector<float> left(frames),right(frames);float* out[]{left.data(),right.data()};
    Result r;
    for(uint32_t block=0;block<blocks;++block){
        std::vector<kick::clap::Event> events;if(block==2)events.push_back(kick::clap::Event::note(true,60,1,0,0));
        g.process(out,frames,nullptr,events.data(),uint32_t(events.size()));
        for(uint32_t i=0;i<frames;++i){
            r.finite=r.finite && std::isfinite(left[i]);
            if(block==60)r.loud=std::max(r.loud,std::abs(left[i]));else if(block==70)r.data=std::max(r.data,std::abs(left[i]));else r.worst=std::max(r.worst,std::abs(left[i]));
            if(block>=80)r.after=std::max(r.after,std::abs(left[i]));
        }
        r.longestRun=std::max(r.longestRun,g.outputGuard(a).inARow);
    }
    r.guard=g.outputGuard(a);r.restarts=g.host(a).statistics().restarts;
    return r;
}
}

int main(int argc,char** argv){try{
    if(argc<2)return 2;
    for(const bool inWorker:{false,true}){
        if(inWorker && argc<3)break;
        if(inWorker)setenv("OSCAIRN_PLUGIN_WORKER",argv[2],1);else unsetenv("OSCAIRN_PLUGIN_WORKER");
        const std::string where=inWorker?"in a worker":"in the engine";
        // Calls from 1: block b is call b+1. 1e9 in block 40, a loud 15 in block 60.
        const auto r=play(argv[1],"41:1e9,61:15,71:4095",100);
        std::printf("%s: %llu blocks not heard, loudest elsewhere %.3f, the loud block's peak %.1f, after them %.3f, restarts %llu\n",where.c_str(),
            (unsigned long long)r.guard.blocks,r.worst,r.loud,r.after,(unsigned long long)r.restarts);
        check(r.guard.blocks==1,where+": the 1e9 block not heard: "+std::to_string(r.guard.blocks));
        check(r.finite && r.worst<=1,where+": nothing past +120 dBFS reaches the mix: "+std::to_string(r.worst));
        check(std::abs(r.loud-15)<1e-3,where+": a legitimately loud block is heard: "+std::to_string(r.loud));
        check(std::abs(r.data-4095)<1e-3,where+": a block carrying data is heard: "+std::to_string(r.data));
        check(r.restarts==0 && r.guard.inARow==0 && r.after>.05,where+": the plugin plays on, unrestarted, its run over: "+std::to_string(r.after));
        // From block 20 on, 2.7e8 in every block for 200 (the runaway seen): a run of 200, the plugin not restarted.
        const auto s=play(argv[1],"21:2.7e8:200",260);
        check(s.longestRun==200 && s.guard.blocks==200 && s.restarts==0 && s.worst<=1,where+": 200 runaway blocks in a row show as a run of 200: "+std::to_string(s.longestRun));
        // NaN and Inf: stopped by the CLAP host before the guard; nothing not finite reaches the mix.
        for(const char* bad:{"21:nan","21:inf"}){
            const auto x=play(argv[1],bad,60);
            check(x.finite && x.worst<=1,where+": "+bad+": nothing not finite reaches the mix");
        }
    }
    // The check's cost: its loop on a stereo 128-frame block, as the graph runs it.
    std::vector<float> l(frames,.5f),r(frames,-.5f);const float* ch[]{l.data(),r.data()};
    constexpr int n=2'000'000;uint32_t seen=0;
    const auto t0=std::chrono::steady_clock::now();
    for(int k=0;k<n;++k){
        uint32_t bad=0;
        for(const float* x:ch)for(uint32_t i=0;i<frames;++i)bad|=uint32_t(!(std::fabs(x[i])<=Graph::outputBound));
        seen+=bad;l[k&(frames-1)]=float(k&1)*.25f; // the next block differs, so the loop is not hoisted
        asm volatile(""::"r"(seen):"memory");
    }
    const double ns=std::chrono::duration<double,std::nano>(std::chrono::steady_clock::now()-t0).count()/n;
    std::printf("the check: %.1f ns a stereo 128-frame block\n",ns);
    check(seen==0,"no clean block flagged");
    return 0;
}catch(const std::exception& e){std::fprintf(stderr,"output-guard: %s\n",e.what());return 1;}}
