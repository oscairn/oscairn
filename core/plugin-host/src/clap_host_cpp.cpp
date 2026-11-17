// SPDX-License-Identifier: AGPL-3.0-or-later
// clap_host.h's C++ reference: kick::clap::Host, and what plugin_host.cpp's loadPlugin and ClapHost did around it,
// moved here unchanged.
#include "clap_host.h"
#include "StateFiles.h"
#include "../../clap-host/src/Internal.h"
#include <algorithm>
#include <cstddef>
#include <cstdio>
#include <filesystem>

static_assert(sizeof(oscairn_clap_event) == 64 && alignof(oscairn_clap_event) == 8);
static_assert(sizeof(oscairn_clap_wake) == 4 && std::atomic<uint32_t>::is_always_lock_free);
// kick::clap::Statistics as the C struct lays it out: every field 8 bytes, in the C struct's order.
#define OSCAIRN_STATISTICS_AT(field, index) static_assert(offsetof(kick::clap::Statistics, field) == 8 * (index));
OSCAIRN_STATISTICS_AT(blocks, 0) OSCAIRN_STATISTICS_AT(droppedEvents, 1) OSCAIRN_STATISTICS_AT(processErrors, 2)
OSCAIRN_STATISTICS_AT(lastMicroseconds, 3) OSCAIRN_STATISTICS_AT(maximumMicroseconds, 4) OSCAIRN_STATISTICS_AT(deadlineMisses, 5)
OSCAIRN_STATISTICS_AT(controlSilences, 6) OSCAIRN_STATISTICS_AT(idleBlocks, 7) OSCAIRN_STATISTICS_AT(restarts, 8)
OSCAIRN_STATISTICS_AT(lateBlocks, 9) OSCAIRN_STATISTICS_AT(silentBlocks, 10) OSCAIRN_STATISTICS_AT(allowanceMicroseconds, 11)
OSCAIRN_STATISTICS_AT(busyBlocks, 12) OSCAIRN_STATISTICS_AT(missPanics, 13) OSCAIRN_STATISTICS_AT(minorFaults, 14)
OSCAIRN_STATISTICS_AT(majorFaults, 15) OSCAIRN_STATISTICS_AT(involuntarySwitches, 16) OSCAIRN_STATISTICS_AT(calledBlocks, 17)
OSCAIRN_STATISTICS_AT(restedBlocks, 18) OSCAIRN_STATISTICS_AT(cpuMicroseconds, 19)
static_assert(sizeof(kick::clap::Statistics) == 8 * 20);
#undef OSCAIRN_STATISTICS_AT

// An oscairn_clap_assets table as the kick::clap::AssetResolver the host takes: what it says, thrown when it fails.
struct TableResolver final : kick::clap::AssetResolver
{
    oscairn_clap_assets table{};
    static std::string text(const char* p) { return p; }
    std::string resolve(const std::string& library, const std::string& path, std::string& sha256) override
    {
        oscairn_asset_ref_t ref{}; char file[65536], message[512]{};
        std::snprintf(ref.library, sizeof ref.library, "%s", library.c_str()); std::snprintf(ref.path, sizeof ref.path, "%s", path.c_str()); std::snprintf(ref.sha256, sizeof ref.sha256, "%s", sha256.c_str());
        if(!table.resolve(table.context, &ref, file, sizeof file, message, sizeof message)) throw std::runtime_error(message);
        sha256 = ref.sha256; return file;
    }
    std::string libraryRoot(const std::string& library, const std::string& minVersion) const override
    {
        char path[65536]; return table.library_root(table.context, library.c_str(), minVersion.c_str(), path, sizeof path) ? path : std::string();
    }
    kick::clap::AssetReference create(const std::string& file) override
    {
        oscairn_asset_ref_t ref{}; char message[512]{};
        if(!table.create(table.context, file.c_str(), &ref, message, sizeof message)) throw std::runtime_error(message);
        return {ref.library, ref.path, ref.sha256};
    }
    SampleLocation resolveSample(const std::string& id) override
    {
        oscairn_asset_ref_t ref{}; std::snprintf(ref.path, sizeof ref.path, "sample:%s", id.c_str());
        char pack[65536], message[512]{}; oscairn_sample_location_t at{};
        if(!table.resolve_sample(table.context, &ref, pack, sizeof pack, &at, message, sizeof message)) throw std::runtime_error(message);
        return {pack, at.offset, at.length, at.preload};
    }
};

struct oscairn_clap_host { kick::clap::Host host; TableResolver assets; };

