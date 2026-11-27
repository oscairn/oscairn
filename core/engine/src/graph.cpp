// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/graph.h"
#include "engine_core.h"
#include "event_channel.h"
#include "PreparedInstrument.h"
#include "AudioHelpers.h"
#include "oscairn/realtime.h"

#include "oscairn/harmony.h"
#include "oscairn/sound_context.h"
#include <oscairn-clap/beat.h>
#include <oscairn-clap/param-target.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <numbers>
#include <cstdlib>
#include <cstring>
#include <stdexcept>
#include <utility>

// The layouts the engine core (engine_core.h) shares with graph.h and note_zone.h; the Rust side asserts its own (lib.rs layouts).
static_assert(sizeof(oscairn_event)==sizeof(kick::clap::Event) && alignof(oscairn_event)==alignof(kick::clap::Event));
static_assert(sizeof(oscairn_zones)==sizeof(oscairn::NoteZones) && offsetof(oscairn_zones,count)==offsetof(oscairn::NoteZones,count));
#define OSCAIRN_SAME(a,b) static_assert(offsetof(oscairn::NoteZone,a)==offsetof(oscairn_zone,b));
OSCAIRN_SAME(lowVelocity,low_velocity) OSCAIRN_SAME(channels,channels) OSCAIRN_SAME(transpose,transpose) OSCAIRN_SAME(curve,curve)
OSCAIRN_SAME(fixedVelocity,fixed_velocity) OSCAIRN_SAME(sources,sources) OSCAIRN_SAME(maxNotes,max_notes) OSCAIRN_SAME(part,part)
OSCAIRN_SAME(reserve,reserve) OSCAIRN_SAME(priority,priority) OSCAIRN_SAME(pedals,pedals) OSCAIRN_SAME(functions,functions)
#undef OSCAIRN_SAME
static_assert(sizeof(oscairn::NoteZone)==sizeof(oscairn_zone));
#define OSCAIRN_SAME(a,b) static_assert(offsetof(oscairn::Graph::Activity,a)==offsetof(oscairn_activity,b));
OSCAIRN_SAME(sounding,sounding) OSCAIRN_SAME(quiet,quiet) OSCAIRN_SAME(age,age) OSCAIRN_SAME(asleep,asleep) OSCAIRN_SAME(rest,rest)
OSCAIRN_SAME(perNote,per_note) OSCAIRN_SAME(seedRest,seed_rest) OSCAIRN_SAME(seedNote,seed_note) OSCAIRN_SAME(stolen,stolen)
OSCAIRN_SAME(hardChokes,hard_chokes) OSCAIRN_SAME(stolenForDevice,stolen_for_device) OSCAIRN_SAME(laneLimit,lane_limit)
OSCAIRN_SAME(laneTook,lane_took) OSCAIRN_SAME(badOutput,bad_output) OSCAIRN_SAME(badRun,bad_run) OSCAIRN_SAME(stranded,stranded)
OSCAIRN_SAME(ownVoices,own_voices) OSCAIRN_SAME(honorsVolume,honors_volume) OSCAIRN_SAME(perNoteScale,per_note_scale)
OSCAIRN_SAME(voiceLimit,voice_limit) OSCAIRN_SAME(overLimit,over_limit) OSCAIRN_SAME(stolenForLoss,faded_for_loss) OSCAIRN_SAME(lostBlocks,lost) OSCAIRN_SAME(stolenForDeadline,faded_for_deadline) OSCAIRN_SAME(unfitBlocks,unfit) OSCAIRN_SAME(importanceMismatch,importance_mismatch) OSCAIRN_SAME(heldOff,held_off)
#undef OSCAIRN_SAME
static_assert(sizeof(oscairn::Graph::Activity)==sizeof(oscairn_activity));

namespace oscairn
{
    // One audio-thread ledger per note connection, in the engine core; shared by successive compiled plans.
    // ports: its note ports, MIDI's notes on every one; clap: a bit for each taking CLAP notes.
    struct NoteRoute
    {
        oscairn_route* const ledger;
        explicit NoteRoute(uint32_t ports,uint16_t clap=0,uint32_t fadeFrames=0):ledger(rust_core_route_new(ports,clap,fadeFrames)) {}
        ~NoteRoute() { rust_core_route_free(ledger); }
        NoteRoute(const NoteRoute&)=delete;
        NoteRoute& operator=(const NoteRoute&)=delete;
    };
    // A C callback (engine_core.h) calling `emit` with each event.
    template<class Emit> static bool emitTo(void* emit,const oscairn_event* e) { return (*static_cast<Emit*>(emit))(*reinterpret_cast<const kick::clap::Event*>(e)); }
    static const oscairn_event* coreEvents(const kick::clap::Event* e) { return reinterpret_cast<const oscairn_event*>(e); }
    static oscairn_activity* coreActivity(Graph::Activity* a) { return reinterpret_cast<oscairn_activity*>(a); }

    // A harmony's input (setHarmonies): the notes through its zones, and the last harmony sent.
    struct Graph::HeldKeys { NoteRoute route{16u}; oscairn_harmony_t last{}; };

    /// A compiled graph: nodes in dependency order with everything the audio thread needs
    /// preallocated, so processing never allocates or locks.
    struct Graph::Plan
    {
        uint64_t replacementSerial=0;
        bool rampGains=false; // see commit()
        std::vector<NodeId> grouped; // see Graph::grouped
        struct Harmony { std::shared_ptr<Graph::HeldKeys> held; NoteZones zones; bool input=false; uint8_t root=0; uint16_t scale=1; }; // see setHarmonies
        std::vector<Harmony> harmonies;
        std::vector<Graph::ControlTarget> controls; // see setControlTargets
        struct Step
        {
            std::shared_ptr<PluginHost> host; // keeps the plugin alive while any plan uses it
            NodeId id = 0;
            std::shared_ptr<std::array<std::atomic<float>,Graph::meterChannels>> peaks; // the node's, shared with every plan
            uint32_t                          audioInputs = 0, audioOutputs = 0;
            bool ringOut = false; // the last four of audioOutputs are the plugin's ring-out buffer (Graph::ringOutPort)
            // peers: the routes sharing its part's note limit (NoteZone::part), itself included; channel:
            // into a shared engine's context (RFC 0021 slice 3), the context's MIDI channel, else -1.
            struct NoteSource {int step;std::shared_ptr<NoteRoute> route;NoteZones zones;bool draining=false;std::vector<oscairn_route*> peers;int16_t channel=-1;};
            bool retiring=false;std::shared_ptr<Graph::Activity> activity; // see Graph::setRetiring
            int harmony=-1; // the part's harmony it gets, in Plan::harmonies
            std::vector<NoteSource> noteSources;
            float* const* in=nullptr; float* const* out=nullptr; // its buffers, in the engine core's audio
            std::vector<oscairn_lane_route> routes; // where its param-target lanes go
            bool lanesIn = false; // lanes reach it
            bool playable = true; // a note can reach it in this plan, or its sound can come (see the plan's build); else it may rest
            uint32_t lane = 0; // the budget's lane: the thread it renders on (PluginHost::lane)
            bool inProcess = false; // it renders in the engine's process (no PluginHost::lane): within the callback's time
            // RFC 0021 slice 3: a shared engine's step renders its instance for every node playing one
            // of its sounds (a SoundContext): each node's notes come in on its channel, its parameters
            // too, and its output is the instance's bus `offset` on, with its own meter and activity.
            struct Context
            {
                NodeId id = 0; std::shared_ptr<PluginHost> host; SoundContext* sound = nullptr; int16_t channel = 0;
                uint32_t offset = 0, channels = 0;
                std::shared_ptr<std::array<std::atomic<float>,Graph::meterChannels>> peaks; std::shared_ptr<Graph::Activity> activity;
            };
            std::vector<Context> contexts;
            // RFC 0045's plugin groups: a node rendered in its sources' round trip, its audio routes (the engine core's
            // order, first: the channel's first) mixed in the worker; empty for every other step.
            struct Fused { int source; uint32_t from, to; float gain; std::array<uint8_t,4> live; bool first; };
            std::vector<Fused> fused;
        };

        struct Tap { int step; std::shared_ptr<NoteRoute> route; NoteZones zones; uint64_t ports; uint8_t channel; }; // see setMidiTaps; step -1: the input
        std::vector<Tap>               taps;
        std::vector<Step>              steps;
        std::vector<uint32_t> waves; // RFC 0034: where each wave of steps ends; a wave's steps render at the same time
        // Its note routing and shared budget (Graph::setVoiceBudget), its audio, and its block, in the engine core; last,
        // so freed before what they name (the block first).
        struct FreeNotes { void operator()(oscairn_notes* n) const noexcept { rust_core_notes_free(n); } };
        std::unique_ptr<oscairn_notes,FreeNotes> notes;
        struct FreeAudio { void operator()(oscairn_audio* a) const noexcept { rust_core_audio_free(a); } };
        std::unique_ptr<oscairn_audio,FreeAudio> audio;
        struct FreeRun { void operator()(oscairn_run* r) const noexcept { rust_core_run_free(r); } };
        std::unique_ptr<oscairn_run,FreeRun> run;
    };

    //==============================================================================
    void Graph::Retired::push( Plan* p ) noexcept
    {
        const auto head = head_.load( std::memory_order_relaxed );
        slots_[head % slots_.size()] = p;
        head_.store( head + 1, std::memory_order_release );
    }

    Graph::Plan* Graph::Retired::pop() noexcept
    {
        const auto tail = tail_.load( std::memory_order_relaxed );
        if( tail == head_.load( std::memory_order_acquire ) )
            return nullptr;
        Plan* p = slots_[tail % slots_.size()];
        tail_.store( tail + 1, std::memory_order_release );
        return p;
    }

    //==============================================================================
    static uint32_t countChannels(const std::vector<kick::clap::Port>& ports)
    {
        uint32_t count=0;std::vector<uint32_t> ids;
        for(const auto& p:ports)
        {
            if(!p.channels || p.channels>Graph::maxAudioChannels-count || std::find(ids.begin(),ids.end(),p.id)!=ids.end())
                throw std::invalid_argument("Invalid audio ports: zero/too many channels or duplicate port ID");
            count+=p.channels;ids.push_back(p.id);
        }
        return count;
    }

