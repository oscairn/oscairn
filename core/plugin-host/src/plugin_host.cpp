// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/plugin_host.h"
#include "StateFiles.h"
#include "clap_host.h"
#include <kick_clap/Discovery.h>
#include <algorithm>
#include <atomic>
#include <filesystem>
#include <sstream>
#include <iomanip>
#include <locale>
#include <cctype>
#include <chrono>
#include <cmath>
#include <fstream>
#include <future>
#include <map>
#include <memory>
#include <mutex>
#include <thread>
#include <unistd.h>
#ifdef __APPLE__
#include <libproc.h>
#endif
#ifdef __linux__
#include <fcntl.h>
#include <pthread.h>
#include <sys/mman.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#endif
#ifdef __GLIBC__
#include <malloc.h>
#endif
#include <cerrno>
#include <cstdio>
#include <cstring>
#include <cstdlib>
#include <stdexcept>
#include <string_view>
#include <system_error>
namespace oscairn
{
#ifdef OSCAIRN_LV2
    std::shared_ptr<PluginHost> loadLv2Plugin(const std::string&,const std::string&,double,uint32_t,ProcessingMode,Lv2BufferLimits);
#endif
    namespace
    {
        // A CLAP preset as a URI: clap-preset:KIND:LOCATION#LOAD_KEY, the parts percent-encoded.
        std::string encodePart(const std::string& s)
        {
            static const char* hex="0123456789ABCDEF";std::string out;
            for(unsigned char c:s){if(c<0x20 || c=='%' || c==':' || c=='#' || c==0x7f){out+='%';out+=hex[c>>4];out+=hex[c&15];}else out+=char(c);}
            return out;
        }
        std::string decodePart(const std::string& s)
        {
            std::string out;
            for(size_t i=0;i<s.size();++i){
                if(s[i]!='%'){out+=s[i];continue;}
                if(i+2>=s.size() || !std::isxdigit(static_cast<unsigned char>(s[i+1])) || !std::isxdigit(static_cast<unsigned char>(s[i+2])))throw std::invalid_argument("Invalid preset URI");
                out+=char(std::stoi(s.substr(i+1,2),nullptr,16));i+=2;
            }
            return out;
        }
        std::string presetUri(const kick::clap::Preset& p){return "clap-preset:"+std::to_string(p.kind)+":"+encodePart(p.location)+"#"+encodePart(p.loadKey);}
        kick::clap::Preset presetFrom(const std::string& uri)
        {
            const std::string scheme="clap-preset:";
            const auto colon=uri.find(':',scheme.size()),hash=uri.find('#');
            if(uri.compare(0,scheme.size(),scheme)!=0 || colon==std::string::npos || hash==std::string::npos || hash<colon)throw std::invalid_argument("Not a CLAP preset URI");
            kick::clap::Preset p;const auto kind=uri.substr(scheme.size(),colon-scheme.size());
            if(kind!="0" && kind!="1")throw std::invalid_argument("Invalid preset location kind");
            p.kind=uint32_t(kind[0]-'0');p.location=decodePart(uri.substr(colon+1,hash-colon-1));p.loadKey=decodePart(uri.substr(hash+1));
            return p;
        }

        // A failed clap_host.h call thrown as the exception ClapHost threw for it.
        [[noreturn]] void fail()
        {
            int kind=0;const std::string message=OSCAIRN_CLAP(error)(&kind);
            if(kind==OSCAIRN_CLAP_INVALID_ARGUMENT)throw std::invalid_argument(message);
            if(kind==OSCAIRN_CLAP_LOGIC_ERROR)throw std::logic_error(message);
            throw std::runtime_error(message);
        }
        template<class T> T checked(T result){if(!result)fail();return result;}
        bool checked(int result){if(result<0)fail();return result;}
        template<class T> void append(void* to,const T* item){static_cast<std::vector<T>*>(to)->push_back(*item);}