namespace
{
    namespace state_files = oscairn::state_files;
    thread_local std::string failure;
    thread_local int failureKind = 0;
    // f() as a C call: what it throws becomes this thread's failure and `failed` is returned.
    template<class F, class R> R guard(F&& f, R failed) noexcept
    {
        try { return f(); }
        catch(const std::invalid_argument& e) { failureKind = OSCAIRN_CLAP_INVALID_ARGUMENT; failure = e.what(); }
        catch(const std::logic_error& e) { failureKind = OSCAIRN_CLAP_LOGIC_ERROR; failure = e.what(); }
        catch(const std::exception& e) { failureKind = OSCAIRN_CLAP_RUNTIME_ERROR; failure = e.what(); }
        catch(...) { failureKind = OSCAIRN_CLAP_RUNTIME_ERROR; failure = "Unknown error"; }
        return failed;
    }
    template<class F> bool guard(F&& f) noexcept { return guard([&]{ f(); return true; }, false); }
    template<class F> int guardInt(F&& f) noexcept { return guard([&]{ return f() ? 1 : 0; }, -1); }
    // The plugins a binary offers, as Discovery.cpp's inspect checked them; what fails is thrown.
    std::vector<kick::clap::Descriptor> plugins(const std::string& path)
    {
        const auto library=kick::clap::detail::openLibrary(path);
        const auto* factory=library->factory();
        if(!factory)throw std::runtime_error("No plugin factory");
        const auto count=factory->get_plugin_count(factory);
        if(!count || count>4096)throw std::runtime_error("Invalid factory descriptor count");
        std::vector<kick::clap::Descriptor> all;
        for(uint32_t i=0;i<count;++i){
            const auto* d=factory->get_plugin_descriptor(factory,i);
            if(!d || !d->id || !d->id[0] || !d->name || !d->name[0] || !clap_version_is_compatible(d->clap_version))throw std::runtime_error("Invalid or incompatible plugin descriptor");
            if(std::any_of(all.begin(),all.end(),[&](const auto& p){return p.id==d->id;}))throw std::runtime_error("Duplicate plugin ID in factory");
            all.push_back(kick::clap::detail::describe(library->path,*d));
        }
        return all;
    }
    kick::clap::Preset preset(const oscairn_clap_preset& p) { return {p.name, p.kind, p.location, p.load_key}; }
    oscairn_clap_preset preset(const kick::clap::Preset& p) { return {p.name.c_str(), p.kind, p.location.c_str(), p.loadKey.c_str()}; }
}