    Graph::Graph( double sampleRate, uint32_t maxFrames, ProcessingMode mode, Lv2BufferLimits lv2Buffers, AudioLayout audio )
        : sampleRate_( sampleRate )
        , maxFrames_( maxFrames )
        , mode_( mode )
        , lv2Buffers_( lv2Buffers ), audioLayout_(std::move(audio))
    {
        if(!std::isfinite(sampleRate) || sampleRate<=0 || !maxFrames || maxFrames>1048576)
            throw std::invalid_argument("Invalid graph sample rate or block size");
        inputChannels_=countChannels(audioLayout_.inputs);outputChannels_=countChannels(audioLayout_.outputs);
        for(auto& g:liveTarget_)g.store(1.f,std::memory_order_relaxed);
        liveFrom_.fill(1.f);liveTo_.fill(1.f);
        if(const char* serial=std::getenv("OSCAIRN_GRAPH_SERIAL");serial && *serial && *serial!='0')parallel_.store(false); // RFC 0034, for measuring
        deadlines_=worker::deadlineAllowance();
        if(const char* us=std::getenv("OSCAIRN_DEADLINE_SLACK_US");us && *us)lowSlack_=std::clamp(float(std::atof(us)),0.f,1e6f); // the profile's compute.deadlineSlack
        if(const char* us=std::getenv("OSCAIRN_WORKER_BUDGET_US");us && *us)workerBudgetUs_=float(std::chrono::duration<double,std::micro>(worker::workerAllowance(1,1e6,us)).count()); // 1 frame at 1 MHz: 0.5 us when not a valid override
        if(workerBudgetUs_<1)workerBudgetUs_=0;
        audioCores_=float(std::max(1u,worker::usableCores()-1));
        if(const char* n=std::getenv("OSCAIRN_AUDIO_CORES");n && std::atoi(n)>0)audioCores_=float(std::atoi(n)); // a small device's lanes' total on a desktop, for tests and diagnosis
        helpers_=&AudioHelpers::shared(1e6*maxFrames/sampleRate);
    }

    Graph::~Graph()
    {
        // The audio thread must have stopped calling process().
        delete pending_.exchange( nullptr );
        delete current_;
        collect();
    }

    NodeId Graph::add( const std::string& path, const std::string& pluginId, uint64_t memoryRoom )
    {
        if(nextId_>=graphInput)throw std::overflow_error("Graph node IDs exhausted");
        Node node;
        auto loaded=loadPlugin(path,pluginId,sampleRate_,maxFrames_,mode_,lv2Buffers_,memoryRoom);
        node.host = preparedInstrument(std::move(loaded),sampleRate_,maxFrames_,mode_,lv2Buffers_);node.host->setAssets(assets_); // a pool passes it on to the instances it loads later
        node.audioInputs=countChannels(node.host->inputPorts());
        node.audioOutputs=countChannels(node.host->outputPorts());

        const NodeId id = nextId_++;
        nodes_.emplace( id, std::move( node ) );
        return id;
    }

    NodeId Graph::add( std::shared_ptr<PluginHost> host )
    {
        if(nextId_>=graphInput)throw std::overflow_error("Graph node IDs exhausted");
        if(!host)throw std::invalid_argument("no plugin host");
        Node node;node.host=std::move(host);
        if(const auto* sound=dynamic_cast<const SoundContext*>(node.host.get()))node.effectGroup=sound->engine()->group;
        node.audioInputs=countChannels(node.host->inputPorts());node.audioOutputs=countChannels(node.host->outputPorts());
        const NodeId id=nextId_++;nodes_.emplace(id,std::move(node));return id;
    }
    void Graph::share(NodeId id,std::shared_ptr<PluginHost> context)
    {
        auto& node=nodes_.at(id);
        const auto* sound=dynamic_cast<const SoundContext*>(context.get());
        if(!sound || sound->engine()->host!=node.host)throw std::invalid_argument("not a sound context of the node's own instance");
        node.host=std::move(context);node.effectGroup=sound->engine()->group;
        node.audioInputs=countChannels(node.host->inputPorts());node.audioOutputs=countChannels(node.host->outputPorts());
        const auto& ports=node.host->outputPorts();
        std::erase_if(audioRoutes_,[&](const auto& r){return (r.from.node==id && std::none_of(ports.begin(),ports.end(),[&](const auto& p){return p.id==r.from.port;})) || r.to.node==id;});
    }

    NodeId Graph::addEffectContext(NodeId source) { return addEffectContext(*this, source); }
    NodeId Graph::addEffectContext(const Graph& graph,NodeId source)
    {
        if(nextId_>=graphInput)throw std::overflow_error("Graph node IDs exhausted");
        const auto& from=graph.nodes_.at(source);
        Node node;node.host=from.host->createEffectContext();node.effectGroup=from.effectGroup;
        node.audioInputs=countChannels(node.host->inputPorts());node.audioOutputs=countChannels(node.host->outputPorts());
        const NodeId id=nextId_++;nodes_.emplace(id,std::move(node));return id;
    }

    NodeId Graph::effectSource(NodeId id,const std::set<NodeId>& except) const {
        const auto& group=nodes_.at(id).effectGroup;
        for(const auto& [other,node]:nodes_)if(node.effectGroup==group && !except.count(other))return other;
        return id;
    }
    std::unique_ptr<Graph> Graph::prepareReplacement() const {
        auto next=std::make_unique<Graph>(sampleRate_,maxFrames_,mode_,lv2Buffers_,audioLayout_);
        next->nextId_=nextId_;next->assets_=assets_;return next;
    }
    void Graph::carryOver(const Graph& current,const std::set<NodeId>& ids) {
        for(const auto id:ids){if(nodes_.count(id))throw std::logic_error("a carried-over node ID is taken");auto node=current.nodes_.at(id);node.retiring=false;nodes_[id]=std::move(node);setRetiring(id,true);}
        nextId_=std::max(nextId_,current.nextId_); // a replacement prepared ahead: the current graph may have added since
        // Their live gains are frozen at what they are now: the slots are the next Performance's. Routes between the same
        // channels through different live gains become one, their gains summed.
        for(auto r:current.audioRoutes_)if(ids.count(r.from.node) && (ids.count(r.to.node) || r.to.node==graphOutput)){
            for(auto& slot:r.live){r.gain*=current.liveGain(slot);slot=0;}
            const auto same=std::find_if(audioRoutes_.begin(),audioRoutes_.end(),[&](const auto& x){return x.from==r.from && x.to==r.to && x.channels==r.channels && x.live==r.live;});
            if(same!=audioRoutes_.end())same->gain+=r.gain;else audioRoutes_.push_back(r);}
        for(const auto& [edge,route]:current.noteEdges_)if(ids.count(edge.second) && (ids.count(edge.first) || edge.first==graphInput)){
            noteEdges_.emplace(edge,route);draining_.insert(edge);
            if(const auto z=current.noteZones_.find(edge);z!=current.noteZones_.end())noteZones_[edge]=z->second;}
    }
    void Graph::takeNotes(const Graph& current,NodeId from,NodeId node) {
        for(const auto& [edge,route]:current.noteEdges_){
            if(edge.second!=from)continue;
            const std::pair key{edge.first,node};
            if(edge.first==graphInput){if(auto it=noteEdges_.find(key);it!=noteEdges_.end())it->second=route;continue;} // with none, they are released
            const auto source=nodes_.find(edge.first);
            if(source==nodes_.end() || !source->second.retiring || noteEdges_.contains(key))continue;
            noteEdges_.emplace(key,route);draining_.insert(key);
            if(const auto z=current.noteZones_.find(edge);z!=current.noteZones_.end())noteZones_[key]=z->second;
        }
    }
    size_t Graph::ringOut(const Graph& current,NodeId from,NodeId node) {
        const auto it=nodes_.find(node);if(it==nodes_.end() || !it->second.host->ringsOut())return 0;
        const auto mains=it->second.host->mainAudioPorts(false);if(mains.empty())return 0;
        std::vector<AudioConnection> routes;size_t lost=0;
        for(auto r:current.audioRoutes_){
            if(r.from.node!=from || r.from.port!=mains.front() || r.from.channel>=2)continue;
            for(auto& slot:r.live){r.gain*=current.liveGain(slot);slot=0;}
            const auto to=nodes_.find(r.to.node);
            // No longer the outgoing chain (taken over): left out, the others ring; one that carries nothing is no loss.
            if(r.to.node!=graphOutput && (to==nodes_.end() || !to->second.retiring)){if(r.gain!=0)++lost;continue;}
            r.from={node,ringOutPort,r.from.channel};
            // Ways through different live gains, frozen, are one route: their gains summed (connecting both would keep the last).
            const auto same=std::find_if(routes.begin(),routes.end(),[&](const auto& x){return x.from==r.from && x.to==r.to && x.channels==r.channels;});
            if(same!=routes.end())same->gain+=r.gain;else routes.push_back(r);
        }
        for(auto r:current.audioRoutes_) // the earlier tail, fading out through its own chain
            if(r.from.node==from && r.from.port==ringOutPort && r.from.channel<2 && (r.to.node==graphOutput || nodes_.contains(r.to.node)))
                {r.from={node,ringOutPort,r.from.channel+2};routes.push_back(r);}
        connectAudioBatch(routes);
        return lost;
    }
    void Graph::fadeRoutes(const std::set<NodeId>& nodes,const std::set<NodeId>& ringing,uint8_t slot) {
        for(auto& r:audioRoutes_)if(r.to.node==graphOutput && (r.from.port==ringOutPort ? ringing.count(r.from.node)!=0 : nodes.count(r.from.node)!=0))r.live[0]=slot;
    }
    void Graph::dropRingOut(NodeId node,bool older) { std::erase_if(audioRoutes_,[&](const auto& r){return r.from.node==node && r.from.port==ringOutPort && (!older || r.from.channel>=2);}); }
    size_t Graph::ringOutRoutes(NodeId node) const { return size_t(std::count_if(audioRoutes_.begin(),audioRoutes_.end(),[&](const auto& r){return r.from.node==node && r.from.port==ringOutPort;})); }
    void Graph::publishReplacement(Graph& next,const std::function<void()>& onPublish) {
        if(next.sampleRate_!=sampleRate_ || next.maxFrames_!=maxFrames_ || next.inputChannels_!=inputChannels_ || next.outputChannels_!=outputChannels_)
            throw std::invalid_argument("Replacement device layout mismatch");
        next.replacementSerial_=replacementSerial_+1;
        std::unique_ptr<Plan> plan(next.compile());
        ++plansCompiled_;
        grouped_ = plan->grouped;
        removedHosts_.reserve(removedHosts_.size()+nodes_.size());
        // A host carried over stays in use; listing it here too would keep it alive forever (collect() frees at one owner).
        // So is one taken over under another ID (RFC 0021).
        std::set<const PluginHost*> staying;for(const auto& [id,node]:next.nodes_)staying.insert(node.host.get());
        onPublish(); // caller supplies a nonthrowing metadata/transport commit
        for(const auto& [id,node]:nodes_)if(!staying.count(node.host.get()))removedHosts_.push_back(node.host);
        nodes_.swap(next.nodes_);audioRoutes_.swap(next.audioRoutes_);noteEdges_.swap(next.noteEdges_);noteZones_.swap(next.noteZones_);draining_.swap(next.draining_);paramTargets_.swap(next.paramTargets_);controlTargets_.swap(next.controlTargets_);taps_.swap(next.taps_);harmonies_.swap(next.harmonies_);
        nextId_=next.nextId_;replacementSerial_=next.replacementSerial_;
        notePlanned();
        delete pending_.exchange(plan.release(),std::memory_order_acq_rel);
    }