        // An AssetResolver as clap_host.h's table: Host.cpp's oscairn.assets calls past their thread and argument checks.
        bool report(char* message, uint32_t capacity, const std::string& text) { if(message && capacity) std::snprintf(message, capacity, "%s", text.c_str()); return false; }
    }
}
oscairn_clap_assets oscairn_clap_assets_of(kick::clap::AssetResolver* assets)
        {
            using R = kick::clap::AssetResolver;
            return {assets,
                [](void* c, oscairn_asset_ref_t* ref, char* path, uint32_t capacity, char* message, uint32_t size) {
                    const std::string library(ref->library, strnlen(ref->library, sizeof(ref->library))), relative(ref->path, strnlen(ref->path, sizeof(ref->path)));
                    std::string hash(ref->sha256, strnlen(ref->sha256, sizeof(ref->sha256)));
                    try {
                        const auto file = static_cast<R*>(c)->resolve(library, relative, hash);
                        if (file.size() + 1 > capacity || hash.size() >= sizeof(ref->sha256)) return oscairn::report(message, size, "asset path too long");
                        std::memcpy(path, file.c_str(), file.size() + 1);
                        std::memcpy(ref->sha256, hash.c_str(), hash.size() + 1);
                        return true;
                    } catch (const std::exception& e) { return oscairn::report(message, size, e.what()); }
                },
                [](void* c, const char* library, const char* minVersion, char* path, uint32_t capacity) {
                    try {
                        const auto root = static_cast<R*>(c)->libraryRoot(std::string(library, strnlen(library, 256)), minVersion ? std::string(minVersion, strnlen(minVersion, 128)) : std::string());
                        if (root.empty() || root.size() + 1 > capacity) return false;
                        std::memcpy(path, root.c_str(), root.size() + 1);
                        return true;
                    } catch (...) { return false; }
                },
                [](void* c, const char* file, oscairn_asset_ref_t* ref, char* message, uint32_t size) {
                    try {
                        const auto made = static_cast<R*>(c)->create(std::string(file, strnlen(file, 4096)));
                        if (made.library.size() >= sizeof(ref->library) || made.path.size() >= sizeof(ref->path) || made.sha256.size() >= sizeof(ref->sha256)) return oscairn::report(message, size, "asset reference too long");
                        *ref = {};
                        std::memcpy(ref->library, made.library.c_str(), made.library.size() + 1);
                        std::memcpy(ref->path, made.path.c_str(), made.path.size() + 1);
                        std::memcpy(ref->sha256, made.sha256.c_str(), made.sha256.size() + 1);
                        return true;
                    } catch (const std::exception& e) { return oscairn::report(message, size, e.what()); }
                },
                [](void* c, const oscairn_asset_ref_t* ref, char* path, uint32_t capacity, oscairn_sample_location_t* location, char* message, uint32_t size) {
                    const std::string name(ref->path, strnlen(ref->path, sizeof(ref->path))), digest(ref->sha256, strnlen(ref->sha256, sizeof(ref->sha256)));
                    if (name.rfind("sample:", 0) != 0 || (!digest.empty() && digest != name.substr(7))) return oscairn::report(message, size, "not a sample reference: " + name);
                    try {
                        const auto at = static_cast<R*>(c)->resolveSample(name.substr(7));
                        if (at.pack.size() + 1 > capacity) return oscairn::report(message, size, "sample path too long");
                        std::memcpy(path, at.pack.c_str(), at.pack.size() + 1);
                        *location = {at.offset, at.length, at.preload};
                        return true;
                    } catch (const std::exception& e) { return oscairn::report(message, size, e.what()); }
                }};
        }
namespace oscairn
{
    namespace
    {

