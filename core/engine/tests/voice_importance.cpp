// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0046 step 2: oscairn.voice-budget draft 2's importance report from the SDK helper, in the Basic Synth (its
// sound-contexts Engine), in process and, with OSCAIRN_PLUGIN_WORKER set, through a worker's shared block. Usage:
// basic-synth.
//
// Two keys held softly, three pedaled loudly: the list puts the pedaled voices first, whatever their level (a held key
// outranks a pedaled note), the quietest of them first; a key just struck ranks above the other held keys until it is 50 ms old. With
// its context's limit lowered by two, the synth gives up the two voices it listed first: the list left is the old one
// less its first two.
#include "oscairn/graph.h"
#include "AllocationGuard.h"
#include <array>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <span>
#include <stdexcept>
#include <string>
#include <vector>
using namespace oscairn;
using kick::clap::Event;
static void check(bool ok,const std::string& why){if(!ok)throw std::runtime_error(why);}

int main(int argc,char** argv){try{
    if(argc!=2)return 2;
    constexpr uint32_t frames=64;
    std::array<float,frames> left{},right{};float* out[]{left.data(),right.data()};
    Graph g(48000,frames);const auto s=g.add(argv[1]);
    g.connectNotes(graphInput,s);g.connectAudio(s,graphOutput);g.commit();
    auto& synth=g.host(s);
    const bool worker=std::getenv("OSCAIRN_PLUGIN_WORKER")!=nullptr;
    check(synth.voiceBudget() && synth.inWorker()==worker,"the Basic Synth offers oscairn.voice-budget, in a worker when asked");
    auto block=[&](std::span<const Event> events={}){forbidAllocation=true;const bool ok=g.process(out,frames,nullptr,events.data(),uint32_t(events.size()));forbidAllocation=false;check(ok,"rendered without allocating");};
    auto send=[&](const Event& e){block({&e,1});};
    auto wait=[&](int blocks){for(int i=0;i<blocks;++i)block();};
    struct Entry{uint8_t tier;float level;bool struck;};
    auto list=[&]{
        const oscairn_voice_budget_importance_t* lists=nullptr;const uint32_t n=synth.importanceReport(lists);
        check(n==1,"one context listed: "+std::to_string(n));
        std::vector<Entry> v;for(uint32_t i=0;i<lists[0].count;++i)v.push_back({lists[0].least[i].tier,lists[0].least[i].level_db,(lists[0].least[i].flags&OSCAIRN_VOICE_BUDGET_STRUCK)!=0});
        return std::pair{uint32_t(lists[0].voices),v};};
    // No reserve, so held voices are listed too; no limit yet.
    std::array<oscairn_voice_budget_context_t,1> limits{{{OSCAIRN_VOICE_BUDGET_SELECTED,0,0,0,0}}};
    synth.setVoiceLimits(0,limits.data(),1);
    send(Event::note(true,40,.3));send(Event::note(true,43,.3)); // held, soft
    send(Event::midi(0xb0,64,127)); // the pedal down
    for(const auto& [k,v]:std::array<std::pair<int,double>,3>{{{60,.9},{64,.6},{67,1.}}}){send(Event::note(true,k,v));send(Event::note(false,k,0));}
    send(Event::note(true,72,.05)); // struck just now, very soft
    {
        const auto [voices,v]=list();
        check(voices==6 && v.size()==6,"six voices listed: "+std::to_string(voices)+", "+std::to_string(v.size()));
        check(v.back().tier==2 && v.back().struck,"the key just struck ranks above every other held key: tier "+std::to_string(v.back().tier));
    }
    send(Event::note(false,72,0));wait(60); // the soft key let go (pedaled now); 80 ms on, none is just struck
    const auto [voices,before]=list();
    check(voices==6 && before.size()==6,"six voices: "+std::to_string(voices));
    std::string seen;for(const auto& e:before)seen+=std::to_string(e.tier)+"@"+std::to_string(int(std::lround(e.level)))+" ";
    for(size_t i=1;i<before.size();++i)check(before[i-1].tier<before[i].tier || (before[i-1].tier==before[i].tier && before[i-1].level<=before[i].level+.5f),"ascending: "+seen);
    check(before[4].tier==2 && before[5].tier==2,"the held keys last, though softer than the pedaled ones: "+seen);
    check(before[0].tier==1 && before[3].tier==1,"the four pedaled voices before them: "+seen);
    // Two fewer for its context: the two listed first go.
    limits[0].limit=voices-2;synth.setVoiceLimits(0,limits.data(),1);wait(10);
    const auto [left2,after]=list();
    std::string now;for(const auto& e:after)now+=std::to_string(e.tier)+"@"+std::to_string(int(std::lround(e.level)))+" ";
    check(left2==4 && after.size()==4,"four voices left: "+std::to_string(left2)+": "+now);
    for(size_t i=0;i<4;++i)check(after[i].tier==before[i+2].tier && std::abs(after[i].level-before[i+2].level)<1.5f,"the list less its first two: was "+seen+", now "+now);
    std::printf("ok: the importance report lists in the order the synth gives voices up (%s)\n",worker?"in a worker":"in process");
    return 0;
}catch(const std::exception& e){std::cerr<<"FAIL: "<<e.what()<<'\n';return 1;}}