    void Graph::replace(NodeId id,const std::string& path,const std::string& pluginId,const std::string& preset,const std::function<void()>& rewire)
    {
        const auto old=nodes_.at(id);
        for(const auto& [from,to]:paramTargets_)
            if(from.first==id || to.first==id)throw std::invalid_argument("Remove parameter mappings on this node before replacing its plugin");
        Node next;
        auto loaded=loadPlugin(path,pluginId,sampleRate_,maxFrames_,mode_,lv2Buffers_);loaded->setAssets(assets_);
        if(!preset.empty())loaded->loadPreset(preset);
        next.host=preparedInstrument(std::move(loaded),sampleRate_,maxFrames_,mode_,lv2Buffers_);next.host->setAssets(assets_);
        next.audioInputs=countChannels(next.host->inputPorts());next.audioOutputs=countChannels(next.host->outputPorts());
        const auto routes=audioRoutes_;const auto notes=noteEdges_;const auto zones=noteZones_;
        std::erase_if(audioRoutes_,[&](const auto& e){return e.from.node==id || e.to.node==id;});
        std::erase_if(noteEdges_,[&](const auto& edge){return edge.first.first==id || edge.first.second==id;});
        std::erase_if(noteZones_,[&](const auto& edge){return edge.first.first==id || edge.first.second==id;});
        nodes_.at(id)=std::move(next);
        try
        {
            rewire();
            std::unique_ptr<Plan> plan(compile());
            removedHosts_.push_back(old.host);
            collect();delete pending_.exchange(plan.release(),std::memory_order_acq_rel);
        }
        catch(...){nodes_.at(id)=old;audioRoutes_=routes;noteEdges_=notes;noteZones_=zones;throw;}
    }

    void Graph::restore( NodeId id, const Node& node )
    {
        if(!id || id >= graphInput || !node.host) throw std::invalid_argument("invalid node to restore");
        if(nodes_.count(id)) remove(id);
        nodes_.emplace(id, node);
        nextId_ = std::max(nextId_, id + 1);
    }

    void Graph::remove( NodeId node )
    {
        if(auto it=nodes_.find(node);it!=nodes_.end())removedHosts_.push_back(it->second.host);
        nodes_.erase( node );
        std::erase_if(audioRoutes_,[&](const auto& e){return e.from.node==node || e.to.node==node;});
        for( auto it = paramTargets_.begin(); it != paramTargets_.end(); )
            it = ( it->first.first == node || it->second.first == node ) ? paramTargets_.erase( it ) : std::next( it );
        for( auto it = controlTargets_.begin(); it != controlTargets_.end(); )
        {
            std::erase_if( it->second, [&]( const ControlTarget& t ) { return t.target == node; } );
            it = it->first.first == node || it->second.empty() ? controlTargets_.erase( it ) : std::next( it );
        }
        auto touches = [&]( const std::pair<NodeId, NodeId>& e ) { return e.first == node || e.second == node; };
        std::erase_if(noteEdges_,[&](const auto& edge){return touches(edge.first);});
        std::erase_if(noteZones_,[&](const auto& edge){return touches(edge.first);});
        std::erase_if(draining_,touches);
    }

    std::shared_ptr<PluginHost> Graph::release( NodeId node )
    {
        auto host=nodes_.at(node).host;
        remove(node);removedHosts_.pop_back();
        return host;
    }

    void Graph::checkSource( NodeId from, bool audio ) const
    {
        if( from == graphInput && (!audio || inputChannels_) )
            return;
        const auto it = nodes_.find( from );
        if( it == nodes_.end() )
            throw std::invalid_argument( "no such source node" );
        if( audio ? it->second.audioOutputs == 0 : it->second.host->noteOutputPorts() == 0 )
            throw std::invalid_argument( audio ? "the source has no audio outputs" : "the source has no note outputs" );
    }

    void Graph::checkDestination( NodeId to, bool audio ) const
    {
        if( to == graphOutput && audio && outputChannels_ )
            return;
        const auto it = nodes_.find( to );
        if( it == nodes_.end() )
            throw std::invalid_argument( "no such destination node" );
        if( audio ? it->second.audioInputs == 0 : it->second.host->noteInputPorts() == 0 )
            throw std::invalid_argument( audio ? "the destination has no audio inputs" : "the destination has no note inputs" );
    }

    void Graph::connectAudio( NodeId from, NodeId to )
    {
        checkSource( from, true );
        checkDestination( to, true );

        auto mainChannels=[&](NodeId node,bool input) {
            const bool external=node==graphInput || node==graphOutput;
            const auto& ports=external?(input?audioLayout_.outputs:audioLayout_.inputs):
                (input?host(node).inputPorts():host(node).outputPorts());
            const auto ids=external?std::vector<uint32_t>{ports.front().id}:host(node).mainAudioPorts(input);
            std::vector<AudioEndpoint> result;
            for(auto id:ids)for(const auto& p:ports)if(p.id==id)
                for(uint32_t c=0;c<p.channels;++c)result.push_back({node,id,c});
            return result;
        };
        const auto source=mainChannels(from,false),destination=mainChannels(to,true);
        std::vector<AudioConnection> connections;
        const auto count=source.size()==1 && destination.size()==2?2:std::min(source.size(),destination.size());
        for(size_t i=0;i<count;++i)connections.push_back({source[std::min(i,source.size()-1)],destination[i]});
        connectAudioBatch(connections);
    }

    uint32_t Graph::resolve(const AudioEndpoint& e,bool source,uint32_t count) const
    {
        const std::vector<kick::clap::Port>* ports=nullptr;
        if(source && e.node==graphInput)ports=&audioLayout_.inputs;
        else if(!source && e.node==graphOutput)ports=&audioLayout_.outputs;
        else
        {
            const auto it=nodes_.find(e.node);
            if(it==nodes_.end())throw std::invalid_argument("Unknown audio endpoint node");
            ports=source?&it->second.host->outputPorts():&it->second.host->inputPorts();
            if(source && e.port==ringOutPort && it->second.host->ringsOut())
            {
                if(!count || e.channel>=ringOutChannels || count>ringOutChannels-e.channel)throw std::invalid_argument("Audio channel range exceeds selected port");
                return it->second.audioOutputs+e.channel;
            }
        }
        uint32_t offset=0;
        for(const auto& p:*ports)
        {
            if(p.id==e.port)
            {
                if(!count || e.channel>=p.channels || count>p.channels-e.channel)throw std::invalid_argument("Audio channel range exceeds selected port");
                return offset+e.channel;
            }
            offset+=p.channels;
        }
        throw std::invalid_argument("Unknown audio port ID");
    }
    void Graph::connectAudioBatch(const std::vector<AudioConnection>& connections)
    {
        // Stage the whole operation locally: invalid ranges or limits cannot leave
        // half of a mono-to-stereo connection in the editable graph.
        auto next=audioRoutes_;
        for(const auto& c:connections)
        {
            resolve(c.from,true,c.channels);resolve(c.to,false,c.channels);
            if(!std::isfinite(c.gain) || std::abs(c.gain)>16)
                throw std::invalid_argument("Audio route gain must be finite and within -16..16");
            for(uint32_t i=0;i<c.channels;++i)
            {
                AudioConnection channel=c;channel.channels=1;channel.from.channel+=i;channel.to.channel+=i;
                auto existing=std::find_if(next.begin(),next.end(),[&](const auto& r){return r.from==channel.from && r.to==channel.to && r.live==channel.live;});
                if(existing!=next.end())existing->gain=c.gain;
                else
                {
                    if(next.size()==maxAudioConnections)throw std::invalid_argument("Audio connection limit exceeded");
                    next.push_back(channel);
                }
            }
        }
        audioRoutes_.swap(next);
    }
    void Graph::connectAudio(const AudioConnection& c) { connectAudioBatch({c}); }
    void Graph::disconnectAudio(const AudioConnection& c)
    {
        resolve(c.from,true,c.channels);resolve(c.to,false,c.channels);
        std::erase_if(audioRoutes_,[&](const AudioConnection& r) {
            return r.from.node==c.from.node && r.to.node==c.to.node &&
                r.from.port==c.from.port && r.to.port==c.to.port &&
                r.from.channel>=c.from.channel && r.from.channel-c.from.channel<c.channels &&
                r.to.channel>=c.to.channel && r.to.channel-c.to.channel==r.from.channel-c.from.channel;
        });
    }
    void Graph::disconnectRoute(const AudioConnection& c)
    {
        resolve(c.from,true,c.channels);resolve(c.to,false,c.channels);
        std::erase_if(audioRoutes_,[&](const AudioConnection& r) {
            return r.from.node==c.from.node && r.to.node==c.to.node && r.live==c.live &&
                r.from.port==c.from.port && r.to.port==c.to.port &&
                r.from.channel>=c.from.channel && r.from.channel-c.from.channel<c.channels &&
                r.to.channel>=c.to.channel && r.to.channel-c.to.channel==r.from.channel-c.from.channel;
        });
    }