        // The CLAP host (clap_host.h) as a PluginHost: its description kept in the C++ types, again whenever it changes.
        class ClapHost final : public PluginHost
        {
            oscairn_clap_host* host;
            kick::clap::Descriptor descriptor_;
            std::vector<kick::clap::Parameter> parameters_;
            std::vector<kick::clap::Port> inputs_, outputs_;
            uint64_t revision_ = ~uint64_t(0);
            // After each control call: a rescan or a restart changed the description.
            void sync()
            {
                const auto revision=OSCAIRN_CLAP(revision)(host);if(revision==revision_)return;
                descriptor_={};parameters_.clear();inputs_.clear();outputs_.clear();
                const oscairn_clap_description to{this,
                    [](void* c,const oscairn_clap_descriptor* d){static_cast<ClapHost*>(c)->descriptor_={d->path,d->id,d->name,d->vendor,d->version,d->description,{},d->preset_discovery,{},{}};},
                    [](void* c,const char* f){static_cast<ClapHost*>(c)->descriptor_.features.emplace_back(f);},
                    [](void* c,const oscairn_clap_preset* p){static_cast<ClapHost*>(c)->descriptor_.presets.push_back({p->name,p->kind,p->location,p->load_key});},
                    [](void* c,uint32_t number,const char* name){static_cast<ClapHost*>(c)->descriptor_.lanes.emplace_back(number,name);},
                    [](void* c,bool input,const oscairn_clap_port* p){auto& h=*static_cast<ClapHost*>(c);(input?h.inputs_:h.outputs_).push_back({p->id,p->name,p->channels,p->flags});},
                    [](void* c,const oscairn_clap_parameter* p){static_cast<ClapHost*>(c)->parameters_.push_back({p->id,p->name,p->module,p->minimum,p->maximum,p->default_value,p->flags,
                        p->widget,p->priority,p->range_high,p->group,p->role,p->view,p->field,p->index,{reinterpret_cast<const std::array<uint8_t,11>*>(p->algorithms),reinterpret_cast<const std::array<uint8_t,11>*>(p->algorithms)+p->algorithm_count},{p->seconds,p->seconds+p->second_count}});}};
                OSCAIRN_CLAP(describe)(host,&to);
                revision_=revision;
            }
            // A control call that can rescan or restart the plugin: the description brought up to date, then its failure thrown.
            void control(bool ok){sync();checked(ok);}
        public:
            explicit ClapHost(oscairn_clap_host* h):host(h){sync();}
            ~ClapHost() override {OSCAIRN_CLAP(free)(host);}
            ClapHost(const ClapHost&)=delete;
            ClapHost& operator=(const ClapHost&)=delete;
            std::shared_ptr<PluginHost> createEffectContext() override {return std::make_shared<ClapHost>(checked(OSCAIRN_CLAP(effect_context)(host)));}
            std::optional<oscairn_effect_resources_t> effectResources() const override {
                oscairn_effect_resources_t r{};if(checked(OSCAIRN_CLAP(effect_resources)(host,&r)))return r;return {};
            }
            std::optional<oscairn_memory_t> declaredMemory() const override {oscairn_memory_t m{};if(checked(OSCAIRN_CLAP(declared_memory)(host,&m)))return m;return {};}
            void setMemoryRoom(uint64_t bytes) override {OSCAIRN_CLAP(set_memory_room)(host,bytes);}
            void setActiveOutputs(uint64_t ports) override {OSCAIRN_CLAP(set_active_outputs)(host,ports);}
            uint32_t soundContextCapacity() const noexcept override {return OSCAIRN_CLAP(sound_context_capacity)(host);}
            std::vector<oscairn_sound_context_info_t> soundContexts() const override {
                std::vector<oscairn_sound_context_info_t> r;checked(OSCAIRN_CLAP(sound_contexts)(host,append<oscairn_sound_context_info_t>,&r));return r;
            }
            uint32_t prepareSound(const std::string& name,const std::vector<oscairn_sound_parameter_t>& p,const std::string& state,const std::string& preset,const std::string& sfz,const std::string& library,const std::string& directory) override {
                uint32_t id=0;
                if(!preset.empty()){if(!state.empty() || !p.empty() || !sfz.empty() || !library.empty() || !directory.empty())throw std::invalid_argument("A native sound from a preset takes nothing else");
                    const auto k=presetFrom(preset);const oscairn_clap_preset c{"",k.kind,k.location.c_str(),k.loadKey.c_str()};
                    checked(OSCAIRN_CLAP(prepare_preset)(host,name.c_str(),&c,&id));return id;}
                if(!state.empty())throw std::invalid_argument("Native contexts accept parameter, SFZ or preset patches only");
                checked(OSCAIRN_CLAP(prepare_sound)(host,name.c_str(),p.data(),uint32_t(p.size()),sfz.c_str(),library.c_str(),directory.c_str(),&id));return id;}
            bool selectSound(uint32_t id,int16_t channel) override {return checked(OSCAIRN_CLAP(select_sound)(host,id,channel));}
            bool discardSound(uint32_t id,bool) override {return checked(OSCAIRN_CLAP(discard_sound)(host,id));}
            bool preparesSnapshots() const noexcept override {return OSCAIRN_CLAP(prepares_snapshots)(host);}
            bool ringsOut() const noexcept override {return OSCAIRN_CLAP(rings_out)(host);}
            bool chokeRetiring() override {return checked(OSCAIRN_CLAP(choke_retiring)(host));}
            bool voiceBudget() const noexcept override {return OSCAIRN_CLAP(voice_budget)(host);}
            bool honorsVolume() const noexcept override {return OSCAIRN_CLAP(honors_volume)(host);}
            void setVoiceLimits(uint32_t limit,const oscairn_voice_budget_context_t* contexts,uint32_t count) noexcept override {OSCAIRN_CLAP(set_voice_limits)(host,limit,contexts,count);}
            uint32_t voiceReport(double& voiceCost,const oscairn_voice_budget_note_t*& notes,uint32_t& count) const noexcept override {return OSCAIRN_CLAP(voice_report)(host,&voiceCost,&notes,&count);}
            std::array<uint32_t,16> prepareSnapshot(const std::string& directory) override {
                std::array<uint32_t,16> ids{};checked(OSCAIRN_CLAP(prepare_snapshot)(host,directory.c_str(),ids.data()));return ids;
            }
            bool savesSounds() const noexcept override {return OSCAIRN_CLAP(saves_sounds)(host);}
            void saveSound(uint32_t id,const std::string& directory) override {checked(OSCAIRN_CLAP(save_sound)(host,id,directory.c_str()));}
            std::vector<oscairn_sound_parameter_t> soundValues(uint32_t id) const override {
                std::vector<oscairn_sound_parameter_t> r;checked(OSCAIRN_CLAP(sound_values)(host,id,append<oscairn_sound_parameter_t>,&r));return r;
            }
            bool supportsState() const noexcept override {return OSCAIRN_CLAP(supports_state)(host);}
            bool hasWindow() const override {return OSCAIRN_CLAP(has_window)(host);}
            void showWindow(bool show,const std::string& title) override {checked(OSCAIRN_CLAP(show_window)(host,show,title.c_str()));}
            // RFC 0005: through the isolated scan helper where one is configured; without one, this
            // plugin already runs in this process, and so does its index (within the index's budget).
            std::vector<PresetInfo> presets() override {
                if(!placementPolicy().scan.empty())return scanPluginPresets(descriptor_.path,descriptor_.id);
                std::vector<PresetInfo> r;
                checked(OSCAIRN_CLAP(presets)(host,[](void* to,const oscairn_clap_preset* p){static_cast<std::vector<PresetInfo>*>(to)->push_back({presetUri({p->name,p->kind,p->location,p->load_key}),p->name});},&r));
                return r;
            }
            void loadPreset(const std::string& uri) override {
                const auto p=presetFrom(uri);const oscairn_clap_preset c{p.name.c_str(),p.kind,p.location.c_str(),p.loadKey.c_str()};
                control(OSCAIRN_CLAP(load_preset)(host,&c));
            }
            void setAssets(kick::clap::AssetResolver* assets) noexcept override {const auto table=oscairn_clap_assets_of(assets);OSCAIRN_CLAP(set_assets)(host,assets?&table:nullptr);}
            std::vector<kick::clap::AssetReference> assetDependencies() const override {
                std::vector<kick::clap::AssetReference> r;
                checked(OSCAIRN_CLAP(asset_dependencies)(host,[](void* to,const oscairn_clap_asset* a){static_cast<std::vector<kick::clap::AssetReference>*>(to)->push_back({a->library,a->path,a->sha256,a->optional});},&r));
                return r;
            }
            void saveSnapshot(const std::string& directory) override {
                if(soundContextCapacity() && !OSCAIRN_CLAP(supports_sound_snapshot)(host))throw std::runtime_error("Native engine has no complete sound-context snapshot interface");
                PluginHost::saveSnapshot(directory);
            }
            void saveState(const std::string& directory) override {checked(OSCAIRN_CLAP(save_state)(host,directory.c_str()));}
            void loadState(const std::string& directory) override {control(OSCAIRN_CLAP(load_state)(host,directory.c_str()));}
            void service() override {control(OSCAIRN_CLAP(service)(host));}
            bool reset() override {checked(OSCAIRN_CLAP(reset)(host));return true;}
            void reconfigure(double rate,uint32_t frames) override {control(OSCAIRN_CLAP(reconfigure)(host,rate,frames));}
            const kick::clap::Descriptor& descriptor() const noexcept override { return descriptor_; }
            const std::vector<kick::clap::Parameter>& parameters() const noexcept override { return parameters_; }
            const std::vector<kick::clap::Port>& inputPorts() const noexcept override { return inputs_; }
            const std::vector<kick::clap::Port>& outputPorts() const noexcept override { return outputs_; }
            bool usesClapNotes(uint32_t port) const noexcept override { return OSCAIRN_CLAP(uses_clap_notes)(host,port); }
            bool takesSysex() const noexcept override { return OSCAIRN_CLAP(takes_sysex)(host); }
            bool emitsSysex() const noexcept override { return OSCAIRN_CLAP(emits_sysex)(host); }
            void panic() noexcept override { OSCAIRN_CLAP(panic)(host); }
            uint32_t noteInputPorts() const noexcept override { return OSCAIRN_CLAP(note_input_ports)(host); }
            uint32_t noteOutputPorts() const noexcept override { return OSCAIRN_CLAP(note_output_ports)(host); }
            double parameterValue(clap_id id) const noexcept override { return OSCAIRN_CLAP(parameter_value)(host,id); }
            uint64_t parameterReports() const noexcept override { return OSCAIRN_CLAP(parameter_reports)(host); }
            uint64_t parameterRevision() const noexcept override { return OSCAIRN_CLAP(revision)(host); }
            std::string parameterText(clap_id id, double value) const override {
                std::string s;checked(OSCAIRN_CLAP(parameter_text)(host,id,value,[](void* to,const char* text,size_t size){static_cast<std::string*>(to)->assign(text,size);},&s));return s;
            }
            bool setParameter(clap_id id, double value) noexcept override { return OSCAIRN_CLAP(set_parameter)(host,id,value); }
            bool parameterGesture(clap_id id, bool begin) noexcept override { return OSCAIRN_CLAP(parameter_gesture)(host,id,begin); }
            bool sendEvent(const kick::clap::Event& e) noexcept override { return OSCAIRN_CLAP(send_event)(host,&e); }
            bool popOutput(kick::clap::Event& event) noexcept override { return OSCAIRN_CLAP(pop_output)(host,&event); }
            uint64_t outputEventDrops() const noexcept override { return OSCAIRN_CLAP(output_event_drops)(host); }
            const kick::clap::Event* outputEvents(uint32_t& count) const noexcept override { return OSCAIRN_CLAP(output_events)(host,&count); }
            kick::clap::Statistics statistics() const noexcept override { kick::clap::Statistics s;OSCAIRN_CLAP(statistics)(host,&s);return s; }
            bool asleep() const noexcept override { return OSCAIRN_CLAP(asleep)(host); }
            bool restful() const noexcept override { return OSCAIRN_CLAP(restful)(host); }
            void rested(uint32_t frames, uint32_t blocks) noexcept override { OSCAIRN_CLAP(rested)(host,frames,blocks); }
            void mirrorWake(std::atomic<uint32_t>* mirror) noexcept override { OSCAIRN_CLAP(mirror_wake)(host,mirror); }
            bool wasReset() const noexcept override { return OSCAIRN_CLAP(was_reset)(host); }
            bool process(const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames, const kick::clap::Event* events, uint32_t count, const clap_event_transport_t* transport) noexcept override { return OSCAIRN_CLAP(process)(host,inputs,ni,outputs,no,frames,events,count,transport); }
        };
    }
    void PluginHost::saveSnapshot(const std::string& directory)
    {
        if(soundContextCapacity() && !supportsState())throw std::runtime_error("Plugin cannot snapshot all native sounds");
        state_files::Bundle bundle(directory);
        std::ostringstream out;out.imbue(std::locale::classic());out<<std::setprecision(17);
        if(supportsState()){saveState((bundle.temporary.path/"state").string());out<<"state 1\n";}
        else {
            // shortcut: parameter-only restore fits one queue; add offline flushing for larger plugins.
            if(std::count_if(parameters().begin(),parameters().end(),[](const auto& p){return !(p.flags&CLAP_PARAM_IS_READONLY);})>kick::clap::Host::maxEvents)
                throw std::runtime_error("Parameter-only snapshots support at most 2048 writable parameters");
            out<<"parameters 1\n";
            for(const auto& p:parameters())if(!(p.flags&CLAP_PARAM_IS_READONLY)) {
                const double value=parameterValue(p.id);if(!std::isfinite(value))throw std::runtime_error("Nonfinite plugin parameter");
                out<<p.id<<' '<<value<<'\n';
            }
        }
        const auto text=out.str();state_files::write(bundle.temporary.path/"snapshot",text.data(),text.size());bundle.publish();
    }
    void PluginHost::loadSnapshot(const std::string& directory)
    {
        const auto data=state_files::read(std::filesystem::path(directory)/"snapshot",2*1024*1024);
        std::istringstream input(std::string(data.begin(),data.end()));input.imbue(std::locale::classic());
        std::string kind;unsigned version;if(!(input>>kind>>version) || version!=1)throw std::runtime_error("Invalid plugin snapshot version");
        if(kind=="state") {input>>std::ws;if(!input.eof())throw std::runtime_error("Trailing snapshot data");loadState((std::filesystem::path(directory)/"state").string());return;}
        if(kind!="parameters" || soundContextCapacity())throw std::runtime_error("Unsupported parameter-only snapshot");
        std::vector<oscairn_sound_parameter_t> values;uint32_t id;double value;
        while(input>>std::ws && !input.eof()) {
            if(values.size()>=kick::clap::Host::maxEvents || !(input>>id>>value) || !std::isfinite(value))throw std::runtime_error("Invalid parameter snapshot");
            const auto& all=parameters();const auto p=std::find_if(all.begin(),all.end(),[&](const auto& p){return p.id==id;});
            if(p==all.end() || (p->flags&CLAP_PARAM_IS_READONLY) || value<p->minimum || value>p->maximum || ((p->flags&CLAP_PARAM_IS_STEPPED) && value!=std::trunc(value)) ||
               std::any_of(values.begin(),values.end(),[&](const auto& v){return v.id==id;}))throw std::runtime_error("Snapshot parameter does not match plugin");
            values.push_back({id,value});
        }
        if(values.size()!=static_cast<size_t>(std::count_if(parameters().begin(),parameters().end(),[](const auto& p){return !(p.flags&CLAP_PARAM_IS_READONLY);})))throw std::runtime_error("Incomplete parameter snapshot");
        for(const auto& v:values)if(!setParameter(v.id,v.value))throw std::runtime_error("Snapshot parameter queue full");
    }

