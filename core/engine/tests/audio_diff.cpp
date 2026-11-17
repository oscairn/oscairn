// SPDX-License-Identifier: AGPL-3.0-or-later
// The differential test of a plan's audio (engine_core.h's audio_*): the Rust implementation, driven
// through its C interface by random plans and blocks, and compared bit for bit after every call with
// the outputs its C++ reference (graph.cpp's mixing, output guard and meters as they were) gave on the
// same streams (vectors.h, core/engine/tests/vectors/engine-audio.vec; samples with every NaN alike):
// each step's input buffers after its mix, its output buffers after the guard, its and its buses'
// meters and activity counters, and the graph's outputs. Plans change under it with their gains
// carried over (a played change); live gains move and jump. The plugins are modeled: outputs from
// their inputs and noise, now and then a NaN, an infinity, a sample past outputBound, silence, a
// block not rendered, its inputs written over (working in place).
//   oscairn-audio-diff-test replay FILE [plans] [blocks]
//   oscairn-audio-diff-test record FILE COMMIT [plans] [blocks]   new vectors from the Rust core (docs/developer-guide.md)
//   oscairn-audio-diff-test bench [blocks]                        a 16-part Performance's audio, ns a block
#include "../src/engine_core.h"
#include "vectors.h"
#include "oscairn/graph.h"
#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <new>
#include <vector>