    void Graph::connectNotes( NodeId from, NodeId to, const std::vector<NoteZone>& zones )
    {
        checkSource( from, false );
        checkDestination( to, false );
        if(zones.size()>NoteZones::capacity || !std::all_of(zones.begin(),zones.end(),[](const NoteZone& z){return z.valid();}))
            throw std::invalid_argument("invalid or too many note zones");
        const auto key=std::make_pair(from,to);
        if(!noteEdges_.contains(key))
        {
            const auto& h=host(to);uint16_t clap=0;
            for(uint32_t p=0;p<16;++p)clap|=uint16_t(h.usesClapNotes(p))<<p;
            noteEdges_.emplace(key,std::make_shared<NoteRoute>(h.noteInputPorts(),clap,uint32_t(sampleRate_*.005))); // RFC 0018's 5 ms steal fade
        }
        if(zones.empty())noteZones_.erase(key);else noteZones_[key]=zones;
    }

    void Graph::disconnectAudio( NodeId from, NodeId to )
    {
        std::erase_if( audioRoutes_, [&]( const AudioConnection& c ) {
            return c.from.node == from && c.to.node == to;
        } );
    }

    void Graph::disconnectNotes( NodeId from, NodeId to )
    {
        noteEdges_.erase({from,to});noteZones_.erase({from,to});draining_.erase({from,to});
    }

    void Graph::setMidiTaps( std::vector<MidiTap> taps )
    {
        for(const auto& t:taps)
        {
            checkSource(t.from,false);
            if(t.zones.size()>NoteZones::capacity || !std::all_of(t.zones.begin(),t.zones.end(),[](const NoteZone& z){return z.valid();}))
                throw std::invalid_argument("invalid or too many note zones");
            if(t.channel>16)throw std::invalid_argument("a MIDI channel is 1 to 16, or 0 for each message's own");
        }
        decltype(taps_) next;
        for(auto& t:taps)
        {
            // A tap whose chosen destination changed is a different tap: the old route is gone, and
            // beginBlock sends its held notes' offs with the old ports and channel.
            const auto kept=std::find_if(taps_.begin(),taps_.end(),[&](const auto& o){return o.first.key==t.key && o.first.from==t.from && o.first.destination==t.destination;});
            auto route=kept!=taps_.end() && std::none_of(next.begin(),next.end(),[&](const auto& n){return n.second==kept->second;})?kept->second:std::make_shared<NoteRoute>(16u);
            next.emplace_back(std::move(t),std::move(route));
        }
        taps_.swap(next);
    }

    void Graph::setHarmonies( std::vector<Harmony> harmonies )
    {
        for(const auto& h:harmonies)
            if(h.zones.size()>NoteZones::capacity || !std::all_of(h.zones.begin(),h.zones.end(),[](const NoteZone& z){return z.valid();}) ||
               h.root>11 || h.scale>=4096 || !(h.scale&1))
                throw std::invalid_argument("invalid harmony: zones, root or scale");
        decltype(harmonies_) next;
        for(auto& h:harmonies)
        {
            const auto kept=std::find_if(harmonies_.begin(),harmonies_.end(),[&](const auto& o){return o.first.key==h.key;});
            // Held keys are routed through the old input and zones: if either changed, later
            // releases may never reach them, so start fresh (a lost release is a lost chord).
            const bool same=kept!=harmonies_.end() && kept->first.input==h.input && kept->first.zones==h.zones;
            next.emplace_back(std::move(h),same?kept->second:std::make_shared<HeldKeys>());
        }
        harmonies_.swap(next);
    }

    std::vector<Graph::MidiTap> Graph::midiTaps() const
    {
        std::vector<MidiTap> out;for(const auto& [t,route]:taps_)out.push_back(t);return out;
    }

    void Graph::setDraining( NodeId from, NodeId to, bool draining )
    {
        if(draining)draining_.insert({from,to});else draining_.erase({from,to});
    }

    void Graph::setRetiring( NodeId node, bool retiring )
    {
        auto& n=nodes_.at(node);
        if(retiring==n.retiring)return;
        n.retiring=retiring;
        // Counting starts afresh; the audio thread takes the flag with the next plan.
        n.activity=std::make_shared<Activity>();
        n.activity->sounding.store(UINT32_MAX,std::memory_order_relaxed); // not idle until it has been counted
    }

    void Graph::seedNoteCost( NodeId node, double rest, double perNote )
    {
        auto& a=*nodes_.at(node).activity;
        a.seedRest.store(float(rest),std::memory_order_relaxed);a.seedNote.store(float(perNote),std::memory_order_relaxed);
    }
    const Graph::Activity* Graph::instanceActivity( NodeId node ) const
    {
        const auto it=nodes_.find(node);
        if(it==nodes_.end())return nullptr;
        if(const auto* sound=dynamic_cast<const SoundContext*>(it->second.host.get()))
            if(const auto e=engineCosts_.find(sound->engine()->host.get());e!=engineCosts_.end())return e->second.get();
        return it->second.activity.get();
    }
    Graph::NoteCost Graph::noteCost( NodeId node, bool instance ) const
    {
        const auto it=nodes_.find(node);
        const auto* found=it==nodes_.end()?nullptr:instance?instanceActivity(node):it->second.activity.get();
        if(!found)return {};
        const auto& a=*found;
        return {a.rest.load(std::memory_order_relaxed),a.perNote.load(std::memory_order_relaxed),a.stolen.load(std::memory_order_relaxed),a.hardChokes.load(std::memory_order_relaxed),
                a.stolenForDevice.load(std::memory_order_relaxed),a.laneLimit.load(std::memory_order_relaxed),a.laneTook.load(std::memory_order_relaxed),
                a.stolenForLoss.load(std::memory_order_relaxed),a.lostBlocks.load(std::memory_order_relaxed),a.stolenForDeadline.load(std::memory_order_relaxed),
                a.unfitBlocks.load(std::memory_order_relaxed),a.importanceMismatch.load(std::memory_order_relaxed),a.heldOff.load(std::memory_order_relaxed)};
    }
    Graph::OutputGuard Graph::outputGuard( NodeId node ) const
    {
        const auto it=nodes_.find(node);
        if(it==nodes_.end() || !it->second.activity)return {};
        return {it->second.activity->badOutput.load(std::memory_order_relaxed),it->second.activity->badRun.load(std::memory_order_relaxed)};
    }
    uint32_t Graph::sounding( NodeId node ) const
    {
        const auto it=nodes_.find(node);
        return it==nodes_.end() || !it->second.activity ? UINT32_MAX : it->second.activity->sounding.load(std::memory_order_relaxed);
    }
    bool Graph::silent( NodeId node, uint64_t quietFrames ) const
    {
        const auto& a=*nodes_.at(node).activity;
        return !a.sounding.load(std::memory_order_relaxed) && (a.asleep.load(std::memory_order_relaxed) || a.quiet.load(std::memory_order_relaxed)>=quietFrames);
    }
    void Graph::reload( NodeId id, const std::string& path, const std::string& pluginId, uint64_t memoryRoom )
    {
        auto& node=nodes_.at(id);
        auto loaded=loadPlugin(path,pluginId,sampleRate_,maxFrames_,mode_,lv2Buffers_,memoryRoom);
        node.host=preparedInstrument(std::move(loaded),sampleRate_,maxFrames_,mode_,lv2Buffers_);node.host->setAssets(assets_);
        node.audioInputs=countChannels(node.host->inputPorts());node.audioOutputs=countChannels(node.host->outputPorts());
    }
    bool Graph::idle( NodeId node, uint64_t quietFrames, uint64_t limitFrames, uint64_t heldLimitFrames ) const
    {
        const auto& a=*nodes_.at(node).activity;
        const auto age=a.age.load(std::memory_order_relaxed);
        if(age>=heldLimitFrames)return true;
        if(a.sounding.load(std::memory_order_relaxed))return false;
        return a.asleep.load(std::memory_order_relaxed) || a.quiet.load(std::memory_order_relaxed)>=quietFrames || age>=limitFrames;
    }

    void Graph::mapParamTarget( NodeId source, uint32_t lane, NodeId target, clap_id param )
    {
        if( !nodes_.count( source ) || !nodes_.count( target ) || source == target )
            throw std::invalid_argument( "a parameter mapping needs two different nodes" );
        const auto& params = nodes_.at( target ).host->parameters();
        const auto  found  = std::find_if( params.begin(), params.end(), [&]( const kick::clap::Parameter& p ) { return p.id == param; } );
        if( found == params.end() || ( found->flags & CLAP_PARAM_IS_READONLY ) )
            throw std::invalid_argument( "the target has no such parameter, or it is read-only" );
        paramTargets_[{ source, lane }] = { target, param };
    }

    void Graph::unmapParamTarget( NodeId source, uint32_t lane )
    {
        paramTargets_.erase( { source, lane } );
    }

    double Graph::ControlCurve::shape( double value ) const noexcept
    {
        const double x = std::clamp( value, 0.0, 1.0 );
        switch( kind )
        {
        case Kind::Linear: break;
        case Kind::Exponential: return ( std::exp( 4 * x ) - 1 ) / ( std::exp( 4.0 ) - 1 );
        case Kind::Logarithmic: return std::log1p( ( std::exp( 4.0 ) - 1 ) * x ) / 4;
        case Kind::SCurve: return x * x * ( 3 - 2 * x );
        // exp, not pow( 10, y ): clang turns that into exp10 from -O1 up, which rounds differently from pow; the Rust run does the same sum.
        case Kind::Db: return x > 0 ? std::exp( std::fma( high - low, x, low ) / 20 * std::numbers::ln10 ) : 0;
        case Kind::Power: return std::pow( x, exponent );
        }
        return x;
    }