    std::vector<uint32_t> PluginHost::mainAudioPorts(bool input) const
    {
        const auto& ports=input?inputPorts():outputPorts();
        const auto main=std::find_if(ports.begin(),ports.end(),[](const auto& p){return p.flags&CLAP_AUDIO_PORT_IS_MAIN;});
        if(main!=ports.end())return {main->id};
        if(ports.size()==1)return {ports.front().id};
        throw std::invalid_argument("No unambiguous main audio bus; specify port/channel routes");
    }
    namespace
    {
        PlacementPolicy& policy()
        {
            static PlacementPolicy p=[]{
                PlacementPolicy initial;if(const char* w=std::getenv("OSCAIRN_PLUGIN_WORKER"))initial.worker=w;
                if(const char* mb=std::getenv("OSCAIRN_WORKER_MEMORY_MB")){char* end=nullptr;const auto n=std::strtoull(mb,&end,10);
                    if(*mb>='0' && *mb<='9' && !*end)initial.memoryLimit=n<<20;
                    else std::fprintf(stderr,"oscairn: OSCAIRN_WORKER_MEMORY_MB=%s is not a whole number of megabytes, ignored (the limit stays %llu)\n",mb,(unsigned long long)(initial.memoryLimit>>20));}
                if(const char* s=std::getenv("OSCAIRN_PLUGIN_SCAN"))initial.scan=s;
                if(const char* h=std::getenv("OSCAIRN_PLUGIN_HOSTS");h && *h){char* end=nullptr;const long n=std::strtol(h,&end,10);if(!*end && n>=0 && n<=64)initial.hosts=uint32_t(n);}
                if(const char* list=std::getenv("OSCAIRN_OWN_HOST"))for(std::string_view rest=list;!rest.empty();){
                    const auto comma=rest.find(',');if(const auto id=rest.substr(0,comma);!id.empty())initial.ownHost.emplace(id);
                    rest=comma==rest.npos?std::string_view():rest.substr(comma+1);}
                return initial;}();
            return p;
        }
    }
    void setPlacementPolicy(PlacementPolicy p) {policy()=std::move(p);}
    const PlacementPolicy& placementPolicy() {return policy();}
    uint64_t residentBytes(long pid)
    {
        if(pid == 0) pid = long(getpid());
#ifdef __APPLE__
        proc_taskinfo info{};
        if(proc_pidinfo(int(pid), PROC_PIDTASKINFO, 0, &info, sizeof(info)) != int(sizeof(info))) return 0;
        return info.pti_resident_size;
#else
        std::ifstream statm("/proc/" + std::to_string(pid) + "/statm");
        uint64_t pages = 0, resident = 0;
        if(!(statm >> pages >> resident)) return 0;
        return resident * uint64_t(sysconf(_SC_PAGESIZE));
#endif
    }

