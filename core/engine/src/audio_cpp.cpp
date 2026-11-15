// SPDX-License-Identifier: AGPL-3.0-or-later
// A plan's audio (engine_core.h's audio_*) in C++, graph.cpp's code as it was before the Rust core: the
// reference the Rust implementation (core/engine/rust/src/audio.rs) is held to by engine-audio-diff,
// and the one OSCAIRN_ENGINE_CORE=cpp builds the engine with.
#include "engine_core.h"
#include "oscairn/graph.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <cmath>
#include <cstring>
#include <vector>

using oscairn::Graph;
static Graph::Activity* activity(oscairn_activity* a) { return reinterpret_cast<Graph::Activity*>(a); }

// Laid out as Graph::Plan was: each step its own buffers, a connection finding its source's buffer
// through the steps each block.
struct oscairn_audio
{
    using Peaks=std::array<std::atomic<float>,Graph::meterChannels>;
    struct AudioRoute { int source; uint32_t from, to; float gain; bool first=false; mutable float now=0, delta=0; mutable uint32_t left=0; std::array<uint8_t,4> live{}; };
    struct Bus { uint32_t offset, channels; Peaks* peaks; Graph::Activity* activity; };
    struct Step
    {
        const void* host; uint32_t audioInputs, audioOutputs, metered; Peaks* peaks; Graph::Activity* activity;
        std::vector<AudioRoute> audioSources; std::vector<uint32_t> silentInputs; std::vector<Bus> contexts;
        std::vector<float> inputBuffer, outputBuffer; std::vector<float*> out, in;
        std::vector<float> lastOut; uint32_t fadedFor = 0; // the output guard: each channel's last good sample, and the frames faded since
    };
    std::vector<Step> steps;
    std::vector<AudioRoute> outputRoutes;
    std::vector<float> externalInputs; std::vector<float*> external;
    std::vector<uint32_t> silentOutputs;
    uint32_t inputChannels, outputChannels, maxFrames;
};

