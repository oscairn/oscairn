// SPDX-License-Identifier: AGPL-3.0-or-later
// oscairn.voice-budget's drafts against the host (the SDK owner's review of RFC 0046's draft 2): an engine built as
// draft 1 (a four-entry table under OSCAIRN_EXT_VOICE_BUDGET, draft 1's ID) keeps its voice budget, and a draft-2 table
// whose importance is null (a draft-1 engine rebuilt against draft 2's header) is taken as draft 1: loaded, its limit
// kept, no importance report. A full draft-2 table reports. In process and, with OSCAIRN_PLUGIN_WORKER set, through a
// worker. Usage: voice-fixture.
#include "oscairn/graph.h"
#include <array>
#include <cstdio>
#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>
using namespace oscairn;
using kick::clap::Event;
static void check(bool ok,const std::string& why){if(!ok)throw std::runtime_error(why);}

int main(int argc,char** argv){try{
    if(argc!=2)return 2;
    constexpr uint32_t frames=64;
    std::array<float,frames> left{},right{};float* out[]{left.data(),right.data()};
    setenv("OSCAIRN_FIXTURE_VOICE_BUDGET","1",1);setenv("OSCAIRN_FIXTURE_VOICE_US","1",1);
    for(const char* mode:{"OSCAIRN_FIXTURE_DRAFT_1","OSCAIRN_FIXTURE_NULL_IMPORTANCE",""}){
        unsetenv("OSCAIRN_FIXTURE_DRAFT_1");unsetenv("OSCAIRN_FIXTURE_NULL_IMPORTANCE");
        if(*mode)setenv(mode,"1",1);
        const std::string name=*mode?mode:"draft 2";
        Graph g(48000,frames);const auto s=g.add(argv[1]);
        g.connectNotes(graphInput,s);g.connectAudio(s,graphOutput);g.commit();
        auto& fixture=g.host(s);
        check(fixture.voiceBudget(),name+": the voice budget is kept");
        auto block=[&](const Event* e=nullptr){check(g.process(out,frames,nullptr,e,e?1:0),name+": rendered");};
        for(int k:{60,62,64,65}){const auto e=Event::note(true,k,.8);block(&e);}
        for(int i=0;i<4;++i)block();
        auto voices=[&]{double cost=0;const oscairn_voice_budget_note_t* notes=nullptr;uint32_t count=0;return fixture.voiceReport(cost,notes,count);};
        check(voices()==4,name+": four voices: "+std::to_string(voices()));
        const oscairn_voice_budget_importance_t* lists=nullptr;
        const uint32_t reported=fixture.importanceReport(lists);
        check(*mode?reported==0:reported==1,name+": importance lists "+std::to_string(reported));
        std::array<oscairn_voice_budget_context_t,1> limits{{{OSCAIRN_VOICE_BUDGET_SELECTED,0,0,0,0}}};
        fixture.setVoiceLimits(2,limits.data(),1);
        for(int i=0;i<10;++i)block();
        check(voices()==2,name+": keeps to a limit of two: "+std::to_string(voices()));
    }
    std::printf("ok: a draft-1 table and a null importance keep the voice budget without a report; draft 2 reports (%s)\n",
                std::getenv("OSCAIRN_PLUGIN_WORKER")?"in a worker":"in process");
    return 0;
}catch(const std::exception& e){std::cerr<<"FAIL: "<<e.what()<<'\n';return 1;}}