    void Graph::setControlTargets( ControlTargets targets )
    {
        // Only what can be compiled: a source and targets in the graph, and no lane driving its own node.
        for( auto it = targets.begin(); it != targets.end(); )
        {
            std::erase_if( it->second, [&]( const ControlTarget& t ) { return !nodes_.count( t.target ) || t.target == it->first.first || t.path.empty(); } );
            it = !nodes_.count( it->first.first ) || it->second.empty() ? targets.erase( it ) : std::next( it );
        }
        controlTargets_ = std::move( targets );
    }

    const PluginHost& Graph::host( NodeId node ) const
    {
        return *nodes_.at( node ).host;
    }

    PluginHost& Graph::host( NodeId node )
    {
        return *nodes_.at( node ).host;
    }

    Graph::Description Graph::describe() const
    {
        Description result;
        for( const auto& [id,node] : nodes_ )
            result.nodes.push_back({id,node.host->descriptor(),node.host->inputPorts(),node.host->outputPorts(),
                                    node.host->noteInputPorts(),node.host->noteOutputPorts()});
        for(const auto& r:audioRoutes_)if(r.from.port!=ringOutPort)result.audioRoutes.push_back(r); // internal: see ringOutPort
        result.audioLayout=audioLayout_;
        for(const auto& [edge,state]:noteEdges_)result.notes.push_back(edge);
        result.noteZones=noteZones_;result.draining=draining_;
        result.mappings = mappings();
        return result;
    }

    std::vector<Graph::Mapping> Graph::mappings() const
    {
        std::vector<Mapping> result;
        for( const auto& [from,to] : paramTargets_ )
            result.push_back({from.first,from.second,to.first,to.second});
        return result;
    }