    void disableTransparentHugePages(const char* who)
    {
#ifdef __linux__
        const char* keep = std::getenv("OSCAIRN_THP");
        if(keep && std::string(keep) == "keep") return;
        if(prctl(PR_SET_THP_DISABLE, 1, 0, 0, 0) != 0) std::fprintf(stderr, "%s: transparent huge pages not disabled (%s)\n", who, std::strerror(errno));
#else
        (void)who;
#endif
    }

    bool holdCpuWakeLatency(bool hold, const char* device)
    {
#ifdef __linux__
        static std::mutex mutex;
        static int fd = -1;
        static bool said = false;
        std::lock_guard lock(mutex);
        if(!hold) { if(fd >= 0) { ::close(fd); fd = -1; } return false; }
        if(fd >= 0) return true;
        if(const char* keep = std::getenv("OSCAIRN_CPU_LATENCY"); keep && std::string(keep) == "keep") return false;
        fd = ::open(device, O_WRONLY | O_CLOEXEC);
        const int32_t none = 0;
        if(fd >= 0 && ::write(fd, &none, sizeof none) == ssize_t(sizeof none)) return true;
        const int e = errno;
        if(fd >= 0) { ::close(fd); fd = -1; }
        if(!said) { said = true; std::fprintf(stderr, "oscairn: no CPU wake-up latency request (%s: %s); idle cores may sleep deeply between blocks\n", device, std::strerror(e)); }
        return false;
#else
        (void)hold; (void)device;
        return false;
#endif
    }