extern "C"
{
const char* cpp_clap_error(int* kind) { if(kind) *kind = failureKind; return failure.c_str(); }

oscairn_clap_host* cpp_clap_new(const char* path, const char* id, double rate, uint32_t frames, bool offline, uint64_t memoryRoom)
{
    return guard([&]{
        const std::string pluginId = id;
        const auto scan=plugins(path); // through this host's own library registry, not the Rust host's discovery
        const auto found=std::find_if(scan.begin(),scan.end(),[&](const auto& p){return pluginId.empty() || p.id==pluginId;});
        if(found==scan.end())throw std::runtime_error("No matching CLAP plugin in "+std::string(path));
        auto host=std::make_unique<oscairn_clap_host>();
        host->host.setMemoryRoom(memoryRoom);
        host->host.setOfflineRendering(offline);
        if(!host->host.load(*found,rate,frames))throw std::runtime_error("Cannot load "+found->id+": "+host->host.error());
        host->host.setMemoryRoom(0);
        host->host.lockAudioLayout();
        // The first service (the plugin's init callback, its parameter flush) holds the gate; run it
        // before any audio thread can see the plugin, or its first block is refused as a control silence.
        host->host.service();
        return host.release();
    }, static_cast<oscairn_clap_host*>(nullptr));
}
oscairn_clap_host* cpp_clap_effect_context(oscairn_clap_host* source)
{
    return guard([&]{
        auto child=std::make_unique<oscairn_clap_host>();
        if(!child->host.loadEffectContext(source->host))throw std::runtime_error(child->host.error());
        child->assets.table = source->assets.table; // its own copy: the source may go first
        if(source->assets.table.resolve) child->host.setAssets(&child->assets);
        child->host.lockAudioLayout();return child.release();
    }, static_cast<oscairn_clap_host*>(nullptr));
}
void cpp_clap_free(oscairn_clap_host* h) { delete h; }

uint64_t cpp_clap_revision(const oscairn_clap_host* h) { return h->host.metadataRevision(); }
void cpp_clap_describe(const oscairn_clap_host* h, const oscairn_clap_description* to)
{
    const auto& d = h->host.descriptor();
    const oscairn_clap_descriptor descriptor{d.path.c_str(), d.id.c_str(), d.name.c_str(), d.vendor.c_str(), d.version.c_str(), d.description.c_str(), d.presetDiscovery};
    to->descriptor(to->context, &descriptor);
    for(const auto& f : d.features) to->feature(to->context, f.c_str());
    for(const auto& p : d.presets) { const auto c = preset(p); to->preset(to->context, &c); }
    for(const auto& [number, name] : d.lanes) to->lane(to->context, number, name.c_str());
    for(const bool input : {true, false})
        for(const auto& p : input ? h->host.inputPorts() : h->host.outputPorts()) { const oscairn_clap_port c{p.id, p.name.c_str(), p.channels, p.flags}; to->port(to->context, input, &c); }
    for(const auto& p : h->host.parameters())
    {
        const oscairn_clap_parameter c{p.id, p.name.c_str(), p.module.c_str(), p.minimum, p.maximum, p.defaultValue, p.flags, p.widget, p.priority, p.rangeHigh,
                                       p.group, p.role, p.view, p.field, p.index, reinterpret_cast<const uint8_t(*)[11]>(p.algorithms.data()), uint32_t(p.algorithms.size()),
                                       p.seconds.data(), uint32_t(p.seconds.size())};
        to->parameter(to->context, &c);
    }
}
uint32_t cpp_clap_note_input_ports(const oscairn_clap_host* h) { return h->host.noteInputPorts(); }
uint32_t cpp_clap_note_output_ports(const oscairn_clap_host* h) { return h->host.noteOutputPorts(); }
bool cpp_clap_uses_clap_notes(const oscairn_clap_host* h, uint32_t port) { return h->host.usesClapNotes(port); }
bool cpp_clap_takes_sysex(const oscairn_clap_host* h) { return h->host.takesSysex(); }
bool cpp_clap_emits_sysex(const oscairn_clap_host* h) { return h->host.emitsSysex(); }
bool cpp_clap_supports_state(const oscairn_clap_host* h) { return h->host.supportsState(); }
bool cpp_clap_supports_sound_snapshot(const oscairn_clap_host* h) { return h->host.supportsSoundSnapshot(); }
bool cpp_clap_has_window(const oscairn_clap_host* h) { return h->host.hasGui(); }
bool cpp_clap_voice_budget(const oscairn_clap_host* h) { return h->host.voiceBudget(); }
bool cpp_clap_honors_volume(const oscairn_clap_host* h) { return h->host.honorsVolume(); }

bool cpp_clap_service(oscairn_clap_host* h) { return guard([&]{ h->host.service(); }); }
bool cpp_clap_reset(oscairn_clap_host* h) { return guard([&]{ h->host.reset(); }); }
bool cpp_clap_reconfigure(oscairn_clap_host* h, double rate, uint32_t frames)
{
    return guard([&]{ if(!h->host.reconfigure(rate,frames))throw std::runtime_error(h->host.error()); });
}
int cpp_clap_effect_resources(const oscairn_clap_host* h, oscairn_effect_resources_t* r) { return guardInt([&]{ return h->host.effectResources(*r); }); }
int cpp_clap_declared_memory(const oscairn_clap_host* h, oscairn_memory_t* m) { return guardInt([&]{ return h->host.declaredMemory(*m); }); }
void cpp_clap_set_memory_room(oscairn_clap_host* h, uint64_t bytes) { h->host.setMemoryRoom(bytes); }
void cpp_clap_set_active_outputs(oscairn_clap_host* h, uint64_t ports) { h->host.setActiveOutputs(ports); }
void cpp_clap_set_assets(oscairn_clap_host* h, const oscairn_clap_assets* assets)
{
    if(assets) h->assets.table = *assets;
    h->host.setAssets(assets ? &h->assets : nullptr);
}
bool cpp_clap_asset_dependencies(const oscairn_clap_host* h, void (*each)(void*, const oscairn_clap_asset*), void* context)
{
    std::vector<kick::clap::AssetReference> all;
    if(!guard([&]{ all = h->host.assetDependencies(); })) return false;
    for(const auto& a : all) { const oscairn_clap_asset c{a.library.c_str(), a.path.c_str(), a.sha256.c_str(), a.optional}; each(context, &c); }
    return true;
}
bool cpp_clap_show_window(oscairn_clap_host* h, bool show, const char* title)
{
    return guard([&]{ if(!show)h->host.closeGui();else if(!h->host.showGui(title))throw std::runtime_error(h->host.error()); });
}
bool cpp_clap_presets(oscairn_clap_host* h, void (*each)(void*, const oscairn_clap_preset*), void* context)
{
    std::vector<kick::clap::Preset> all;
    if(!guard([&]{ all = h->host.presets(); })) return false;
    for(const auto& p : all) { const auto c = preset(p); each(context, &c); }
    return true;
}
bool cpp_clap_load_preset(oscairn_clap_host* h, const oscairn_clap_preset* p)
{
    return guard([&]{ if(!h->host.loadPreset(preset(*p)))throw std::runtime_error(h->host.error()); });
}

bool cpp_clap_save_state(oscairn_clap_host* h, const char* directory)
{
    return guard([&]{
        auto& host = h->host;
        state_files::Bundle bundle(directory);std::vector<uint8_t> bytes;
        if(!host.saveState(bytes))throw std::runtime_error(host.error());
        const auto& id=host.descriptor().id;
        state_files::write(bundle.temporary.path/"plugin.id",id.data(),id.size());
        state_files::write(bundle.temporary.path/"state.clap",bytes.data(),bytes.size());
        bundle.publish();
    });
}
bool cpp_clap_load_state(oscairn_clap_host* h, const char* directory)
{
    return guard([&]{
        auto& host = h->host;
        const auto path=std::filesystem::canonical(directory);
        const auto id=state_files::read(path/"plugin.id",65536);
        if(std::string(id.begin(),id.end())!=host.descriptor().id)throw std::runtime_error("State belongs to a different CLAP plugin");
        const auto bytes=state_files::read(path/"state.clap",kick::clap::Host::maxStateBytes);
        if(!host.loadState(bytes))throw std::runtime_error("CLAP state restore failed: "+host.error());
    });
}

double cpp_clap_parameter_value(const oscairn_clap_host* h, uint32_t id) { return h->host.parameterValue(id); }
bool cpp_clap_parameter_text(const oscairn_clap_host* h, uint32_t id, double value, oscairn_clap_text text, void* context)
{
    std::string s;
    if(!guard([&]{ s = h->host.parameterText(id, value); })) return false;
    text(context, s.data(), s.size());
    return true;
}
bool cpp_clap_set_parameter(oscairn_clap_host* h, uint32_t id, double value) { return h->host.setParameter(id, value); }
bool cpp_clap_parameter_gesture(oscairn_clap_host* h, uint32_t id, bool begin) { return h->host.parameterGesture(id, begin); }
uint64_t cpp_clap_parameter_reports(const oscairn_clap_host* h) { return h->host.parameterReports(); }
uint64_t cpp_clap_value_rescans(const oscairn_clap_host* h) { return h->host.valueRescans(); }
bool cpp_clap_send_event(oscairn_clap_host* h, const oscairn_clap_event* e) { return h->host.enqueue(*e); }
bool cpp_clap_pop_output(oscairn_clap_host* h, oscairn_clap_event* e) { return h->host.popOutput(*e); }
uint64_t cpp_clap_output_event_drops(const oscairn_clap_host* h) { return h->host.outputEventDrops(); }
void cpp_clap_statistics(const oscairn_clap_host* h, oscairn_clap_statistics* s) { *s = h->host.statistics(); }

uint32_t cpp_clap_sound_context_capacity(const oscairn_clap_host* h) { return h->host.soundContextCapacity(); }
bool cpp_clap_sound_contexts(const oscairn_clap_host* h, void (*each)(void*, const oscairn_sound_context_info_t*), void* context)
{
    std::vector<oscairn_sound_context_info_t> all;
    if(!guard([&]{ all = h->host.soundContexts(); })) return false;
    for(const auto& c : all) each(context, &c);
    return true;
}
bool cpp_clap_prepare_sound(oscairn_clap_host* h, const char* name, const oscairn_sound_parameter_t* parameters, uint32_t count,
                            const char* patch, const char* library, const char* directory, uint32_t* id)
{
    return guard([&]{ *id = h->host.prepareSound(name, std::vector<oscairn_sound_parameter_t>(parameters, parameters + count), patch, library, directory); });
}
bool cpp_clap_prepare_preset(oscairn_clap_host* h, const char* name, const oscairn_clap_preset* p, uint32_t* id)
{
    return guard([&]{ *id = h->host.prepareSound(name, preset(*p)); });
}
int cpp_clap_select_sound(oscairn_clap_host* h, uint32_t id, int16_t channel) { return guardInt([&]{ return h->host.selectSound(id, channel); }); }
int cpp_clap_discard_sound(oscairn_clap_host* h, uint32_t id) { return guardInt([&]{ return h->host.discardSound(id); }); }
bool cpp_clap_prepares_snapshots(const oscairn_clap_host* h) { return h->host.preparesSoundSnapshots(); }
bool cpp_clap_prepare_snapshot(oscairn_clap_host* h, const char* directory, uint32_t ids[16])
{
    return guard([&]{
        auto& host = h->host;
        // What saveSnapshot wrote for a native engine: "state 1" and its state bundle.
        const auto path=std::filesystem::path(directory);
        const auto head=state_files::read(path/"snapshot",64);
        if(std::string(head.begin(),head.end())!="state 1\n")throw std::runtime_error("Not a native sound snapshot");
        const auto id=state_files::read(path/"state"/"plugin.id",65536);
        if(std::string(id.begin(),id.end())!=host.descriptor().id)throw std::runtime_error("State belongs to a different CLAP plugin");
        const auto selected=host.prepareSoundSnapshot(state_files::read(path/"state"/"state.clap",kick::clap::Host::maxStateBytes));
        std::copy(selected.begin(), selected.end(), ids);
    });
}
bool cpp_clap_saves_sounds(const oscairn_clap_host* h) { return h->host.savesSounds(); }
bool cpp_clap_save_sound(oscairn_clap_host* h, uint32_t id, const char* directory)
{
    return guard([&]{
        auto& host = h->host;
        state_files::Bundle bundle(directory);
        const auto bytes=host.saveSoundState(id);const auto& plugin=host.descriptor().id;
        std::filesystem::create_directory(bundle.temporary.path/"state");
        state_files::write(bundle.temporary.path/"state"/"plugin.id",plugin.data(),plugin.size());
        state_files::write(bundle.temporary.path/"state"/"state.clap",bytes.data(),bytes.size());
        const std::string head="state 1\n";state_files::write(bundle.temporary.path/"snapshot",head.data(),head.size());
        bundle.publish();
    });
}
bool cpp_clap_sound_values(const oscairn_clap_host* h, uint32_t id, void (*each)(void*, const oscairn_sound_parameter_t*), void* context)
{
    std::vector<oscairn_sound_parameter_t> all;
    if(!guard([&]{ all = h->host.soundValues(id); })) return false;
    for(const auto& v : all) each(context, &v);
    return true;
}
bool cpp_clap_rings_out(const oscairn_clap_host* h) { return h->host.ringsOut(); }
uint32_t cpp_clap_wants_events(const oscairn_clap_host* h) { return h->host.wantsEvents(); }
int cpp_clap_choke_retiring(oscairn_clap_host* h) { return guardInt([&]{ return h->host.chokeRetiringSounds(); }); }

bool cpp_clap_process(oscairn_clap_host* h, const float* const* inputs, uint32_t ni, float* const* outputs, uint32_t no, uint32_t frames,
                      const oscairn_clap_event* events, uint32_t count, const clap_event_transport_t* transport)
{
    return h->host.process(inputs, ni, outputs, no, frames, events, count, transport);
}
const oscairn_clap_event* cpp_clap_output_events(const oscairn_clap_host* h, uint32_t* count) { return h->host.outputEvents(*count); }
void cpp_clap_panic(oscairn_clap_host* h) { h->host.panic(); }
bool cpp_clap_asleep(const oscairn_clap_host* h) { return h->host.asleep(); }
bool cpp_clap_restful(const oscairn_clap_host* h) { return h->host.restful(); }
void cpp_clap_rested(oscairn_clap_host* h, uint32_t frames, uint32_t blocks) { h->host.rested(frames, blocks); }
void cpp_clap_mirror_wake(oscairn_clap_host* h, oscairn_clap_wake* mirror) { h->host.mirrorWake(mirror); }
bool cpp_clap_was_reset(const oscairn_clap_host* h) { return h->host.wasReset(); }
void cpp_clap_set_voice_limits(oscairn_clap_host* h, uint32_t limit, const oscairn_voice_budget_context_t* contexts, uint32_t count)
{
    h->host.setVoiceLimits(limit, contexts, count);
}
uint32_t cpp_clap_voice_report(const oscairn_clap_host* h, double* cost, const oscairn_voice_budget_note_t** notes, uint32_t* count)
{
    return h->host.voiceReport(*cost, *notes, *count);
}
}