    Graph::Plan* Graph::compile(bool ramped) const
    {
        // RFC 0021 slice 3: the nodes playing sounds of one shared engine are one step, its first
        // node's (the lead); every edge to or from any of them is the lead's.
        std::map<const PluginHost*, NodeId> leads;
        std::map<NodeId, NodeId> lead;
        for( const auto& [id, node] : nodes_ )
            if( const auto* sound = dynamic_cast<const SoundContext*>( node.host.get() ) )
                lead[id] = leads.emplace( sound->engine()->host.get(), id ).first->second;
        auto leadOf = [&]( NodeId id ) { const auto it = lead.find( id ); return it == lead.end() ? id : it->second; };
        // Dependency order (Kahn's algorithm), lowest node ID first among ready nodes so
        // the order is stable.
        std::map<NodeId, int> indegree;
        for( const auto& [id, node] : nodes_ )
            if( leadOf( id ) == id )
                indegree[id] = 0;
        auto internal = [&]( const std::pair<NodeId, NodeId>& e ) { return nodes_.count( e.first ) && nodes_.count( e.second ); };
        auto led = [&]( NodeId from, NodeId to ) { return std::pair{ nodes_.count( from ) ? leadOf( from ) : from, nodes_.count( to ) ? leadOf( to ) : to }; };
        std::vector<std::pair<NodeId, NodeId>> noteDependencies;
        for(const auto& [edge,state]:noteEdges_)noteDependencies.push_back(led(edge.first,edge.second));
        std::vector<std::pair<NodeId, NodeId>> audioDependencies;
        for(const auto& e:audioRoutes_)audioDependencies.push_back(led(e.from.node,e.to.node));
        std::vector<std::pair<NodeId, NodeId>> mappingEdges;
        for( const auto& [from, to] : paramTargets_ )
            mappingEdges.push_back( led( from.first, to.first ) );
        for( const auto& [from, targets] : controlTargets_ )
            for( const auto& t : targets )
                mappingEdges.push_back( led( from.first, t.target ) );
        for( const auto* edges : { &std::as_const(audioDependencies), &std::as_const(noteDependencies), &std::as_const( mappingEdges ) } )
            for( const auto& e : *edges )
                if( internal( e ) )
                    ++indegree[e.second];

        std::vector<NodeId> order;
        while( order.size() < indegree.size() )
        {
            auto ready = std::find_if( indegree.begin(), indegree.end(), []( const auto& d ) { return d.second == 0; } );
            if( ready == indegree.end() )
                throw std::runtime_error( "the graph's connections form a cycle" );
            const NodeId id = ready->first;
            ready->second   = -1; // done
            order.push_back( id );
            for( const auto* edges : { &std::as_const(audioDependencies), &std::as_const(noteDependencies), &std::as_const( mappingEdges ) } )
                for( const auto& e : *edges )
                    if( e.first == id && internal( e ) )
                        --indegree[e.second];
        }
        // RFC 0034: waves. A node's is one past its sources' and those of earlier nodes sharing its
        // engine (effect contexts never render at the same time); the plan runs wave by wave, in
        // the same order rendering serially or not, so both route notes identically.
        // RFC 0045's plugin groups: a node whose audio all comes from one group of its own worker process, whose notes
        // and lanes come from earlier waves, joins that group's wave, so the group's blocks cross to the worker once
        // and render there in order, each node's input mixed in the worker (WorkerHost::startGrouped). Not where the
        // core's state would differ there: a played change's ramps (ramped), ring-out routes, retiring nodes, blocks
        // past the worker's buffer. A groupable node nothing feeds (a bus whose send is at 0, left out of the plan) in a
        // process where a group renders rides that group's wave (a floor on its own): its block crosses to the worker in
        // the group's round trip, not one of its own (two more wake-ups each block).
        std::map<NodeId, uint32_t> wave, floors;
        std::map<NodeId, NodeId> groupOf; // a groupable node's group, by its first node
        auto hostOf = [&]( NodeId id ) -> const PluginHost* {
            const auto* host = nodes_.at( id ).host.get();
            if( const auto* sound = dynamic_cast<const SoundContext*>( host ) ) host = sound->engine()->host.get();
            return host; };
        const bool grouping = !ramped && maxFrames_ <= 8192 && worker::workerGroups();
        auto groupable = [&]( NodeId id ) {
            const auto* host = hostOf( id );
            return host && host->groupable() && host->lane() && !nodes_.at( id ).retiring
                && std::none_of( audioRoutes_.begin(), audioRoutes_.end(), [&]( const auto& r ) { return ( leadOf( r.from.node ) == id && r.from.port == ringOutPort ) || ( r.to.node == id && r.to.port == ringOutPort ); } ); };
        auto waves = [&] {
            wave.clear(); groupOf.clear();
            for( size_t i = 0; i < order.size(); ++i )
            {
                uint32_t w = 0;
                if( const auto f = floors.find( order[i] ); f != floors.end() ) w = f->second;
                for( const auto* edges : { &std::as_const(audioDependencies), &std::as_const(noteDependencies), &std::as_const( mappingEdges ) } )
                    for( const auto& e : *edges )
                        if( e.second == order[i] && internal( e ) )
                            w = std::max( w, wave.at( e.first ) + 1 );
                uint32_t apart = 0; // one past the waves of earlier nodes sharing its engine
                for( size_t j = 0; j < i; ++j )
                    if( nodes_.at( order[j] ).effectGroup == nodes_.at( order[i] ).effectGroup || nodes_.at( order[j] ).host == nodes_.at( order[i] ).host )
                        apart = std::max( apart, wave.at( order[j] ) + 1 );
                w = std::max( w, apart );
                if( grouping && groupable( order[i] ) )
                {
                    groupOf[order[i]] = order[i];
                    std::optional<NodeId> group;
                    bool joins = true;
                    for( const auto& e : audioDependencies )
                        if( e.second == order[i] )
                        {
                            const auto g = groupOf.find( e.first );
                            if( !internal( e ) || g == groupOf.end() || ( group && *group != g->second ) || hostOf( e.first )->lane() != hostOf( order[i] )->lane() ) { joins = false; break; }
                            group = g->second;
                        }
                    uint32_t routes = 0;
                    for( const auto& r : audioRoutes_ ) if( r.to.node == order[i] ) routes += r.channels;
                    if( joins && group && routes <= PluginHost::maxGroupRoutes )
                    {
                        const uint32_t at = wave.at( *group );
                        for( const auto* edges : { &std::as_const( noteDependencies ), &std::as_const( mappingEdges ) } )
                            for( const auto& e : *edges )
                                if( e.second == order[i] && internal( e ) && wave.at( e.first ) >= at ) joins = false;
                        if( joins && apart <= at ) { w = at; groupOf[order[i]] = *group; }
                    }
                }
                wave[order[i]] = w;
            }
        };
        waves();
        std::vector<std::pair<NodeId, NodeId>> riders; // a node fed by nothing, and a node of its process's group whose wave it rides
        auto fed = [&]( NodeId id ) {
            auto into = [&]( const std::vector<std::pair<NodeId, NodeId>>& edges ) { return std::any_of( edges.begin(), edges.end(), [&]( const auto& e ) { return e.second == id; } ); };
            return into( audioDependencies ) || into( noteDependencies ) || into( mappingEdges ); };
        if( grouping )
            for( const NodeId id : order )
            {
                if( !groupable( id ) || fed( id ) ) continue;
                // Where its process renders nothing fed in its own wave: the earliest later wave where it does.
                std::optional<NodeId> anchor;
                bool alone = true;
                for( const NodeId x : order )
                {
                    if( x == id || !groupable( x ) || !fed( x ) || hostOf( x )->lane() != hostOf( id )->lane() ) continue;
                    if( wave.at( x ) == wave.at( id ) ) alone = false;
                    if( wave.at( x ) > wave.at( id ) && ( !anchor || wave.at( x ) < wave.at( *anchor ) ) ) anchor = x;
                }
                if( alone && anchor ) { floors[id] = wave.at( *anchor ); riders.push_back( { id, *anchor } ); }
            }
        if( !floors.empty() ) waves();
        std::stable_sort( order.begin(), order.end(), [&]( NodeId a, NodeId b ) { return wave.at( a ) < wave.at( b ); } );

        auto plan = std::make_unique<Plan>();
        plan->replacementSerial=replacementSerial_;
        std::map<NodeId, int> stepOf;
        std::map<NodeId, std::pair<int16_t, uint32_t>> contextOf; // a context's channel and its bus's first channel
        for(auto it=engineCosts_.begin();it!=engineCosts_.end();)it=leads.count(it->first)?std::next(it):engineCosts_.erase(it);
        for( const NodeId id : order )
        {
            const auto& node = nodes_.at( id );
            Plan::Step  step;
            step.host         = node.host;
            step.id           = id;
            step.peaks        = node.peaks;
            step.retiring     = node.retiring;
            step.activity     = node.activity;
            step.audioInputs  = node.audioInputs;
            step.audioOutputs = node.audioOutputs;
            if( const auto* sound = dynamic_cast<const SoundContext*>( node.host.get() ) )
            {
                const auto& engine = sound->engine()->host;
                step.host = engine; step.peaks = nullptr;
                auto& cost = engineCosts_[engine.get()];
                if( !cost ) cost = std::make_shared<Activity>();
                step.activity = cost;
                step.audioInputs = countChannels( engine->inputPorts() ); step.audioOutputs = countChannels( engine->outputPorts() );
                for( const auto& [other, l] : lead )
                {
                    if( l != id ) continue;
                    const auto& n = nodes_.at( other );
                    auto* context = const_cast<SoundContext*>( dynamic_cast<const SoundContext*>( n.host.get() ) );
                    uint32_t offset = 0;
                    for( uint32_t i = 0; i < context->bus(); ++i ) offset += engine->outputPorts()[i].channels;
                    step.contexts.push_back( { other, n.host, context, int16_t( context->channel() ), offset, engine->outputPorts()[context->bus()].channels, n.peaks, n.activity } );
                    contextOf[other] = { int16_t( context->channel() ), offset };
                    stepOf[other] = int( plan->steps.size() );
                }
            }
            // The ring-out buffer's four channels, after the node's own, while it is routed.
            step.ringOut      = std::any_of(audioRoutes_.begin(),audioRoutes_.end(),[&](const auto& r){return r.from.node==id && r.from.port==ringOutPort;});
            if(step.ringOut) step.audioOutputs += ringOutChannels;
            stepOf[id]        = (int)plan->steps.size();
            if( !plan->steps.empty() && wave.at( id ) != wave.at( plan->steps.back().id ) ) plan->waves.push_back( uint32_t( plan->steps.size() ) );
            plan->steps.push_back( std::move( step ) );
        }
        if( !plan->steps.empty() ) plan->waves.push_back( uint32_t( plan->steps.size() ) );
        std::vector<oscairn_audio_route> outputRoutes; // the graph's outputs' connections, and each step's
        std::vector<std::vector<oscairn_audio_route>> audioSources(plan->steps.size());
        auto route=[&](NodeId from,uint32_t source,NodeId to,uint32_t dest,float gain,const std::array<uint8_t,4>& live)
        {
            if(const auto c=contextOf.find(from);c!=contextOf.end())source+=c->second.second; // its bus on the instance
            oscairn_audio_route r{from==graphInput?-1:stepOf.at(from),source,dest,gain,{live[0],live[1],live[2],live[3]}};
            (to==graphOutput?outputRoutes:audioSources[size_t(stepOf.at(to))]).push_back(r);
        };
        for(const auto& c:audioRoutes_)
        {
            const auto from=resolve(c.from,true,c.channels),to=resolve(c.to,false,c.channels);
            for(uint32_t i=0;i<c.channels;++i)route(c.from.node,from+i,c.to.node,to+i,static_cast<float>(c.gain),c.live);
        }
        for(size_t i=0;i<plan->steps.size();++i)
            if(const auto g=groupOf.find(plan->steps[i].id);g!=groupOf.end() && g->second!=g->first)
            {
                std::vector<bool> written(plan->steps[i].audioInputs);
                for(const auto& r:audioSources[i])
                {
                    const bool first=r.to<written.size() && !written[r.to];
                    if(r.to<written.size())written[r.to]=true;
                    plan->steps[i].fused.push_back({r.source,r.from,r.to,r.gain,{r.live[0],r.live[1],r.live[2],r.live[3]},first});
                }
                plan->grouped.push_back(plan->steps[i].id);
            }
        for( const auto& [id, anchor] : riders ) if( wave.at( id ) == wave.at( anchor ) ) plan->grouped.push_back( id );
        for( const auto& [edge,state] : noteEdges_ )
        {
            const auto [from,to]=edge;
            if( !nodes_.count( to ) || ( from != graphInput && !nodes_.count( from ) ) )
                continue;
            NoteZones zones;
            if(const auto z=noteZones_.find(edge);z!=noteZones_.end()){zones.count=z->second.size();std::copy(z->second.begin(),z->second.end(),zones.zones.begin());}
            const auto c=contextOf.find(to);
            plan->steps[(size_t)stepOf[to]].noteSources.push_back({from == graphInput ? -1 : stepOf[from],state,zones,draining_.count(edge)!=0,{},c==contextOf.end()?int16_t(-1):c->second.first});
        }
        // A part's connections share its note limit: each knows the others (none draining).
        for(auto& step:plan->steps)for(auto& source:step.noteSources)if(source.zones.count && source.zones.zones[0].part && !source.draining)
            for(const auto& other:plan->steps)for(const auto& o:other.noteSources)
                if(o.zones.count && o.zones.zones[0].part==source.zones.zones[0].part && !o.draining)source.peers.push_back(o.route->ledger);
        // The budget's lanes (RFC 0045): the steps rendering on one thread share one.
        std::map<const void*,uint32_t> lanes;
        for(auto& step:plan->steps)if(step.host){step.inProcess=!step.host->lane();step.lane=lanes.emplace(step.host->lane(),uint32_t(lanes.size())).first->second;}
        // The engine core's view of the plan: each step's note connections, contexts and lane. A period only for a lane every
        // plugin of which is a block late: one on time beside them (a plugin host's) must be done within the allowance.
        std::vector<std::vector<oscairn_source>> sources(plan->steps.size());
        std::vector<std::vector<oscairn_context>> contexts(plan->steps.size());
        std::vector<std::vector<oscairn_link>> next(plan->steps.size()); // where each step's audio goes, its largest gain there
        for(size_t i=0;i<plan->steps.size();++i)for(const auto& r:audioSources[i])if(r.source>=0)
        {
            auto& links=next[size_t(r.source)];
            auto it=std::find_if(links.begin(),links.end(),[&](const oscairn_link& l){return l.step==i;});
            if(it==links.end())links.push_back({uint32_t(i),std::abs(r.gain)});else it->gain=std::max(it->gain,std::abs(r.gain));
        }

        for(const auto& [tap,route]:taps_)
        {
            if(tap.from!=graphInput && !nodes_.count(tap.from))continue; // removed since
            Plan::Tap t{tap.from==graphInput?-1:stepOf.at(tap.from),route,{},tap.ports,tap.channel};
            t.zones.count=tap.zones.size();std::copy(tap.zones.begin(),tap.zones.end(),t.zones.zones.begin());
            plan->taps.push_back(std::move(t));
        }

        for(const auto& [harmony,held]:harmonies_)
        {
            Plan::Harmony h{held,{},harmony.input,harmony.root,harmony.scale};
            h.zones.count=harmony.zones.size();std::copy(harmony.zones.begin(),harmony.zones.end(),h.zones.zones.begin());
            for(const auto node:harmony.nodes)if(const auto s=stepOf.find(node);s!=stepOf.end())plan->steps[(size_t)s->second].harmony=int(plan->harmonies.size());
            plan->harmonies.push_back(std::move(h));
        }

        // The note connection a lane's per-note events address: the target's from the lane's node, if any.
        auto noteSourceOf=[&](const Plan::Step& target,std::pair<NodeId,NodeId> edge)->int16_t {
            const auto notes=noteEdges_.find(edge);
            for(size_t i=0;notes!=noteEdges_.end() && i<target.noteSources.size();++i)if(target.noteSources[i].route==notes->second)return int16_t(i);
            return -1;};
        for( const auto& [from, to] : paramTargets_ )
        {
            const auto& params = nodes_.at( to.first ).host->parameters();
            const auto  p      = std::find_if( params.begin(), params.end(), [&]( const kick::clap::Parameter& q ) { return q.id == to.second; } );
            if( p == params.end() )
                continue; // the plugin no longer has it
            auto& source = plan->steps[(size_t)stepOf[from.first]];
            auto& target = plan->steps[(size_t)stepOf[to.first]];
            const auto c = contextOf.find( to.first );
            source.routes.push_back( { from.second, stepOf[to.first], to.second, p->minimum, p->maximum, ( p->flags & CLAP_PARAM_IS_STEPPED ) != 0, -1, c == contextOf.end() ? int16_t( -1 ) : c->second.first, -1 } );
            source.routes.back().notes=noteSourceOf(target,{from.first,to.first});
            target.lanesIn = true;
        }
        for( const auto& [from, targets] : controlTargets_ )
            for( const auto& t : targets )
            {
                const auto& params = nodes_.at( t.target ).host->parameters();
                const auto  p      = std::find_if( params.begin(), params.end(), [&]( const kick::clap::Parameter& q ) { return q.id == t.param; } );
                if( p == params.end() || ( p->flags & CLAP_PARAM_IS_READONLY ) )
                    continue;
                auto& source = plan->steps[(size_t)stepOf[from.first]];
                auto& target = plan->steps[(size_t)stepOf[t.target]];
                const auto c = contextOf.find( t.target );
                source.routes.push_back( { from.second, stepOf[t.target], t.param, p->minimum, p->maximum, ( p->flags & CLAP_PARAM_IS_STEPPED ) != 0, -1, c == contextOf.end() ? int16_t( -1 ) : c->second.first, int32_t( plan->controls.size() ) } );
                plan->controls.push_back( t );
                source.routes.back().notes=noteSourceOf(target,{from.first,t.target});
                target.lanesIn = true;
            }
        // OSCAIRN_WORKER_REST=disabled (realtime.h; not the default since 2026-11-27, which is all): a step can be played, so it is never rested, when a note can reach it
        // (a note connection not draining, from the input or a step that can be played), audio comes from the input or such
        // a step, lanes reach it, or nothing comes into it at all (it sounds on its own). Steps come after their sources.
        for(size_t i=0;i<plan->steps.size();++i)
        {
            auto& s=plan->steps[i];
            const auto can=[&](int from){return from<0 || size_t(from)>=i || plan->steps[size_t(from)].playable;};
            s.playable=s.lanesIn || (s.noteSources.empty() && audioSources[i].empty())
                || std::any_of(s.noteSources.begin(),s.noteSources.end(),[&](const Plan::Step::NoteSource& n){return !n.draining && can(n.step);})
                || std::any_of(audioSources[i].begin(),audioSources[i].end(),[&](const oscairn_audio_route& r){return can(r.source);});
        }

        {
            using Peaks=oscairn_peaks*;
            std::vector<std::vector<oscairn_bus>> buses(plan->steps.size());
            std::vector<oscairn_audio_step> steps;
            for(size_t i=0;i<plan->steps.size();++i)
            {
                auto& step=plan->steps[i];
                for(const auto& c:step.contexts)buses[i].push_back({c.offset,c.channels,reinterpret_cast<Peaks>(c.peaks.get()),coreActivity(c.activity.get())});
                steps.push_back({step.host.get(),step.audioInputs,step.audioOutputs,step.audioOutputs-(step.ringOut?Graph::ringOutChannels:0u),reinterpret_cast<Peaks>(step.peaks.get()),
                                 coreActivity(step.activity.get()),audioSources[i].data(),uint32_t(audioSources[i].size()),buses[i].data(),uint32_t(buses[i].size())});
            }
            plan->audio.reset(rust_core_audio_new(steps.data(),uint32_t(steps.size()),outputRoutes.data(),uint32_t(outputRoutes.size()),inputChannels_,outputChannels_,maxFrames_));
        }
        for(size_t i=0;i<plan->steps.size();++i)
        {
            auto& step=plan->steps[i];
            step.in=rust_core_audio_buffers(plan->audio.get(),int32_t(i),false);step.out=rust_core_audio_buffers(plan->audio.get(),int32_t(i),true);
        }
        // The notes, then the block around them (the steps' parts only it reads besides).
        std::vector<oscairn_step> steps;
        for(size_t i=0;i<plan->steps.size();++i)
        {
            auto& step=plan->steps[i];
            for(auto& s:step.noteSources)sources[i].push_back({s.route->ledger,reinterpret_cast<const oscairn_zones*>(&s.zones),s.peers.data(),uint32_t(s.peers.size()),s.channel,s.draining,s.step});
            for(auto& c:step.contexts)contexts[i].push_back({c.channel,coreActivity(c.activity.get()),c.id});
            const bool late=step.host && std::all_of(plan->steps.begin(),plan->steps.end(),[&](const Plan::Step& o){return !o.host || o.lane!=step.lane || o.host->oneBlockLate();});
            steps.push_back({coreActivity(step.activity.get()),sources[i].data(),uint32_t(sources[i].size()),contexts[i].data(),uint32_t(contexts[i].size()),step.lane,step.inProcess,late,
                             next[i].data(),uint32_t(next[i].size()),step.host.get(),step.id,step.harmony,step.routes.data(),uint32_t(step.routes.size()),step.lanesIn,
                             step.host && (worker::workerRest()==worker::WorkerRest::all || (worker::workerRest()==worker::WorkerRest::disabled && !step.playable))});
        }
        const auto budgeted=std::make_unique<bool[]>(steps.size());
        plan->notes.reset(rust_core_notes_new(steps.data(),uint32_t(steps.size()),uint32_t(lanes.size()),budgeted.get()));
        std::vector<oscairn_harmony_source> harmonies;
        for(auto& h:plan->harmonies)harmonies.push_back({h.held->route.ledger,reinterpret_cast<const oscairn_zones*>(&h.zones),h.input,h.root,h.scale,&h.held->last});
        std::vector<oscairn_tap> taps;
        for(auto& t:plan->taps)taps.push_back({t.step,t.route->ledger,reinterpret_cast<const oscairn_zones*>(&t.zones),t.ports,t.channel});
        std::vector<oscairn_control> controls;
        static_assert(sizeof(ControlCurve)==sizeof(oscairn_curve) && offsetof(ControlCurve,start)==offsetof(oscairn_curve,start) && offsetof(ControlCurve,exponent)==offsetof(oscairn_curve,exponent));
        for(auto& c:plan->controls)controls.push_back({reinterpret_cast<const oscairn_curve*>(c.path.data()),uint32_t(c.path.size()),c.origin,c.modulate});
        plan->run.reset(rust_core_run_new(plan->notes.get(),plan->audio.get(),steps.data(),budgeted.get(),uint32_t(steps.size()),plan->waves.data(),uint32_t(plan->waves.size()),
                                              harmonies.data(),uint32_t(harmonies.size()),taps.data(),uint32_t(taps.size()),controls.data(),uint32_t(controls.size())));
        return plan.release();
    }