    std::string applyMemoryPolicy(int oomScoreAdj)
    {
        // The vDSO's clock data page cannot be locked and is mapped into the process at its first read: a
        // worker whose first clock read was its audio thread's took a fault there. Read now, at the start.
        (void)std::chrono::steady_clock::now();
        const char* level=std::getenv("OSCAIRN_RELIABILITY");
        if(!level || (std::string(level)!="instrument" && std::string(level)!="pass-through"))return {};
#ifdef __linux__
        std::string missed;
        // Faulted in when allocated, during preparation, never during a block; code pages stay resident too.
        // MCL_FUTURE past a finite RLIMIT_MEMLOCK does not fail here: every later mmap past the limit
        // fails, so a plugin load or a recorder ring throws instead of a degraded lock. Lock the
        // future only where the limit cannot be reached (the device image; RFC 0028).
        rlimit memlock{};
        const bool unlimited = getrlimit(RLIMIT_MEMLOCK, &memlock) == 0 && memlock.rlim_cur == RLIM_INFINITY;
        const int flags = MCL_CURRENT | (unlimited ? MCL_FUTURE : 0);
        // Locked, a thread's whole stack is resident from its start: glibc's default is RLIMIT_STACK,
        // 8 MiB, which made thread stacks 82 of the engine's 118 MB resident and 8 of a worker's 16.
        // Threads made from here on without a size of their own get threadStack (plugin_host.h);
        // the threads off the audio path that are Oscairn's own (ServiceThread) give back what they
        // do not touch. The main thread's stack is not a thread's and is unchanged.
        pthread_attr_t stack;
        const bool sized = pthread_attr_init(&stack) == 0 && pthread_attr_setstacksize(&stack, threadStack) == 0 && pthread_setattr_default_np(&stack) == 0;
        pthread_attr_destroy(&stack);
#ifdef __GLIBC__
        // Each glibc arena is a 64 MiB reservation (no memory until used), and what its threads freed
        // stays in it, locked; at most two keep that spread small. The audio threads do not allocate.
        mallopt(M_ARENA_MAX, 2);
#endif
        if(mlockall(flags) != 0) missed += std::string("memory not locked (") + std::strerror(errno) + "; RLIMIT_MEMLOCK too low?)";
        else
        {
            if(!unlimited) missed += "future memory not locked: RLIMIT_MEMLOCK is " + std::to_string(uint64_t(memlock.rlim_cur));
            // Libraries no audio thread calls into: TLS, crypto and network (libcurl's, for package
            // downloads on the control thread), 12 MB of the engine's locked memory in the Linux container.
            // Their pages may be reclaimed and are faulted back when a download needs them.
            static constexpr std::string_view offPath[]{"libcurl", "libssl", "libcrypto", "libgnutls", "libnettle", "libhogweed", "libgmp", "libtasn1",
                "libp11-kit", "libunistring", "libidn2", "libpsl", "libkrb5", "libk5crypto", "libkrb5support", "libgssapi", "libcom_err", "libkeyutils",
                "libldap", "liblber", "libsasl2", "librtmp", "libssh", "libnghttp2", "libbrotli"};
            std::ifstream maps("/proc/self/maps");
            for(std::string line; std::getline(maps, line);)
            {
                const auto slash = line.rfind('/');
                if(slash == std::string::npos) continue;
                const std::string_view file(line.c_str() + slash + 1);
                if(std::none_of(std::begin(offPath), std::end(offPath), [&](std::string_view l) { return file.substr(0, l.size()) == l && file.size() > l.size() && (file[l.size()] == '.' || file[l.size()] == '-' || file[l.size()] == '_'); })) continue;
                unsigned long from = 0, to = 0;
                if(std::sscanf(line.c_str(), "%lx-%lx", &from, &to) == 2 && to > from) munlock(reinterpret_cast<void*>(from), to - from);
            }
        }
        // Lowering it below 0 needs CAP_SYS_RESOURCE; raising it does not.
        std::ofstream adj("/proc/self/oom_score_adj");adj<<oomScoreAdj<<std::flush;
        if(!adj)missed+=std::string(missed.empty()?"":"; ")+"oom_score_adj not set to "+std::to_string(oomScoreAdj);
        if(!sized)missed+=std::string(missed.empty()?"":"; ")+"thread stacks left at their default size";
        return missed;
#else
        (void)oomScoreAdj;
        return "memory locking and the out-of-memory order are Linux only";
#endif
    }

