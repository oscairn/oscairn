// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <algorithm>
#include <charconv>
#include <cstdint>
#include <filesystem>
#include <stdexcept>
#include <string>
#include <string_view>
#include <vector>
namespace oscairn::host
{
struct AudioConfiguration
{
    std::string inputDevice,outputDevice;
    std::vector<uint32_t> inputs,outputs{0,1}; // logical order -> physical channel (zero based)
    bool explicitOutputs=false;
    uint32_t bufferFrames=0; // the block the device is asked for (a device profile's, RFC 0024); 0: the build's default
    // A profile's audio.device (RFC 0024): the device's channels, opened in each direction used
    // whether mapped or not (a TDM link opens all its slots), and the rate it must run at. 0: as today.
    uint32_t deviceChannels=0,sampleRate=0;
    std::string panelDevice,panelMap; // a non-MIDI panel the engine reads and its message map (a profile's panel, DeviceReaders.h); empty: none
    std::string keyboardDevice; // non-MIDI keys the engine reads (a profile's keyboard, DeviceReaders.h); empty: none
    std::vector<std::string> controllers; // controller profile files (RFC 0024), each by its full path, beside the built-in ones
    bool nullDevice=false; // --audio null: the null device (NullDevice.h), writing nowhere, with no MIDI device opened; for tests, never a fallback
    static bool isOption(std::string_view name)
    {return name=="--audio-inputs" || name=="--audio-outputs" || name=="--input-device" || name=="--output-device" || name=="--buffer-frames" ||
            name=="--device-channels" || name=="--sample-rate" || name=="--panel-device" || name=="--panel-map" || name=="--keyboard-device" || name=="--controller" || name=="--audio";}
    void setOption(std::string_view name,const std::string& value)
    {
        if(value.empty() || value.find('\0')!=std::string::npos)throw std::invalid_argument("Empty or invalid audio option");
        if(name=="--audio-inputs")inputs=parseChannels(value);
        else if(name=="--audio-outputs"){outputs=parseChannels(value);explicitOutputs=true;}
        else if(name=="--input-device")inputDevice=value;
        else if(name=="--output-device")outputDevice=value;
        else if(name=="--panel-device")panelDevice=value;
        else if(name=="--panel-map")panelMap=value;
        else if(name=="--keyboard-device")keyboardDevice=value;
        else if(name=="--controller")controllers.push_back(std::filesystem::absolute(value).string()); // as many as wanted
        else if(name=="--buffer-frames")bufferFrames=number(value,16,8192,"--buffer-frames is 16 to 8192");
        else if(name=="--device-channels")deviceChannels=number(value,1,256,"--device-channels is 1 to 256");
        else if(name=="--sample-rate")sampleRate=number(value,8000,384000,"--sample-rate is 8000 to 384000");
        else if(name=="--audio"){if(value!="null")throw std::invalid_argument("--audio takes null");nullDevice=true;}
        else throw std::invalid_argument("Unknown audio option");
    }
    void validate() const
    {
        for(const auto* list:{&inputs,&outputs})
        {
            std::vector<uint32_t> seen;
            for(auto c:*list){if(c>=256 || std::find(seen.begin(),seen.end(),c)!=seen.end())throw std::invalid_argument("Invalid or duplicate device channel");seen.push_back(c);}
        }
        if(outputs.empty())throw std::invalid_argument("Audio device needs at least one output");
        if(!explicitOutputs && outputs!=std::vector<uint32_t>{0,1})throw std::invalid_argument("Custom outputs require explicit selection");
        if(inputDevice.find('\0')!=std::string::npos || outputDevice.find('\0')!=std::string::npos)throw std::invalid_argument("Invalid audio device name");
        if(panelDevice.empty()!=panelMap.empty())throw std::invalid_argument("--panel-device and --panel-map come together");
        for(const auto* list:{&inputs,&outputs})
            for(auto c:*list)if(deviceChannels && c>=deviceChannels)throw std::invalid_argument("A mapped channel is beyond --device-channels");
    }
    std::vector<std::string> arguments() const
    {
        validate();
        auto list=[](const auto& channels){std::string result;for(auto c:channels){if(!result.empty())result+=',';result+=std::to_string(c);}return result;};
        std::vector<std::string> args;
        if(nullDevice){args.push_back("--audio");args.push_back("null");}
        if(!inputDevice.empty()){args.push_back("--input-device");args.push_back(inputDevice);}
        if(!outputDevice.empty()){args.push_back("--output-device");args.push_back(outputDevice);}
        if(!inputs.empty()){args.push_back("--audio-inputs");args.push_back(list(inputs));}
        if(explicitOutputs){args.push_back("--audio-outputs");args.push_back(list(outputs));}
        if(bufferFrames){args.push_back("--buffer-frames");args.push_back(std::to_string(bufferFrames));}
        if(deviceChannels){args.push_back("--device-channels");args.push_back(std::to_string(deviceChannels));}
        if(sampleRate){args.push_back("--sample-rate");args.push_back(std::to_string(sampleRate));}
        if(!panelDevice.empty()){args.push_back("--panel-device");args.push_back(panelDevice);args.push_back("--panel-map");args.push_back(panelMap);}
        if(!keyboardDevice.empty()){args.push_back("--keyboard-device");args.push_back(keyboardDevice);}
        for(const auto& c:controllers){args.push_back("--controller");args.push_back(c);}
        return args;
    }
    static uint32_t number(std::string_view value,uint32_t low,uint32_t high,const char* why)
    {
        uint32_t n=0;const auto parsed=std::from_chars(value.data(),value.data()+value.size(),n);
        if(parsed.ec!=std::errc{} || parsed.ptr!=value.data()+value.size() || n<low || n>high)throw std::invalid_argument(why);
        return n;
    }
    static std::vector<uint32_t> parseChannels(std::string_view text)
    {
        if(text=="none")return {};
        std::vector<uint32_t> result;
        while(!text.empty())
        {
            const auto comma=text.find(',');const auto item=text.substr(0,comma);uint32_t n=0;
            const auto parsed=std::from_chars(item.data(),item.data()+item.size(),n);
            if(parsed.ec!=std::errc{} || parsed.ptr!=item.data()+item.size() || n>=256 ||
               std::find(result.begin(),result.end(),n)!=result.end())throw std::invalid_argument("Audio channels must be unique zero-based indices 0..255");
            result.push_back(n);
            if(comma==std::string_view::npos)return result;
            text.remove_prefix(comma+1);
        }
        throw std::invalid_argument("Empty or incomplete audio channel list (use none to disable inputs)");
    }
};
}