    void Graph::commit(bool rampGains)
    {
        Plan* plan = compile(rampGains); // throws on a cycle, before anything changes
        ++plansCompiled_;
        grouped_ = plan->grouped;
        plan->rampGains = rampGains;
        collect();
        notePlanned();
        delete pending_.exchange( plan, std::memory_order_acq_rel ); // a plan the audio thread never took
    }

    bool Graph::grouped( NodeId node ) const { return std::find( grouped_.begin(), grouped_.end(), node ) != grouped_.end(); }

    bool Graph::planned( NodeId node ) const
    {
        const auto p = planned_.find( node );
        const auto n = nodes_.find( node );
        return p != planned_.end() && n != nodes_.end() && p->second == n->second.host.get();
    }

    void Graph::collect()
    {
        while( Plan* p = retired_.pop() )
            delete p;
        // A host is let go once only this list holds it. It may be listed twice (removed, put back by an undo,
        // removed again): each entry is a count of the list's own.
        std::map<const PluginHost*,long> listed;
        for(const auto& host:removedHosts_)++listed[host.get()];
        std::set<const PluginHost*> alone; // decided before erasing: the counts change as entries go
        for(const auto& host:removedHosts_)if(host.use_count()==listed.at(host.get()))alone.insert(host.get());
        std::erase_if(removedHosts_,[&](const auto& host){return alone.count(host.get())!=0;});
    }

    void Graph::service()
    {
        collect();
        for(const auto& [id,node]:nodes_)node.host->service();
        for(const auto& host:removedHosts_)host->service();
    }

    void Graph::setSampleRate( double rate )
    {
        if( !( rate > 0 ) || !std::isfinite( rate ) ) throw std::invalid_argument( "A sample rate must be positive" );
        if( rate == sampleRate_ ) return;
        const AudioPause::Hold hold( pause_ );
        std::set<PluginHost*> done;
        std::string failed;
        auto follow = [&]( PluginHost* host ) {
            if( auto* context = dynamic_cast<SoundContext*>( host ) ) host = context->engine()->host.get(); // the engine it shares, once
            if( !done.insert( host ).second ) return;
            try { host->reconfigure( rate, maxFrames_ ); }
            catch( const std::exception& e ) { failed += ( failed.empty() ? "" : "; " ) + host->descriptor().name + ": " + e.what(); }
        };
        for( auto& [id, node] : nodes_ ) follow( node.host.get() );
        for( auto& host : removedHosts_ ) follow( host.get() ); // the plan playing may hold them until the next commit
        sampleRate_ = rate;
        if( !failed.empty() ) throw std::runtime_error( "Not at the new sample rate: " + failed );
    }

    bool Graph::beginBlock() noexcept {
        // Adopt the newest plan, retiring the current one, if there is room to retire it.
        if( !current_ || !retired_.full() )
            if( Plan* plan = pending_.exchange( nullptr, std::memory_order_acq_rel ) )
            {
                const bool replacement=!current_?plan->replacementSerial!=0:current_->replacementSerial!=plan->replacementSerial;
                if( current_ ) {
                    // Retired connections' releases, stranded plugins, taps gone, a played change's gains (the engine core).
                    rust_core_run_adopt(plan->run.get(),current_->run.get(),plan->rampGains,std::max<uint32_t>(1,uint32_t(sampleRate_*.005)),
                                            reinterpret_cast<oscairn_midi*>(midiOut_.data()),&midiOutCount_,reinterpret_cast<uint64_t*>(&eventDrops_));
                    retired_.push( current_ );
                }
                current_ = plan;
                return replacement;
            }
        return {};
    }

    bool Graph::queueParameter( NodeId node, uint32_t param, double value ) noexcept { return queue( { node, param, value, false } ); }
    bool Graph::queueModulation( NodeId node, uint32_t param, double amount ) noexcept { return queue( { node, param, amount, true } ); }

    bool Graph::queue( const QueuedParameter& q ) noexcept
    {
        std::lock_guard lock( queueLock_ );
        const auto head = queueHead_.load( std::memory_order_relaxed );
        if( head - queueTail_.load( std::memory_order_acquire ) == queued_.size() ) return false;
        queued_[head % queued_.size()] = q;
        queueHead_.store( head + 1, std::memory_order_release );
        return true;
    }

    Graph::Peaks Graph::takePeaks( NodeId node ) noexcept
    {
        Peaks out{};
        if( const auto it = nodes_.find( node ); it != nodes_.end() )
            for( uint32_t c = 0; c < meterChannels; ++c ) out[c] = ( *it->second.peaks )[c].exchange( 0.f, std::memory_order_relaxed );
        return out;
    }

    bool Graph::process( float* const* outputs, uint32_t frames, const clap_event_transport_t* transport,
                         const kick::clap::Event* events, uint32_t eventCount ) noexcept
    {
        return process(nullptr,0,outputs,channels,frames,transport,events,eventCount);
    }