    namespace
    {
        void* serve(void* run) noexcept
        {
            std::unique_ptr<std::function<void()>> f(static_cast<std::function<void()>*>(run));
#ifdef __linux__
            // Below this frame, less a page for the calls just made, nothing is in use yet.
            pthread_attr_t a;
            if(pthread_getattr_np(pthread_self(), &a) == 0)
            {
                void* low = nullptr; size_t size = 0;
                const auto page = uintptr_t(sysconf(_SC_PAGESIZE));
                const auto end = (uintptr_t(__builtin_frame_address(0)) - page) & ~(page - 1);
                if(pthread_attr_getstack(&a, &low, &size) == 0 && end > uintptr_t(low))
                {
                    munlock(low, end - uintptr_t(low));
                    madvise(low, end - uintptr_t(low), MADV_DONTNEED);
                }
                pthread_attr_destroy(&a);
            }
#endif
            try { (*f)(); } catch(...) { std::terminate(); } // as std::thread does
            return nullptr;
        }
    }

    ServiceThread::ServiceThread(std::function<void()> run)
    {
        auto f = std::make_unique<std::function<void()>>(std::move(run));
        pthread_attr_t a;
        int e = pthread_attr_init(&a);
        if(!e) e = pthread_attr_setstacksize(&a, threadStack);
        if(!e) e = pthread_create(&thread_, &a, serve, f.get());
        pthread_attr_destroy(&a);
        if(e) throw std::system_error(e, std::generic_category(), "Cannot start a thread");
        f.release(); running_ = true;
    }

    ServiceThread& ServiceThread::operator=(ServiceThread&& o) noexcept
    {
        if(running_) std::terminate();
        thread_ = o.thread_; running_ = std::exchange(o.running_, false);
        return *this;
    }

    void ServiceThread::join()
    {
        if(!running_) throw std::system_error(EINVAL, std::generic_category(), "No thread to join");
        if(const int e = pthread_join(thread_, nullptr)) throw std::system_error(e, std::generic_category(), "Cannot join a thread");
        running_ = false;
    }

    std::string applyCpuPolicy()
    {
        const char* cgroup=std::getenv("OSCAIRN_AUDIO_CGROUP");
        if(!cgroup || !*cgroup)return {};
#ifdef __linux__
        std::ofstream procs(std::string(cgroup)+"/cgroup.procs");procs<<getpid()<<std::flush;
        return procs?std::string():"not on the audio cores: "+std::string(cgroup)+"/cgroup.procs: "+std::strerror(errno);
#else
        return "audio cores are Linux only";
#endif
    }

