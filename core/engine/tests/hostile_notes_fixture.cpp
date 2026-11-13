// SPDX-License-Identifier: AGPL-3.0-or-later
// A note effect that sends what a careless arpeggiator can (org.oscairn.test.hostile-notes): each
// block it passes on what it receives and then pushes notes until the host refuses one, with
// addresses no instrument should see (keys, channels and ports of -1, past 127, 15 and the ports
// there are), note IDs never announced, offs and chokes for notes never sent, expressions, raw MIDI
// of any bytes, and times up to the block's length (one past its last sample); after a refusal,
// and at the first block after reset(), a choke with every field -1. Seeded, so a run repeats.
#include <clap/clap.h>
#include <cstdint>
#include <cstring>
#include <initializer_list>

namespace {
const char* features[]{CLAP_PLUGIN_FEATURE_NOTE_EFFECT,nullptr};
const clap_plugin_descriptor_t descriptor{CLAP_VERSION,"org.oscairn.test.hostile-notes","Hostile notes","Oscairn","","","","1","",features};

struct Plugin {
    clap_plugin_t api{};
    uint32_t seed=12345;
    bool wasReset=false;
    uint32_t next(){seed=seed*1664525u+1013904223u;return seed>>8;}
    template<class T> T pick(std::initializer_list<T> from){return from.begin()[next()%from.size()];}
    Plugin(){
        api.desc=&descriptor;api.plugin_data=this;
        api.init=[](const clap_plugin_t*){return true;};api.destroy=[](const clap_plugin_t* p){delete static_cast<Plugin*>(p->plugin_data);};
        api.activate=[](const clap_plugin_t*,double,uint32_t,uint32_t){return true;};api.deactivate=[](const clap_plugin_t*){};
        api.start_processing=[](const clap_plugin_t*){return true;};api.stop_processing=[](const clap_plugin_t*){};api.on_main_thread=[](const clap_plugin_t*){};
        api.reset=[](const clap_plugin_t* p){static_cast<Plugin*>(p->plugin_data)->wasReset=true;};
        api.process=[](const clap_plugin_t* p,const clap_process_t* b)->clap_process_status{
            auto& s=*static_cast<Plugin*>(p->plugin_data);
            const auto* in=b->in_events;const auto* out=b->out_events;
            auto chokeAll=[&](uint32_t time){clap_event_note_t e{};e.header={sizeof(e),time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_CHOKE,0};e.note_id=-1;e.port_index=-1;e.channel=-1;e.key=-1;return out->try_push(out,&e.header);};
            if(s.wasReset){s.wasReset=false;chokeAll(0);}
            for(uint32_t i=0;i<in->size(in);++i){const auto* h=in->get(in,i);if(h->space_id==CLAP_CORE_EVENT_SPACE_ID)out->try_push(out,h);}
            const uint32_t frames=b->frames_count;
            for(uint32_t sent=0;sent<70000;++sent){
                const uint32_t time=frames?(s.next()%8==0?frames:s.next()%frames):0;
                bool ok;
                switch(s.next()%5){
                case 0: case 1: case 2:{
                    clap_event_note_t e{};
                    e.header={sizeof(e),time,CLAP_CORE_EVENT_SPACE_ID,s.pick<uint16_t>({CLAP_EVENT_NOTE_ON,CLAP_EVENT_NOTE_ON,CLAP_EVENT_NOTE_OFF,CLAP_EVENT_NOTE_CHOKE,CLAP_EVENT_NOTE_END}),0};
                    e.note_id=s.pick<int32_t>({-1,int32_t(s.next()%64),INT32_MAX,int32_t(0x80000000u)});
                    e.port_index=s.pick<int16_t>({-1,0,0,0,1,15,16,32767});e.channel=s.pick<int16_t>({-1,0,int16_t(s.next()%16),15,16,255,-32768});
                    e.key=s.pick<int16_t>({-1,0,int16_t(s.next()%128),127,128,200,32767});e.velocity=s.pick<double>({0,.5,1,2,-1});
                    ok=out->try_push(out,&e.header);break;}
                case 3:{
                    clap_event_note_expression_t e{};e.header={sizeof(e),time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_NOTE_EXPRESSION,0};
                    e.expression_id=s.pick<clap_note_expression>({CLAP_NOTE_EXPRESSION_VOLUME,CLAP_NOTE_EXPRESSION_TUNING,99});e.note_id=-1;
                    e.port_index=s.pick<int16_t>({-1,0,16});e.channel=s.pick<int16_t>({-1,0,16});e.key=s.pick<int16_t>({-1,60,200});e.value=s.pick<double>({0,.5,1e300});
                    ok=out->try_push(out,&e.header);break;}
                default:{
                    clap_event_midi_t e{};e.header={sizeof(e),time,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_MIDI,0};e.port_index=s.pick<uint16_t>({0,0,1,16,65535});
                    e.data[0]=uint8_t(s.next());e.data[1]=uint8_t(s.next());e.data[2]=uint8_t(s.next());
                    ok=out->try_push(out,&e.header);break;}
                }
                if(!ok && time<frames){chokeAll(0);break;} // refused: its recovery, which may be refused too
            }
            return CLAP_PROCESS_CONTINUE;
        };
        api.get_extension=[](const clap_plugin_t*,const char* id)->const void*{
            static const clap_plugin_note_ports_t notes{
                [](const clap_plugin_t*,bool)->uint32_t{return 1;},
                [](const clap_plugin_t*,uint32_t i,bool,clap_note_port_info_t* p){if(i)return false;*p={};p->id=0;p->supported_dialects=CLAP_NOTE_DIALECT_CLAP|CLAP_NOTE_DIALECT_MIDI;p->preferred_dialect=CLAP_NOTE_DIALECT_CLAP;std::strcpy(p->name,"Notes");return true;}};
            return std::strcmp(id,CLAP_EXT_NOTE_PORTS)?nullptr:&notes;
        };
    }
};
const clap_plugin_factory_t factory{[](const clap_plugin_factory_t*)->uint32_t{return 1;},[](const clap_plugin_factory_t*,uint32_t i){return i==0?&descriptor:nullptr;},
    [](const clap_plugin_factory_t*,const clap_host_t*,const char* id)->const clap_plugin_t*{return std::strcmp(id,descriptor.id)?nullptr:&(new Plugin)->api;}};
}
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{CLAP_VERSION,[](const char*){return true;},[](){},[](const char* id)->const void*{return std::strcmp(id,CLAP_PLUGIN_FACTORY_ID)?nullptr:&factory;}};