    bool Graph::process(const float* const* inputs,uint32_t ni,float* const* outputs,uint32_t no,uint32_t frames,
                        const clap_event_transport_t* transport,const kick::clap::Event* events,uint32_t eventCount,bool adopt,const uint8_t* sources) noexcept
    {
        auto clear=[&]{if(outputs)for(uint32_t c=0;c<no;++c)if(outputs[c])std::fill_n(outputs[c],frames,0.f);};
        if(!frames || frames>maxFrames_ || ni>maxAudioChannels || no>maxAudioChannels || (ni && !inputs) ||
           (no && !outputs) || no<outputChannels_ || (eventCount && !events) || eventCount>kick::clap::Host::maxBlockEvents)
        {clear();return false;}
        for(uint32_t c=0;c<outputChannels_;++c)if(!outputs[c]){clear();return false;}
        const AudioPause::Block block(pause_);
        if(!block){clear();return true;} // setSampleRate is reactivating the plugins
        if(adopt)beginBlock();

        const bool panicInput=std::exchange(panicInput_,false),panicNotes=std::exchange(panicNotes_,false);
        auto* const listener=listener_.load(std::memory_order_acquire);
        // Live gains for this block: each slot ramps from where the last block left it.
        for(uint32_t i=1;i<liveGains;++i)
        {
            const bool jump=liveJump_[i].load(std::memory_order_relaxed) && liveJump_[i].exchange(false,std::memory_order_acquire);
            const float to=liveTarget_[i].load(std::memory_order_relaxed);
            liveFrom_[i]=jump?to:liveTo_[i];liveTo_[i]=to;
        }
        // Parameter values queued since the last block (queueParameter), for their nodes' steps.
        blockParameterCount_=0;
        for(auto tail=queueTail_.load(std::memory_order_relaxed),head=queueHead_.load(std::memory_order_acquire);tail!=head;++tail)
        {
            const auto& q=queued_[tail%queued_.size()];
            if(q.modulation)
            {
                clap_event_param_mod_t m{};
                m.header={sizeof(m),0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_PARAM_MOD,CLAP_EVENT_IS_LIVE};
                m.param_id=q.param;m.note_id=-1;m.port_index=m.channel=m.key=-1;m.amount=q.value;
                blockParameters_[blockParameterCount_++]={q.node,kick::clap::Event::from(m)};
            }
            else
            {
                clap_event_param_value_t v{};
                v.header={sizeof(v),0,CLAP_CORE_EVENT_SPACE_ID,CLAP_EVENT_PARAM_VALUE,CLAP_EVENT_IS_LIVE};
                v.param_id=q.param;v.note_id=-1;v.port_index=v.channel=v.key=-1;v.value=q.value;
                blockParameters_[blockParameterCount_++]={q.node,kick::clap::Event::from(v)};
            }
            queueTail_.store(tail+1,std::memory_order_release);
        }
        Plan* plan = current_;
        if(!plan){clear();return true;}
        // RFC 0032: the free-running beat (oscairn-clap/beat.h), the same for every node this block.
        const double tempo=transport && (transport->flags&CLAP_TRANSPORT_HAS_TEMPO) && std::isfinite(transport->tempo) && transport->tempo>0?transport->tempo:120.0;
        oscairn_event_beat_t beat{};
        beat.header={sizeof(beat),0,kick::clap::Host::beatSpace,OSCAIRN_EVENT_BEAT,0};
        beat.beats=clap_beattime(std::llround(freeBeats_*double(CLAP_BEATTIME_FACTOR)));
        beat.tempo=tempo;
        const auto beatEvent=kick::clap::Event::from(beat);
        freeBeats_+=frames/sampleRate_*tempo/60.0;
        // The block in the engine core (engine_core.h's run_block); the plugins, a sound context's parameters, the
        // listener, the clock, the worker deadline (realtime.h) and the audio helpers through these calls.
        struct Calls
        {
            Plan* plan; uint32_t frames; const clap_event_transport_t* transport; Listener* listener; AudioHelpers* helpers; const float* liveFrom; const float* liveTo;
            static Calls& of(void* c) { return *static_cast<Calls*>(c); }
            static Plan::Step& step(void* c,uint32_t s) { return of(c).plan->steps[s]; }
            static PluginHost& host(void* c,uint32_t s) { return *step(c,s).host; }
            static const kick::clap::Event* events(const oscairn_event* e) { return reinterpret_cast<const kick::clap::Event*>(e); }
            // RFC 0045's plugin groups: a step rendered in its sources' round trip, handed its routes with the block's live
            // gain products, as the core's mix takes them.
            static bool start(void* c,uint32_t s,const oscairn_event* e,uint32_t n)
            {
                auto& st=step(c,s);auto& cx=of(c);
                if(st.fused.empty())return st.host->startProcess(st.in,st.audioInputs,st.out,st.audioOutputs,cx.frames,events(e),n,cx.transport);
                std::array<PluginHost::GroupRoute,PluginHost::maxGroupRoutes> routes;
                const uint32_t count=uint32_t(std::min(st.fused.size(),routes.size()));
                for(uint32_t i=0;i<count;++i)
                {
                    const auto& f=st.fused[i];const auto& source=cx.plan->steps[size_t(f.source)];
                    float start=1,end=1;
                    for(const auto slot:f.live)if(slot){start*=cx.liveFrom[slot];end*=cx.liveTo[slot];}
                    routes[i]={source.host.get(),source.out,source.audioOutputs,f.from,f.to,f.gain,start,end,f.live[0]!=0,f.first};
                }
                return st.host->startGrouped(routes.data(),count,st.in,st.audioInputs,st.out,st.audioOutputs,cx.frames,events(e),n,cx.transport);
            }
            // Never rested while a source was handed this block: its input in the engine is not that block's yet.
            static bool handedSource(void* c,uint32_t s)
            {
                const auto& st=step(c,s);
                return std::any_of(st.fused.begin(),st.fused.end(),[&](const Plan::Step::Fused& f){return of(c).plan->steps[size_t(f.source)].host->handed();});
            }
        } calls{plan,frames,transport,listener,helpers_,liveFrom_.data(),liveTo_.data()};
        using C=Calls;
        static const oscairn_plugins table{nullptr,
            [](void* c,uint32_t s,uint32_t* n){return coreEvents(C::host(c,s).outputEvents(*n));},
            [](void* c,uint32_t s,const oscairn_event* e,uint32_t n){return C::start(c,s,e,n);},
            [](void* c,uint32_t s,const oscairn_event* e,uint32_t n){auto& st=C::step(c,s);return st.host->process(st.in,st.audioInputs,st.out,st.audioOutputs,C::of(c).frames,C::events(e),n,C::of(c).transport);},
            [](void* c,uint32_t s){return C::host(c,s).finishProcess();},
            [](void* c,uint32_t s){return C::host(c,s).oneBlockLate();},
            [](void* c,uint32_t s){C::host(c,s).panic();},
            [](void* c,uint32_t s){return C::host(c,s).eventsKept();},
            [](void* c,uint32_t s){return C::host(c,s).wasReset();},
            [](void* c,uint32_t s){return C::host(c,s).asleep();},
            [](void* c,uint32_t s){return C::host(c,s).voiceBudget();},
            [](void* c,uint32_t s){return C::host(c,s).honorsVolume();},
            [](void* c,uint32_t s){return C::host(c,s).missedBlocks();},
            [](void* c,uint32_t s){return C::host(c,s).blockMicroseconds();},
            [](void* c,uint32_t s,double* cost,const oscairn_voice_budget_note_t** notes,uint32_t* n){return C::host(c,s).voiceReport(*cost,*notes,*n);},
            [](void* c,uint32_t s,const oscairn_voice_budget_importance_t** lists){return C::host(c,s).importanceReport(*lists);},
            [](void* c,uint32_t s,uint32_t limit,const oscairn_voice_budget_context_t* parts,uint32_t n){C::host(c,s).setVoiceLimits(limit,parts,n);},
            [](void* c,uint32_t s,uint32_t k){return C::step(c,s).contexts[k].sound->sound();},
            [](void* c,uint32_t s,uint32_t k,oscairn_emit emit,void* e){C::step(c,s).contexts[k].sound->takeParameters([&](const kick::clap::Event& x){emit(e,coreEvents(&x));});},
            [](void* c,uint32_t node,const oscairn_event* e,uint32_t n,bool sent){C::of(c).listener->events(node,C::events(e),n,sent);},
            [](void*){return int64_t(std::chrono::steady_clock::now().time_since_epoch().count());},
            [](void*,int64_t at,bool only){worker::workerDeadline=std::chrono::steady_clock::time_point(std::chrono::steady_clock::duration(at));worker::workerDeadlineOnly=only;},
            [](void* c,uint32_t n,oscairn_job job,void* context){C::of(c).helpers->run(n,reinterpret_cast<AudioHelpers::Job>(job),context);},
            [](void* c,uint32_t s,uint32_t frames){return !C::handedSource(c,s) && C::host(c,s).rest(frames);},
            [](void* c,uint32_t s,float* cpu){float wait=-1;C::host(c,s).blockTimes(*cpu,wait);return wait;}};
        static_assert(std::is_same_v<std::chrono::steady_clock::duration,std::chrono::nanoseconds>);
        oscairn_plugins plugins=table;plugins.context=&calls;
        const float period=float(frames/sampleRate_*1e6),deadline=period*float(1-worker::mixingReserve);
        const auto callbackEnds=worker::callbackDeadline;
        const oscairn_run_block run{inputs,ni,outputs,no,frames,std::max(1u,uint32_t(sampleRate_*.001)),coreEvents(events),eventCount,sources,
            reinterpret_cast<const oscairn_param*>(blockParameters_.data()),blockParameterCount_,coreEvents(&beatEvent),panicInput,panicNotes,listener!=nullptr,
            parallel_.load(std::memory_order_relaxed),deadlines_,transport_.changed(transport,frames,sampleRate_),
            {voiceBudget_.load(std::memory_order_relaxed),period,deadline,deadlines_?deadline:workerBudgetUs_>0?workerBudgetUs_:period/2,audioCores_,uint64_t(noteEndQuiet*sampleRate_),lowSlack_},
            int64_t(callbackEnds.time_since_epoch().count()),helpers_->worth(),liveFrom_.data(),liveTo_.data(),&nextNoteId_,reinterpret_cast<uint64_t*>(&eventDrops_),
            reinterpret_cast<oscairn_midi*>(midiOut_.data()),&midiOutCount_};
        return rust_core_run_block(plan->run.get(),&run,&plugins);
    }

    const float* Graph::audioOutput(NodeId node,uint32_t channel) const noexcept
    {
        const Plan* plan=current_;
        if(!plan)return nullptr;
        if(node==graphInput)return channel<inputChannels_?rust_core_audio_buffers(plan->audio.get(),-1,false)[channel]:nullptr;
        for(const auto& step:plan->steps)
        {
            if(step.id==node && step.contexts.empty())return channel<step.audioOutputs?step.out[channel]:nullptr;
            for(const auto& c:step.contexts)if(c.id==node)return channel<c.channels?step.out[c.offset+channel]:nullptr;
        }
        return nullptr;
    }
}