    std::vector<PresetInfo> scanPluginPresets(const std::string& path, const std::string& pluginId)
    {
        // One listing per binary as it is now (its path and newest modification), the failures of a
        // helper that ran included: a hung provider costs one helper timeout, not one per caller or
        // refresh. A helper that could not be started (no pipe, no process, no thread) is not kept.
        // shortcut: preset files added to a plugin's folders show once its binary changes or the engine restarts; key on the folders too if that matters.
        struct Listing { kick::clap::ScanResult result; std::string failure; bool started = true; };
        // Never destroyed: a listing's thread may still be running when the process exits.
        static auto& mutex = *new std::mutex;
        static auto& listings = *new std::map<std::pair<std::string, std::filesystem::file_time_type>, std::shared_future<Listing>>;
        const auto helper = placementPolicy().scan;
        if(helper.empty()) throw std::runtime_error("Presets not listed: no plugin scan helper is configured");
        std::error_code e;
        auto stamp = std::filesystem::last_write_time(path, e);
        if(std::filesystem::is_directory(path, e)) // a bundle: its binary may change inside it
            for(const auto& f : std::filesystem::recursive_directory_iterator(path, e)) stamp = std::max(stamp, f.last_write_time(e));
        const auto key = std::pair{path, stamp};
        std::promise<Listing> promise;
        std::shared_future<Listing> listing;
        bool run = false;
        {
            std::lock_guard lock(mutex);
            auto& slot = listings[key];
            if(!slot.valid()) { slot = promise.get_future().share(); run = true; }
            listing = slot;
        }
        if(run)
            // Off the caller's thread, which is the engine's control thread (presets.list) or the
            // desktop's message thread: neither waits on a provider. The scan's own 15 s bounds the
            // helper, and a hung provider dies with it.
            try
            {
                std::thread([helper, key, promise = std::move(promise)]() mutable {
                    Listing l;
                    const bool ran = kick::clap::indexPresetsWithHelper(helper, key.first, 15000, l.result, l.failure);
                    if(!ran && l.failure.empty()) l.failure = "Plugin scan failed";
                    l.started = ran || !l.failure.starts_with("Cannot "); // runHelper's "Cannot ...": the helper never started
                    promise.set_value(std::move(l));
                }).detach();
            }
            catch(const std::system_error& error)
            {
                { std::lock_guard lock(mutex); listings.erase(key); }
                throw std::runtime_error(std::string("Presets not listed: cannot start the listing: ") + error.what());
            }
        if(listing.wait_for(std::chrono::seconds(0)) != std::future_status::ready) throw std::runtime_error("Presets are being listed; ask again");
        const auto& l = listing.get();
        if(!l.started) { std::lock_guard lock(mutex); listings.erase(key); } // said once, then asked again
        if(!l.failure.empty()) throw std::runtime_error("Presets not listed: " + l.failure);
        std::vector<PresetInfo> presets;
        for(const auto& plugin : l.result.plugins)
            if(plugin.id == pluginId) for(const auto& preset : plugin.presets) presets.push_back({presetUri(preset), preset.name});
        return presets;
    }

    namespace { std::atomic<uint64_t> instancesLoaded{0}; }
    uint64_t pluginInstancesLoaded() noexcept { return instancesLoaded.load(std::memory_order_relaxed); }
    std::shared_ptr<PluginHost> loadPlugin(const std::string& path,const std::string& id,double rate,uint32_t frames,ProcessingMode mode,Lv2BufferLimits lv2Buffers,uint64_t memoryRoom)
    {
        (void)lv2Buffers; // LV2-only resource policy.
        instancesLoaded.fetch_add(1,std::memory_order_relaxed);
        if(path.find('\0')!=std::string::npos || id.find('\0')!=std::string::npos)
            throw std::invalid_argument("Plugin path and ID must not contain NUL bytes");
        auto source=std::filesystem::path(path).lexically_normal();
        if(source.filename().empty())source=source.parent_path();
        if(source.extension()==".lv2")
        {
#ifdef OSCAIRN_LV2
            return loadLv2Plugin(path,id,rate,frames,mode,lv2Buffers);
#else
            throw std::runtime_error("LV2 support is disabled; build with -DOSCAIRN_LV2=ON");
#endif
        }
        const auto& policy=placementPolicy();
        const auto placement=policy.plugins.find(id);
        if(!policy.worker.empty() && (placement==policy.plugins.end()?policy.standard:placement->second)==Placement::Worker)
            return loadPluginInWorker(policy.worker,path,id,rate,frames,mode,policy.memoryLimit,memoryRoom);
        return std::make_shared<ClapHost>(checked(OSCAIRN_CLAP(new)(path.c_str(),id.c_str(),rate,frames,mode==ProcessingMode::Offline,memoryRoom)));
    }
}
