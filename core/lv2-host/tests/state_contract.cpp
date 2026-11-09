// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/plugin_host.h"
#include "oscairn/assets.h"
#include "../Patch.h"
#include "../../plugin-host/src/StateFiles.h"
#include "AllocationGuard.h"
#include <algorithm>
#include <fstream>
#include <iostream>
#include <sstream>
#include <limits>
using namespace oscairn;
void check(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
template<class F> void rejects(F f,const char* message){bool failed=false;try{f();}catch(const std::exception&){failed=true;}check(failed,message);}
int main(int argc,char** argv)
{
    try
    {
        if(argc!=2)return 2;
        state_files::TemporaryDirectory temp;
        auto h=loadPlugin(argv[1],"urn:oscairn:test:state",48000,512);
        std::array<float,64> samples{};float* outputs[]{samples.data()};auto on=kick::clap::Event::note(true,60,1);
        auto render=[&]{forbidAllocation=true;bool ok=h->process(nullptr,0,outputs,1,64,&on,1,nullptr);forbidAllocation=false;return ok;};
        check(h->presets().size()==3,"bundle presets discovered");h->loadPreset("urn:oscairn:test:preset");
        check(h->parameterValue(0)==.75 && render() && samples[0]==1.5f,"preset restores symbol-addressed port and plugin state");
        uint32_t events=0;const auto* emitted=h->outputEvents(events);
        check(events && emitted[0].header()->type==CLAP_EVENT_NOTE_OFF && emitted[0].header()->time==0 && reinterpret_cast<const clap_event_note_t*>(emitted[0].bytes.data())->key==-1,"restore releases downstream notes before new notes");
        h->setParameter(0,.625);const auto saved=(temp.path/"saved").string();h->saveState(saved);
        h->setParameter(0,.1);h->loadState(saved);
        check(render() && samples[0]==1.25f,"save flushes queued ports and restore discards stale edits");
        h->setParameter(8,3);rejects([&]{h->saveState((temp.path/"failure").string());},"plugin save failure propagated");
        check(!std::filesystem::exists(temp.path/"failure"),"failed snapshot never published");
        h->loadState(saved);
        rejects([&]{h->loadPreset("urn:oscairn:test:bad-preset");},"plugin restore failure propagated despite Lilv void API");
        check(!render() && samples[0]==0,"partially restored instance silenced");
        h->loadState(saved);check(render() && samples[0]==1.25f,"valid restore recovers failed instance");
        auto other=loadPlugin(argv[1],"urn:oscairn:test:effect",48000,512);
        rejects([&]{other->loadState(saved);},"state plugin identity checked");
        rejects([&]{h->loadPreset("urn:unknown");},"unknown preset rejected");

        // RFC 0009 step 3: a file the state names is an asset reference in the store, never copied.
        {
            namespace fs=std::filesystem;
            const auto bundleFile=fs::canonical(argv[1])/"state-file.txt";
            auto files=[](const fs::path& d){std::vector<std::string> out;for(const auto& f:fs::recursive_directory_iterator(d))if(f.is_regular_file())out.push_back(fs::relative(f.path(),d).generic_string());std::sort(out.begin(),out.end());return out;};
            auto contains=[](const fs::path& f,const std::string& what){std::ifstream in(f);std::ostringstream text;text<<in.rdbuf();return text.str().find(what)!=std::string::npos;};
            assets::Store store(temp.path/"assets");
            auto a=loadPlugin(argv[1],"urn:oscairn:test:state",48000,512);a->setAssets(&store);
            float* out[]{samples.data()};
            auto play=[&](PluginHost& p){forbidAllocation=true;bool ok=p.process(nullptr,0,out,1,64,&on,1,nullptr);forbidAllocation=false;return ok;};
            a->loadPreset("urn:oscairn:test:file-preset");
            check(play(*a) && samples[0]==float(a->parameterValue(0))*1.75f,"a state file the plugin reads");
            check(a->assetDependencies().empty(),"a preset's file is no asset until it is saved");
            const auto copied=temp.path/"copied";a->saveState(copied.string());
            const auto deps=a->assetDependencies();
            check(deps.size()==1 && deps[0].library=="user" && deps[0].sha256==assets::sha256File(bundleFile) && deps[0].path==deps[0].sha256.substr(0,16)+"/state-file.txt",
                  "saved, a file outside every library is copied into the user library and listed");
            check(files(copied)==std::vector<std::string>{"assets.tsv","manifest.ttl","state.ttl"} && !contains(copied/"state.ttl",bundleFile.string()) && contains(copied/"state.ttl","oscairn-assets/"),
                  "the state holds an abstract name and the reference, not the file or its path");
            auto b=loadPlugin(argv[1],"urn:oscairn:test:state",48000,512);b->setAssets(&store);
            b->loadState(copied.string());
            check(play(*b) && samples[0]==float(b->parameterValue(0))*1.75f && b->assetDependencies().size()==1,"it loads elsewhere through the store");
            auto c=loadPlugin(argv[1],"urn:oscairn:test:state",48000,512);
            rejects([&]{c->loadState(copied.string());},"without a store the state's file is missing");
            // A file in a registered folder is referenced where it is.
            const auto library=store.registerFolder(fs::path(argv[1]));
            a->loadPreset("urn:oscairn:test:file-preset");
            const auto inPlace=temp.path/"in-place";a->saveState(inPlace.string());
            check(a->assetDependencies().size()==1 && a->assetDependencies()[0].library==library && a->assetDependencies()[0].path=="state-file.txt","a registered folder's file is referenced in place");
            // The user library's copy gone, and the bytes nowhere else: refused, saying which.
            fs::permissions(store.root()/"user",fs::perms::owner_all);
            fs::remove_all(store.root()/"user"/deps[0].path.substr(0,16));
            const auto registry=store.root()/"libraries.tsv";fs::remove(registry);fs::remove_all(store.root()/"index"); // forget the registered folder too
            assets::Store fresh(temp.path/"assets");b->setAssets(&fresh);
            bool missing=false;try{b->loadState(copied.string());}catch(const std::exception& e){missing=std::string(e.what()).find("Missing LV2 state asset")!=std::string::npos;}
            check(missing,"a state whose file is not in the store is refused");
        }

        // Exercise bounded property queues/parser without relying on a friendly plugin.
        auto* world=lilv_world_new();auto path=std::filesystem::canonical(argv[1]).string()+"/";
        auto* bundle=lilv_new_file_uri(world,nullptr,path.c_str());lilv_world_load_bundle(world,bundle);lilv_node_free(bundle);
        auto* uri=lilv_new_uri(world,"urn:oscairn:test:state");
        const auto* plugin=lilv_plugins_get_by_uri(lilv_world_get_all_plugins(world),uri);
        lv2::Urids urids;LV2_URID_Map map{&urids,lv2::Urids::map};lv2::Patch patch(world,plugin,urids,&map);
        const std::string key="urn:oscairn:test:value",text="urn:oscairn:test:text";
        PropertyValue value=.5f,wrong=int32_t(1),nan=std::numeric_limits<float>::quiet_NaN(),longText=std::string(2049,'x');
        check(!patch.request(key,&wrong,16384) && !patch.request(key,&nan,16384) && !patch.request(text,&longText,16384),"typed/finite/bounded property values");
        check(!patch.request(key,&value,24),"property must fit connected port");
        for(uint32_t i=0;i<lv2::Patch::capacity;++i)check(patch.request(key,&value,16384),"complete bounded request accepted");
        check(!patch.request(key,&value,16384),"full request queue rejects");
        lv2::Patch::Packet packet;
        forbidAllocation=true;bool popped=patch.pop(packet);bool collected=patch.collect(reinterpret_cast<const LV2_Atom*>(packet.bytes.data()));forbidAllocation=false;
        check(popped && collected && !patch.service() && std::get<float>(*patch.value(key))==.5f,"allocation-free transfer and Set notification");
        auto* atom=reinterpret_cast<LV2_Atom*>(packet.bytes.data());
        auto* field=reinterpret_cast<LV2_Atom_Property_Body*>(packet.bytes.data()+sizeof(LV2_Atom_Object));
        field->value.size=UINT32_MAX;check(patch.collect(atom) && patch.service()==1,"overflow-sized nested property rejected");
        patch.clear();check(!patch.pending() && !patch.value(key),"restore clears queues and stale confirmation");
        // Put dictionary and an unterminated string must both be handled safely.
        LV2_Atom_Forge forge;lv2_atom_forge_init(&forge,&map);lv2_atom_forge_set_buffer(&forge,packet.bytes.data(),packet.bytes.size());
        LV2_Atom_Forge_Frame object,body;lv2_atom_forge_object(&forge,&object,0,urids(LV2_PATCH__Put));
        lv2_atom_forge_key(&forge,urids(LV2_PATCH__body));lv2_atom_forge_object(&forge,&body,0,0);
        lv2_atom_forge_key(&forge,urids(key.c_str()));lv2_atom_forge_float(&forge,.75f);lv2_atom_forge_pop(&forge,&body);lv2_atom_forge_pop(&forge,&object);
        check(patch.collect(atom) && !patch.service() && std::get<float>(*patch.value(key))==.75f,"Put dictionary decoded");
        PropertyValue padded=std::string("hello");check(patch.request(text,&padded,16384) && patch.pop(packet),"padded String request");
        auto* paddedField=reinterpret_cast<LV2_Atom_Property_Body*>(packet.bytes.data()+sizeof(LV2_Atom_Object)+sizeof(LV2_Atom_Property_Body)+8);
        reinterpret_cast<char*>(&paddedField->value+1)[paddedField->value.size++]=0;
        check(patch.collect(atom) && !patch.service() && std::get<std::string>(*patch.value(text))=="hello","extra zero terminator accepted");
        PropertyValue word=std::string("hello");check(patch.request(text,&word,16384) && patch.pop(packet),"String request");
        auto* last=packet.bytes.data()+packet.size; // Locate value through the two property bodies.
        auto* property=reinterpret_cast<LV2_Atom_Property_Body*>(packet.bytes.data()+sizeof(LV2_Atom_Object));
        auto* v=reinterpret_cast<LV2_Atom_Property_Body*>(reinterpret_cast<uint8_t*>(property)+sizeof(*property)+8);
        check(reinterpret_cast<uint8_t*>(&v->value+1)+v->value.size<=last,"fixture string bounds");
        const auto stringOffset=static_cast<size_t>(reinterpret_cast<uint8_t*>(&v->value+1)-packet.bytes.data());
        check(v->value.size && stringOffset+v->value.size<=packet.bytes.size(),"string payload bounds");
        packet.bytes.at(stringOffset+v->value.size-1)='!';
        check(patch.collect(atom) && patch.service()==1,"unterminated string rejected");
        lilv_node_free(uri);lilv_world_free(world);
        std::cout<<"ok: presets, state failure/recovery, queued ports, patch bounds and malformed notifications\n";return 0;
    }
    catch(const std::exception& e){forbidAllocation=false;std::cerr<<e.what()<<'\n';return 1;}
}