extern "C" {
oscairn_audio* cpp_core_audio_new(const oscairn_audio_step* steps,uint32_t count,const oscairn_audio_route* outputs,uint32_t outputCount,uint32_t inputs,uint32_t channels,uint32_t maxFrames)
{
    auto* audio=new oscairn_audio{{},{},{},{},{},inputs,channels,maxFrames};
    auto routes=[](const oscairn_audio_route* r,uint32_t n){
        std::vector<oscairn_audio::AudioRoute> out;
        for(uint32_t i=0;i<n;++i){oscairn_audio::AudioRoute a{r[i].source,r[i].from,r[i].to,r[i].gain};a.now=a.gain;std::memcpy(a.live.data(),r[i].live,4);out.push_back(a);}
        return out;};
    auto prepare=[](auto& routes,uint32_t channels,auto& silent) {
        std::vector<bool> written(channels);
        for(auto& r:routes){r.first=!written[r.to];written[r.to]=true;}
        for(uint32_t c=0;c<channels;++c)if(!written[c])silent.push_back(c);
    };
    audio->outputRoutes=routes(outputs,outputCount);
    prepare(audio->outputRoutes,channels,audio->silentOutputs);
    audio->externalInputs.resize(size_t(inputs)*maxFrames);
    for(uint32_t c=0;c<inputs;++c)audio->external.push_back(audio->externalInputs.data()+size_t(c)*maxFrames);
    for(uint32_t i=0;i<count;++i)
    {
        const auto& s=steps[i];
        auto& step=audio->steps.emplace_back(oscairn_audio::Step{s.host,s.inputs,s.outputs,s.metered,reinterpret_cast<oscairn_audio::Peaks*>(s.peaks),activity(s.activity)});
        step.audioSources=routes(s.routes,s.route_count);
        for(uint32_t b=0;b<s.bus_count;++b)step.contexts.push_back({s.buses[b].offset,s.buses[b].channels,reinterpret_cast<oscairn_audio::Peaks*>(s.buses[b].peaks),activity(s.buses[b].activity)});
        prepare(step.audioSources,step.audioInputs,step.silentInputs);
        step.inputBuffer.resize(size_t(step.audioInputs)*maxFrames);
        step.outputBuffer.resize(size_t(step.audioOutputs)*maxFrames);
        for(uint32_t c=0;c<step.audioInputs;++c)step.in.push_back(step.inputBuffer.data()+size_t(c)*maxFrames);
        for(uint32_t c=0;c<step.audioOutputs;++c)step.out.push_back(step.outputBuffer.data()+size_t(c)*maxFrames);
        step.lastOut.assign(step.audioOutputs,0.f);
    }
    return audio;
}

void cpp_core_audio_free(oscairn_audio* audio) { delete audio; }

float* const* cpp_core_audio_buffers(oscairn_audio* audio,int32_t step,bool outputs)
{
    if(step<0)return audio->external.data();
    auto& s=audio->steps[size_t(step)];
    return outputs?s.out.data():s.in.data();
}

void cpp_core_audio_carry(oscairn_audio* next,const oscairn_audio* current,uint32_t ramp)
{
    auto carry=[&](const std::vector<oscairn_audio::AudioRoute>& was,std::vector<oscairn_audio::AudioRoute>& now){
        auto host=[](const oscairn_audio& a,int i){return i<0?nullptr:a.steps[size_t(i)].host;};
        for(auto& r:now)for(const auto& o:was)
            if(o.from==r.from && o.to==r.to && host(*current,o.source)==host(*next,r.source) && (o.source<0)==(r.source<0))
            {
                r.now=o.left?o.now:o.gain;
                if(r.now!=r.gain){r.left=ramp;r.delta=(r.gain-r.now)/float(ramp);}
                break;
            }
    };
    carry(current->outputRoutes,next->outputRoutes);
    for(auto& step:next->steps)for(const auto& old:current->steps)if(old.host==step.host){carry(old.audioSources,step.audioSources);break;}
}

void cpp_core_audio_begin(oscairn_audio* audio,const float* const* inputs,uint32_t ni,float* const* outputs,uint32_t no,uint32_t frames)
{
    for(uint32_t c=0;c<audio->inputChannels;++c)
    {
        auto* dest=audio->externalInputs.data()+size_t(c)*audio->maxFrames;
        if(c<ni && inputs[c])std::copy_n(inputs[c],frames,dest);else std::fill_n(dest,frames,0.f);
    }
    for(auto c:audio->silentOutputs)std::fill_n(outputs[c],frames,0.f);
    for(uint32_t c=audio->outputChannels;c<no;++c)if(outputs[c])std::fill_n(outputs[c],frames,0.f);
}

void cpp_core_audio_mix(oscairn_audio* audio,uint32_t s,float* const* outputs,uint32_t frames,const float* liveFrom,const float* liveTo)
{
    auto mix=[&](const oscairn_audio::AudioRoute& r,float* dest)
    {
        const float* from=r.source<0?audio->externalInputs.data()+size_t(r.from)*audio->maxFrames:audio->steps[r.source].out[r.from];
        if(r.live[0])
        {
            // Scaled by its live gains, ramped across the block from the last block's product.
            float start=1,end=1;
            for(const auto slot:r.live)if(slot){start*=liveFrom[slot];end*=liveTo[slot];}
            if(start!=end || r.left)
            {
                const float step=(end-start)/float(frames);
                for(uint32_t i=0;i<frames;++i)
                {
                    if(r.left){r.now+=r.delta;if(--r.left==0)r.now=r.gain;}
                    const float g=r.now*(start+step*float(i+1));
                    dest[i]=r.first?from[i]*g:dest[i]+from[i]*g;
                }
                return;
            }
            const float g=r.gain*end;
            if(r.first)for(uint32_t i=0;i<frames;++i)dest[i]=from[i]*g;
            else for(uint32_t i=0;i<frames;++i)dest[i]+=from[i]*g;
            return;
        }
        if(r.left)
        {
            for(uint32_t i=0;i<frames;++i)
            {
                if(r.left){r.now+=r.delta;if(--r.left==0)r.now=r.gain;}
                dest[i]=r.first?from[i]*r.now:dest[i]+from[i]*r.now;
            }
            return;
        }
        if(r.first)
        {
            if(r.gain==1.f)std::copy_n(from,frames,dest);
            else for(uint32_t i=0;i<frames;++i)dest[i]=from[i]*r.gain;
        }
        else if(r.gain==1.f)for(uint32_t i=0;i<frames;++i)dest[i]+=from[i];
        else for(uint32_t i=0;i<frames;++i)dest[i]+=from[i]*r.gain;
    };
    if(s==audio->steps.size()){for(const auto& r:audio->outputRoutes)mix(r,outputs[r.to]);return;}
    auto& step=audio->steps[s];
    for(auto c:step.silentInputs)std::fill_n(step.inputBuffer.data()+size_t(c)*audio->maxFrames,frames,0.f);
    for(const auto& r:step.audioSources)mix(r,step.inputBuffer.data()+size_t(r.to)*audio->maxFrames);
}

void cpp_core_audio_finish(oscairn_audio* audio,uint32_t s,uint32_t frames,bool processed,bool asleep,uint32_t n)
{
    auto& step=audio->steps[s];
    if(processed)
    {
        uint32_t bad=0;
        for(uint32_t c=0;c<step.audioOutputs;++c){const float* x=step.out[c];for(uint32_t i=0;i<frames;++i)bad|=uint32_t(!(std::fabs(x[i])<=Graph::outputBound));}
        auto count=[&](Graph::Activity& a){
            if(bad){a.badOutput.fetch_add(1,std::memory_order_relaxed);a.badRun.store(a.badRun.load(std::memory_order_relaxed)+1,std::memory_order_relaxed);}
            else a.badRun.store(0,std::memory_order_relaxed);};
        if(step.activity)count(*step.activity);else for(auto& c:step.contexts)count(*c.activity);
        if(!bad){for(uint32_t c=0;c<step.audioOutputs;++c)step.lastOut[c]=step.out[c][frames-1];step.fadedFor=0;}
        else
        {
            for(uint32_t c=0;c<step.audioOutputs;++c)for(uint32_t f=0;f<frames;++f)step.out[c][f]=step.fadedFor+f<n?step.lastOut[c]*float(n-1-step.fadedFor-f)/float(n):0.f;
            step.fadedFor=std::min(n,step.fadedFor+frames);
        }
    }
    auto meter=[&](const auto& peaks,uint32_t from,uint32_t count){
        for(uint32_t c=0;c<std::min(count,Graph::meterChannels);++c)
        {
            float peak=0;
            for(uint32_t i=0;i<frames;++i)peak=std::fmax(peak,std::abs(step.out[from+c][i]));
            auto& held=(*peaks)[c];
            for(float seen=held.load(std::memory_order_relaxed);peak>seen && !held.compare_exchange_weak(seen,peak,std::memory_order_relaxed););
        }};
    if(processed && step.peaks)meter(step.peaks,0,step.metered);
    // Its and its contexts' (a shared engine's, each its own bus): how long it has been silent, so a retiring node may go
    // and a free one be taken over (RFC 0021), and its age. Every block counts toward the age, rendered or not (a worker
    // that died and did not restart) and notes held or not (a lost release), or such a node is never idle.
    auto quiet=[&](Graph::Activity& a,uint32_t from,uint32_t channels){
        if(processed)
        {
            float loudest=0;for(uint32_t c=0;c<channels;++c)for(uint32_t i=0;i<frames;++i)loudest=std::fmax(loudest,std::abs(step.out[from+c][i]));
            a.quiet.store(loudest<1e-5f?a.quiet.load(std::memory_order_relaxed)+frames:0,std::memory_order_relaxed);
            a.asleep.store(asleep,std::memory_order_relaxed);
        }
        a.age.store(a.age.load(std::memory_order_relaxed)+frames,std::memory_order_relaxed);};
    for(auto& context:step.contexts){if(processed)meter(context.peaks,context.offset,context.channels);quiet(*context.activity,context.offset,context.channels);}
    if(step.activity)quiet(*step.activity,0,step.audioOutputs);
}
}
