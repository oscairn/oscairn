// SPDX-License-Identifier: AGPL-3.0-or-later
// Samples for tests, made here from synthetic audio: one-record packs written by samplepack's own
// command-line tool from a 16-bit WAV, each named <sample id>.osp, and waveforms written by the
// library's sp_waveform_build. No recorded audio.
#pragma once
#include <samplepack.h>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace sample_source
{
    struct Sample { std::vector<int16_t> pcm; uint32_t rate = 44100, channels = 1; std::string id; std::filesystem::path pack; };

    inline std::string hex(const uint8_t* id) { constexpr char d[] = "0123456789abcdef"; std::string s; for(int i = 0; i < 32; ++i) { s += d[id[i] >> 4]; s += d[id[i] & 15]; } return s; }

    // Audio unlike any other `seed`'s: a tone and some noise, frames long.
    inline std::vector<int16_t> audio(uint32_t seed, size_t frames, uint32_t channels)
    {
        std::vector<int16_t> pcm(frames * channels);
        uint32_t noise = seed * 2654435761u + 1;
        for(size_t i = 0; i < pcm.size(); ++i)
        {
            noise = noise * 1664525u + 1013904223u;
            pcm[i] = int16_t(std::lround(9000 * std::sin(double(i / channels) * (0.01 + 0.003 * seed))) + int32_t(noise >> 24) - 128);
        }
        return pcm;
    }

    // A one-record pack in `folder` from `cli` (samplepack), its name the sample's id.
    inline Sample pack(const std::string& cli, const std::filesystem::path& folder, uint32_t seed, size_t frames = 6000, uint32_t channels = 1, uint32_t rate = 44100)
    {
        Sample s{audio(seed, frames, channels), rate, channels, {}, {}};
        uint8_t id[32];
        if(sp_sample_id_pcm(rate, channels, s.pcm.data(), frames, id) != SP_OK) throw std::runtime_error("no sample id");
        s.id = hex(id);
        std::filesystem::create_directories(folder);
        const auto wav = folder / (s.id + ".wav");
        {
            std::ofstream out(wav, std::ios::binary);
            auto u32 = [&](uint32_t v) { out.write(reinterpret_cast<const char*>(&v), 4); };
            auto u16 = [&](uint16_t v) { out.write(reinterpret_cast<const char*>(&v), 2); };
            const uint32_t bytes = uint32_t(s.pcm.size() * 2);
            out.write("RIFF", 4); u32(36 + bytes); out.write("WAVEfmt ", 8); u32(16); u16(1); u16(uint16_t(channels)); u32(rate); u32(rate * channels * 2); u16(uint16_t(channels * 2)); u16(16);
            out.write("data", 4); u32(bytes); out.write(reinterpret_cast<const char*>(s.pcm.data()), std::streamsize(bytes));
        }
        s.pack = folder / (s.id + ".osp");
        const auto command = "\"" + cli + "\" pack -o \"" + s.pack.string() + "\" \"" + wav.string() + "\" > /dev/null";
        if(std::system(command.c_str()) != 0) throw std::runtime_error("samplepack pack failed: " + command);
        std::filesystem::remove(wav);
        return s;
    }

    // A waveform naming `samples`, one key bank each across the keyboard, written to `folder` as <id>.owf.
    inline std::filesystem::path waveform(const std::filesystem::path& folder, const std::vector<std::string>& samples, std::string* idOut = nullptr)
    {
        std::vector<sp_key_bank> banks;
        std::vector<uint8_t> ids(32 * samples.size());
        for(size_t i = 0; i < samples.size(); ++i)
        {
            for(int b = 0; b < 32; ++b) ids[32 * i + b] = uint8_t(std::stoi(samples[i].substr(2 * b, 2), nullptr, 16));
            sp_key_bank k{};
            k.key_lo = uint8_t(i * 12); k.key_hi = uint8_t(i * 12 + 11); k.vel_lo = 1; k.vel_hi = 127; k.root = uint8_t(i * 12 + 6); k.sample = uint16_t(i);
            banks.push_back(k);
        }
        std::vector<uint8_t> out(SP_WAVEFORM_MAX_BYTES);
        size_t written = 0;
        if(sp_waveform_build(banks.data(), banks.size(), reinterpret_cast<const uint8_t(*)[32]>(ids.data()), samples.size(), out.data(), out.size(), &written) != SP_OK) throw std::runtime_error("no waveform");
        uint8_t id[32];
        sp_waveform_id(out.data(), written, id);
        std::filesystem::create_directories(folder);
        const auto file = folder / (hex(id) + ".owf");
        std::ofstream(file, std::ios::binary).write(reinterpret_cast<const char*>(out.data()), std::streamsize(written));
        if(idOut) *idOut = hex(id);
        return file;
    }

    // Decodes the first block of the record at `offset` in `pack` (channel 0), as a plugin does.
    inline std::vector<int16_t> firstBlock(const std::filesystem::path& pack, uint64_t offset, uint64_t length, uint64_t preload, const std::string& id)
    {
        std::ifstream in(pack, std::ios::binary);
        const std::string bytes((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
        sp_index_entry entry{{}, offset, length, preload};
        for(int b = 0; b < 32; ++b) entry.id[b] = uint8_t(std::stoi(id.substr(2 * b, 2), nullptr, 16));
        static sp_flac_scratch scratch{};
        std::vector<int16_t> ch0(SP_FLAC_MAX_BLOCK), ch1(SP_FLAC_MAX_BLOCK);
        uint64_t frames = 0;
        if(const auto s = sp_record_decode(reinterpret_cast<const uint8_t*>(bytes.data()), bytes.size(), &entry, 0, 1, 0, &scratch, ch0.data(), ch1.data(), ch0.size(), &frames); s != SP_OK)
            throw std::runtime_error("decode failed: " + std::to_string(s));
        ch0.resize(frames);
        return ch0;
    }
    // Channel 0 of a sample's first `frames` frames.
    inline std::vector<int16_t> channel0(const Sample& s, size_t frames)
    {
        std::vector<int16_t> out;
        for(size_t i = 0; i < frames && i * s.channels < s.pcm.size(); ++i) out.push_back(s.pcm[i * s.channels]);
        return out;
    }
}