namespace
{
constexpr uint32_t maxFrames=128,hosts=32,maxBuses=3;

struct Rng
{
    uint64_t s;
    uint64_t next() { s^=s<<13;s^=s>>7;s^=s<<17;return s; }
    uint32_t below(uint32_t n) { return uint32_t(next()%n); }
    float unit() { return float(next()>>40)/float(1u<<24); }
};

// An activity at a fixed address, its padding zeroed, so two can be compared byte for byte.
struct Activity
{
    alignas(oscairn::Graph::Activity) unsigned char bytes[sizeof(oscairn::Graph::Activity)]{};
    Activity() { new(bytes) oscairn::Graph::Activity; }
    oscairn_activity* core() { return reinterpret_cast<oscairn_activity*>(bytes); }
};

// A plan: steps by plugin (host), their channels, meters, activities, buses and connections.
struct Plan
{
    uint32_t inputs=0,channels=2;
    struct Step { uint32_t host,ins,outs,metered; bool peaks,activity; std::vector<oscairn_audio_route> routes; std::vector<std::pair<uint32_t,uint32_t>> buses; };
    std::vector<Step> steps;
    std::vector<oscairn_audio_route> outputs;
};

// The plan's audio, and each plugin's meters and activities (kept across plans, as a node's).
struct Side
{
    oscairn_audio* audio=nullptr;
    std::array<Activity,hosts*(1+maxBuses)> activities;
    std::array<oscairn_peaks,hosts*(1+maxBuses)> peaks{};
    std::vector<float> outputs;
    ~Side() { if(audio)rust_core_audio_free(audio); }
    void build(const Plan& plan,uint32_t ramp)
    {
        std::vector<std::vector<oscairn_bus>> buses(plan.steps.size());
        std::vector<oscairn_audio_step> steps;
        for(size_t i=0;i<plan.steps.size();++i)
        {
            const auto& s=plan.steps[i];
            for(uint32_t b=0;b<s.buses.size();++b){const uint32_t k=hosts*(1+b)+s.host;buses[i].push_back({s.buses[b].first,s.buses[b].second,&peaks[k],activities[k].core()});}
            steps.push_back({reinterpret_cast<const void*>(uintptr_t(s.host+1)),s.ins,s.outs,s.metered,s.peaks?&peaks[s.host]:nullptr,s.activity?activities[s.host].core():nullptr,
                             s.routes.data(),uint32_t(s.routes.size()),buses[i].data(),uint32_t(buses[i].size())});
        }
        auto* next=rust_core_audio_new(steps.data(),uint32_t(steps.size()),plan.outputs.data(),uint32_t(plan.outputs.size()),plan.inputs,plan.channels,maxFrames);
        if(audio){if(ramp)rust_core_audio_carry(next,audio,ramp);rust_core_audio_free(audio);}
        audio=next;
    }
};

oscairn_audio_route connection(Rng& rng,const Plan& plan,size_t before,uint32_t to)
{
    oscairn_audio_route r{-1,0,to,1.f,{}};
    std::vector<int32_t> sources;
    if(plan.inputs)sources.push_back(-1);
    for(size_t i=0;i<before;++i)if(plan.steps[i].outs)sources.push_back(int32_t(i));
    if(sources.empty())return {-2};
    r.source=sources[rng.below(uint32_t(sources.size()))];
    r.from=rng.below(r.source<0?plan.inputs:plan.steps[size_t(r.source)].outs);
    const float gains[]{1.f,1.f,.5f,-.7f,0.f,2.5f,rng.unit()};r.gain=gains[rng.below(7)];
    if(!rng.below(3)){r.live[0]=uint8_t(1+rng.below(7));for(int k=1;k<4;++k)r.live[k]=uint8_t(rng.below(3)?0:1+rng.below(7));}
    return r;
}
void connect(Rng& rng,Plan& plan,size_t i)
{
    auto& s=plan.steps[i];
    s.routes.clear();
    for(uint32_t n=s.ins?rng.below(2*s.ins+1):0;n--;)if(auto r=connection(rng,plan,i,rng.below(s.ins));r.source>-2)s.routes.push_back(r);
}
Plan randomPlan(Rng& rng)
{
    Plan plan;plan.inputs=rng.below(5);plan.channels=1+rng.below(8);
    std::array<uint32_t,hosts> ids;for(uint32_t h=0;h<hosts;++h)ids[h]=h;
    for(uint32_t h=hosts;h>1;--h)std::swap(ids[h-1],ids[rng.below(h)]);
    for(uint32_t i=0,n=1+rng.below(20);i<n;++i)
    {
        Plan::Step s{ids[i],rng.below(5),rng.below(7),0,rng.below(5)!=0,rng.below(6)!=0,{},{}};
        s.metered=s.outs-(s.outs>=4 && !rng.below(4)?4:0); // the ring-out channels, unmetered
        if(s.outs && !rng.below(4))for(uint32_t b=0,k=1+rng.below(maxBuses);b<k;++b){const uint32_t o=rng.below(s.outs);s.buses.push_back({o,1+rng.below(s.outs-o)});}
        plan.steps.push_back(s);connect(rng,plan,i);
    }
    for(uint32_t n=rng.below(3*plan.channels+1);n--;)if(auto r=connection(rng,plan,plan.steps.size(),rng.below(plan.channels));r.source>-2)plan.outputs.push_back(r);
    return plan;
}
// A played change: gains moved, connections redone here and there, the same plugins.
Plan change(Rng& rng,Plan plan)
{
    auto move=[&](std::vector<oscairn_audio_route>& routes){for(auto& r:routes)if(rng.below(2))r.gain=rng.below(4)?rng.unit()*2:r.gain;};
    for(size_t i=0;i<plan.steps.size();++i){if(!rng.below(6))connect(rng,plan,i);else move(plan.steps[i].routes);}
    move(plan.outputs);
    if(!rng.below(5))if(auto r=connection(rng,plan,plan.steps.size(),rng.below(plan.channels));r.source>-2)plan.outputs.push_back(r);
    return plan;
}

uint64_t compared=0;

void run(uint32_t seed,uint32_t blocks,uint64_t& total,vectors::Tape& tape)
{
    tape.plan(seed,blocks);
    auto see=[&](const void* p,size_t n){compared+=n;tape.see(p,n);};
    std::vector<uint32_t> scratch;
    auto samples=[&](const void* p,size_t n){see(vectors::canonical(p,n,scratch),n);};
    Rng rng{0x9e3779b97f4a7c15ull*(seed+1)};
    Side side;
    Plan plan=randomPlan(rng);
    side.build(plan,0);
    std::array<float,oscairn::Graph::liveGains> liveFrom,liveTo;liveFrom.fill(1.f);liveTo.fill(1.f);
    std::vector<std::vector<float>> inputs(6,std::vector<float>(maxFrames));
    std::vector<float> rendered(maxFrames);
    for(uint32_t block=0,changed=0;block<blocks;++block,++total)
    {
        if(block && (!rng.below(150) || (block-changed<4 && !rng.below(3)))) // now and then another while the last one's gains ramp
        {
            changed=block;
            const bool played=rng.below(4)!=0;
            plan=played?change(rng,plan):randomPlan(rng);
            const uint32_t ramp=played?1+rng.below(400):0;
            side.build(plan,ramp);
        }
        const uint32_t frames=rng.below(4)?maxFrames:1+rng.below(maxFrames);
        for(uint32_t slot=1;slot<8;++slot){liveFrom[slot]=liveTo[slot];if(!rng.below(5))liveTo[slot]=rng.below(5)?rng.unit():0.f;if(!rng.below(9))liveFrom[slot]=liveTo[slot];}
        const uint32_t ni=rng.below(plan.inputs+2),no=plan.channels+rng.below(3);
        std::vector<const float*> in(ni);
        for(uint32_t c=0;c<ni;++c){if(!rng.below(6))continue;for(auto& x:inputs[c])x=rng.unit()*2-1;in[c]=inputs[c].data();}
        const bool nullExtra=rng.below(2);
        std::vector<float*> outs;
        side.outputs.assign(size_t(no)*maxFrames,7.f); // what was there: silenced or mixed over
        for(uint32_t c=0;c<no;++c)outs.push_back(c>=plan.channels && nullExtra?nullptr:side.outputs.data()+size_t(c)*maxFrames);
        rust_core_audio_begin(side.audio,in.data(),ni,outs.data(),no,frames);
        for(uint32_t i=0;i<plan.steps.size();++i)
        {
            const auto& step=plan.steps[i];
            rust_core_audio_mix(side.audio,i,nullptr,frames,liveFrom.data(),liveTo.data());
            float* const* ins=rust_core_audio_buffers(side.audio,int32_t(i),false);float* const* out=rust_core_audio_buffers(side.audio,int32_t(i),true);
            for(uint32_t c=0;c<step.ins;++c)samples(ins[c],frames*sizeof(float));
            // The plugin: from its inputs and noise; now and then a bad sample, silence, a block not rendered.
            for(uint32_t c=0;c<step.outs;++c)
            {
                const uint32_t kind=rng.below(60);
                for(uint32_t f=0;f<frames;++f)rendered[f]=kind<8?(kind<4?0.f:kind<6?-1e-6f:1.5e-5f):(step.ins?ins[c%step.ins][f]*.5f:0.f)+rng.unit()-.5f;
                if(kind==8)rendered[rng.below(frames)]=NAN;
                if(kind==9)rendered[rng.below(frames)]=-INFINITY;
                if(kind==10)rendered[rng.below(frames)]=2e6f;
                std::memcpy(out[c],rendered.data(),frames*sizeof(float));
            }
            if(!rng.below(8))for(uint32_t c=0;c<step.ins;++c)std::fill_n(ins[c],frames,.125f); // a plugin working in place
            const bool processed=rng.below(12)!=0,asleep=!rng.below(3);
            const uint32_t fade=rng.below(4)?48:1+rng.below(300);
            rust_core_audio_finish(side.audio,i,frames,processed,asleep,fade);
            for(uint32_t c=0;c<step.outs;++c)samples(out[c],frames*sizeof(float));
            for(uint32_t b=0;b<=maxBuses;++b)
            {
                const uint32_t k=hosts*b+step.host;
                see(side.activities[k].bytes,sizeof(Activity));samples(side.peaks[k],sizeof(oscairn_peaks));
                if(!rng.below(7))std::memset(side.peaks[k],0,sizeof(oscairn_peaks)); // takePeaks
            }
        }
        rust_core_audio_mix(side.audio,uint32_t(plan.steps.size()),outs.data(),frames,liveFrom.data(),liveTo.data());
        for(uint32_t c=0;c<no;++c)if(outs[c])samples(outs[c],frames*sizeof(float));
        tape.block();
    }
}

// A 16-part Performance: each part's instrument into its chain, the chains into the master at the
// part's live volume and into two send buses, the buses into the master, the master out.
int bench(uint32_t blocks)
{
    Plan plan;plan.channels=2;
    auto route=[](int32_t source,uint32_t c,float gain,uint8_t live=0){return oscairn_audio_route{source,c,c,gain,{live,0,0,0}};};
    for(uint32_t p=0;p<16;++p)plan.steps.push_back({p,0,2,2,true,true,{},{}});
    for(uint32_t p=0;p<16;++p)plan.steps.push_back({16+p,2,2,2,true,true,{route(int32_t(p),0,1),route(int32_t(p),1,1)},{}});
    for(uint32_t b=0;b<2;++b){plan.steps.push_back({32+b,2,2,2,true,true,{},{}});for(uint32_t p=0;p<16;++p)for(uint32_t c=0;c<2;++c)plan.steps.back().routes.push_back(route(int32_t(16+p),c,b?.2f:.3f));}
    plan.steps.push_back({34,2,2,2,true,true,{},{}});
    for(uint32_t p=0;p<18;++p)for(uint32_t c=0;c<2;++c)plan.steps.back().routes.push_back(route(int32_t(16+p),c,p<16?.8f:1.f,p<16?uint8_t(1+p):0));
    plan.outputs={route(34,0,1),route(34,1,1)};
    Side side;side.build(plan,0);
    std::array<float,oscairn::Graph::liveGains> live;live.fill(1.f);
    std::vector<float> signal(maxFrames),out(2*maxFrames);
    for(uint32_t f=0;f<maxFrames;++f)signal[f]=std::sin(float(f)*.1f)*.5f;
    float* outs[2]{out.data(),out.data()+maxFrames};
    auto block=[&]{
        rust_core_audio_begin(side.audio,nullptr,0,outs,2,maxFrames);
        for(uint32_t i=0;i<plan.steps.size();++i)
        {
            rust_core_audio_mix(side.audio,i,nullptr,maxFrames,live.data(),live.data());
            float* const* o=rust_core_audio_buffers(side.audio,int32_t(i),true);
            for(uint32_t c=0;c<2;++c)std::memcpy(o[c],signal.data(),maxFrames*sizeof(float)); // the plugin
            rust_core_audio_finish(side.audio,i,maxFrames,true,false,48);
        }
        rust_core_audio_mix(side.audio,uint32_t(plan.steps.size()),outs,maxFrames,live.data(),live.data());};
    for(uint32_t b=0;b<1000;++b)block();
    std::vector<double> ns;
    for(uint32_t round=0;round<9;++round)
    {
        const auto t0=std::chrono::steady_clock::now();
        for(uint32_t b=0;b<blocks;++b)block();
        ns.push_back(std::chrono::duration<double,std::nano>(std::chrono::steady_clock::now()-t0).count()/blocks);
    }
    std::sort(ns.begin(),ns.end());
    std::printf("audio, %zu steps x 128 frames: median %.0f ns a block (min %.0f, max %.0f), out %g\n",plan.steps.size(),ns[4],ns[0],ns[8],double(out[5]));
    return 0;
}
}

int main(int argc,char** argv)
{
    if(argc>1 && !std::strcmp(argv[1],"bench"))return bench(argc>2?uint32_t(std::atoi(argv[2])):20000);
    vectors::Args args(argc,argv,200,2000,"engine-audio-diff","core/engine/tests/audio_diff.cpp (oscairn-audio-diff-test record)");
    uint64_t total=0;
    for(uint32_t p=0;p<args.plans;++p)run(p,args.blocks,total,args.tape);
    const char* outcome=args.done(0); // no exp, log1p or pow in a plan's audio
    std::printf("engine audio differential: %u plans, %llu blocks, %llu bytes compared: %s\n",args.plans,(unsigned long long)total,(unsigned long long)compared,outcome);
    return 0;
}
