// SPDX-License-Identifier: AGPL-3.0-or-later
#include <clap/clap.h>
#include <clap/ext/audio-ports.h>
#include <clap/ext/note-ports.h>
#include <clap/ext/params.h>
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstdlib>
#include <cstring>

namespace
{
    const char* features[]{CLAP_PLUGIN_FEATURE_INSTRUMENT,nullptr};
    const clap_plugin_descriptor_t descriptor{CLAP_VERSION,"org.oscairn.test.control","Control fixture","Oscairn","","","","1","",features};
    // Parameter 1, "Value"; with OSCAIRN_CONTROL_FIXTURE_PARAMETERS=N, N-1 more (2 to N, 0 each), to measure what
    // a host does per parameter. Key 127 changes parameter 2 to its next quarter without an event and asks
    // the host to read the values again (CLAP_PARAM_RESCAN_VALUES), as a plugin's own preset browser would.
    uint32_t parameterCount()
    {
        const char* n = std::getenv("OSCAIRN_CONTROL_FIXTURE_PARAMETERS");
        return n ? std::clamp<uint32_t>(uint32_t(std::strtoul(n, nullptr, 10)), 1, 30000) : 1;
    }
    struct Plugin
    {
        clap_plugin_t api{};
        const clap_host_t* host = nullptr;
        std::atomic<double> value{0.5}, other{0};
        std::atomic<bool> rescan{false};
        uint32_t count = parameterCount();
        bool gate = false;
        static Plugin& get(const clap_plugin_t* p) { return *static_cast<Plugin*>(p->plugin_data); }
        static void events(const clap_plugin_t* p, const clap_input_events_t* in, const clap_output_events_t* out)
        {
            auto& s = get(p);
            for(uint32_t i=0; i<in->size(in); ++i)
            {
                const auto* e = in->get(in,i);
                if(e->type == CLAP_EVENT_NOTE_ON && reinterpret_cast<const clap_event_note_t*>(e)->key == 127)
                {
                    if(s.count > 1) { s.rescan = true; s.host->request_callback(s.host); }
                    continue;
                }
                if(e->type == CLAP_EVENT_NOTE_ON) s.gate = true;
                if(e->type == CLAP_EVENT_NOTE_OFF) s.gate = false;
                if(e->type == CLAP_EVENT_PARAM_VALUE)
                {
                    const auto* v = reinterpret_cast<const clap_event_param_value_t*>(e);
                    if(v->param_id == 1) s.value = v->value;
                    if(v->param_id == 2) s.other = v->value;
                    out->try_push(out,e);
                }
                if(e->type == CLAP_EVENT_PARAM_GESTURE_BEGIN || e->type == CLAP_EVENT_PARAM_GESTURE_END) out->try_push(out,e);
            }
        }
        Plugin()
        {
            api.desc=&descriptor; api.plugin_data=this;
            api.init=[](const clap_plugin_t*){return true;};
            api.destroy=[](const clap_plugin_t* p){delete &get(p);};
            api.activate=[](const clap_plugin_t*,double,uint32_t,uint32_t){return true;};
            api.deactivate=[](const clap_plugin_t*){};
            api.start_processing=[](const clap_plugin_t*){return true;};
            api.stop_processing=[](const clap_plugin_t*){};
            api.reset=[](const clap_plugin_t* p){get(p).gate=false;};
            api.on_main_thread=[](const clap_plugin_t* p){
                auto& s=get(p);
                if(!s.rescan.exchange(false)) return;
                s.other = s.other + .25 > 1 ? 0 : s.other + .25;
                if(auto* params=static_cast<const clap_host_params_t*>(s.host->get_extension(s.host,CLAP_EXT_PARAMS))) params->rescan(s.host,CLAP_PARAM_RESCAN_VALUES);
            };
            api.process=[](const clap_plugin_t* p,const clap_process_t* b)->clap_process_status{
                events(p,b->in_events,b->out_events);
                auto& s=get(p);
                for(uint32_t c=0;c<2;++c) for(uint32_t i=0;i<b->frames_count;++i)
                    b->audio_outputs[0].data32[c][i] = s.gate ? static_cast<float>(s.value.load()) : b->audio_inputs[0].data32[c][i];
                return CLAP_PROCESS_CONTINUE;
            };
            api.get_extension=[](const clap_plugin_t*,const char* id)->const void* {
                static const clap_plugin_audio_ports_t ports{
                    [](const clap_plugin_t*,bool)->uint32_t{return 1;},
                    [](const clap_plugin_t*,uint32_t i,bool,clap_audio_port_info_t* p){
                        if(i) return false; *p={}; p->id=0; std::strcpy(p->name,"stereo"); p->channel_count=2;
                        p->flags=CLAP_AUDIO_PORT_IS_MAIN; p->port_type=CLAP_PORT_STEREO; p->in_place_pair=CLAP_INVALID_ID; return true;
                    }};
                static const clap_plugin_note_ports_t notes{
                    [](const clap_plugin_t*,bool in)->uint32_t{return in ? 1 : 0;},
                    [](const clap_plugin_t*,uint32_t i,bool in,clap_note_port_info_t* p){
                        if(i || !in) return false; *p={}; p->id=0; p->supported_dialects=p->preferred_dialect=CLAP_NOTE_DIALECT_CLAP; return true;
                    }};
                static const clap_plugin_params_t params{
                    [](const clap_plugin_t* p)->uint32_t{return get(p).count;},
                    [](const clap_plugin_t* plugin,uint32_t i,clap_param_info_t* p){
                        if(i >= get(plugin).count) return false;
                        *p={}; p->id=i+1; std::snprintf(p->name,sizeof p->name,i ? "Extra %u" : "Value",unsigned(i)); p->flags=CLAP_PARAM_IS_AUTOMATABLE;
                        p->min_value=0; p->max_value=1; p->default_value=i ? 0 : .5; return true;
                    },
                    [](const clap_plugin_t* p,clap_id id,double* v){
                        if(id<1 || id>get(p).count) return false;
                        *v = id==1 ? get(p).value.load() : id==2 ? get(p).other.load() : 0; return true;
                    },
                    [](const clap_plugin_t*,clap_id,double v,char* out,uint32_t n){std::snprintf(out,n,"%.3f",v);return true;},
                    nullptr,events};
                if(!std::strcmp(id,CLAP_EXT_AUDIO_PORTS))return &ports;
                if(!std::strcmp(id,CLAP_EXT_NOTE_PORTS))return &notes;
                if(!std::strcmp(id,CLAP_EXT_PARAMS))return &params;
                return nullptr;
            };
        }
    };
    const clap_plugin_factory_t factory{
        [](const clap_plugin_factory_t*)->uint32_t{return 1;},
        [](const clap_plugin_factory_t*,uint32_t i){return i ? nullptr : &descriptor;},
        [](const clap_plugin_factory_t*,const clap_host_t* host,const char* id)->const clap_plugin_t*{
            if(std::strcmp(id,descriptor.id)) return nullptr;
            auto* p=new Plugin; p->host=host; return &p->api;
        }};
}
extern "C" CLAP_EXPORT const clap_plugin_entry_t clap_entry{
    CLAP_VERSION,[](const char*){return true;},[](){},
    [](const char* id)->const void*{return !std::strcmp(id,CLAP_PLUGIN_FACTORY_ID) ? &factory : nullptr;}};
