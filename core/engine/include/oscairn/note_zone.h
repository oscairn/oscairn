// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include <array>
#include <cmath>
#include <cstddef>
#include <cstdint>

namespace oscairn
{
    /// Where an event entered the graph: a MIDI input endpoint (RFC 0012 selectors). The host
    /// numbers its inputs 1..63; 0 is the built-in panel, keys and pads. Events from plugins,
    /// or from a host that does not say, are untagged and pass every zone's source mask.
    inline constexpr uint8_t builtInSource = 0, untaggedSource = 0xFF;
    inline constexpr size_t maxSources = 64;

    /// A zone on a note connection (RFC 0011): which notes pass, and how they change. A note
    /// passes when its key, velocity, MIDI channel and source are in range; it is then
    /// transposed and its velocity shaped by the curve, or set to a fixed velocity.
    struct NoteZone
    {
        uint8_t lowKey = 0, highKey = 127;
        double lowVelocity = 0, highVelocity = 1; // as CLAP velocities, 0..1 (MIDI velocity / 127)
        uint16_t channels = 0xFFFF;               // one bit per MIDI channel
        int8_t transpose = 0;
        double curve = 1;                         // velocity out = in^curve: above 1 softer, below 1 harder
        double fixedVelocity = 0;                 // above 0: every note plays at this velocity
        uint64_t sources = ~0ull;                 // one bit per source; none: no device plays it
        // Notes at once on the connection (RFC 0018 step 2), the first zone's for all of them; 0:
        // no limit. A note past it fades out and chokes the oldest released note, else the oldest
        // held one.
        uint16_t maxNotes = 0;
        // The part whose connections share that limit, one count across its lanes' instruments, the
        // first zone's; 0: the connection counts alone.
        uint32_t part = 0;
        // The part's settings for the shared budget (RFC 0018 step 3), the first zone's: the held
        // notes it keeps when parts give notes up to each other, and its priority, 0..10, lower
        // giving way first.
        uint16_t reserve = 0;
        uint8_t priority = 0;
        // Which devices' pedals the part takes (its input's `receive`), the first zone's: two bits
        // per Pedal class, a PedalReceive each (pedalReceive, below); 0: every pedal from every device.
        uint8_t pedals = 0;
        // The same for the other standard controllers (Function, below): two bits per class, a PedalReceive each; 0: every device's.
        uint16_t functions = 0;
        bool operator==(const NoteZone&) const = default;

        bool valid() const
        {
            return lowKey <= highKey && highKey <= 127 && lowVelocity >= 0 && lowVelocity <= highVelocity && highVelocity <= 1 &&
                   channels && transpose >= -127 && std::isfinite(curve) && curve > 0 && curve <= 16 && fixedVelocity >= 0 && fixedVelocity <= 1;
        }
        bool hears(uint8_t source) const noexcept { return source >= maxSources || (sources >> source & 1); }
        bool accepts(int key, double velocity, int channel, uint8_t source = untaggedSource) const noexcept
        {
            return key >= lowKey && key <= highKey && velocity >= lowVelocity && velocity <= highVelocity && (channel < 0 || channel > 15 || channels & (1u << channel)) && hears(source);
        }
        double shape(double velocity) const noexcept { return fixedVelocity > 0 ? fixedVelocity : std::pow(velocity, curve); }
    };

    /// A part's pedals (PartInput::pedals, NoteZone::pedals): for each class, whose it takes: every
    /// device's (All, the default, whichever device its notes come from), none, or only those of the
    /// devices it takes notes from (Own: its zones' sources).
    enum class Pedal : uint8_t { Sustain, Sostenuto, Soft }; // CC 64, 66, 67
    inline constexpr size_t pedalClasses = 3;
    enum class PedalReceive : uint8_t { All = 0, None = 1, Own = 2 };
    inline PedalReceive pedalReceive(uint8_t pedals, Pedal p) noexcept { return PedalReceive(pedals >> (2 * int(p)) & 3); }
    inline uint8_t withPedalReceive(uint8_t pedals, Pedal p, PedalReceive r) noexcept { const int s = 2 * int(p); return uint8_t((pedals & ~(3 << s)) | int(r) << s); }
    inline int pedalOf(uint8_t controller) noexcept { return controller == 64 ? 0 : controller == 66 ? 1 : controller == 67 ? 2 : -1; } // its Pedal, or -1

    /// The other standard controllers a part takes by its receive setting, as a pedal is (PartInput::functions,
    /// NoteZone::functions): pitch bend (0xE0), the mod wheel (CC 1), breath (CC 2), foot (CC 4), expression (CC 11),
    /// portamento (CC 65) and channel pressure (0xD0). A part not taking one gets none of that message, from any device.
    enum class Function : uint8_t { PitchBend, Modulation, Breath, Foot, Expression, Portamento, Aftertouch };
    inline constexpr size_t functionClasses = 7;
    inline PedalReceive functionReceive(uint16_t functions, Function f) noexcept { return PedalReceive(functions >> (2 * int(f)) & 3); }
    inline uint16_t withFunctionReceive(uint16_t functions, Function f, PedalReceive r) noexcept { const int s = 2 * int(f); return uint16_t((functions & ~(3 << s)) | int(r) << s); }
    /// The Function a MIDI message (status kind 0xB0 with controller `d1`, 0xD0 or 0xE0) is, or -1.
    inline int functionOf(uint8_t kind, uint8_t d1) noexcept
    {
        if(kind == 0xE0) return 0;
        if(kind == 0xD0) return 6;
        if(kind != 0xB0) return -1;
        return d1 == 1 ? 1 : d1 == 2 ? 2 : d1 == 4 ? 3 : d1 == 11 ? 4 : d1 == 65 ? 5 : -1;
    }

    /// The zones of one note connection, fixed in size so the audio thread never allocates.
    /// None: every note passes unchanged.
    struct NoteZones
    {
        static constexpr size_t capacity = 8;
        std::array<NoteZone, capacity> zones{};
        size_t count = 0;
        bool passes(int channel, uint8_t source) const noexcept // a MIDI message other than a note
        {
            for(size_t i = 0; i < count; ++i) if(zones[i].channels & (1u << channel) && zones[i].hears(source)) return true;
            return false;
        }
        bool hears(uint8_t source) const noexcept
        {
            for(size_t i = 0; i < count; ++i) if(zones[i].hears(source)) return true;
            return false;
        }
        // Whether the pedal `controller` (CC 64, 66 or 67) from `source` is one the part takes, by the
        // first zone's `pedals`: any source's (All, whatever the zones' devices), none, or a source its
        // zones hear (Own). No zones: every one.
        bool takesFunction(int function, uint8_t source) const noexcept
        {
            if(!count || function < 0) return true;
            switch(functionReceive(zones[0].functions, Function(function)))
            {
                case PedalReceive::None: return false;
                case PedalReceive::Own: return hears(source);
                default: return true;
            }
        }
        bool takesPedal(uint8_t controller, uint8_t source) const noexcept
        {
            const int p = pedalOf(controller);
            if(!count || p < 0) return true;
            switch(pedalReceive(zones[0].pedals, Pedal(p)))
            {
                case PedalReceive::None: return false;
                case PedalReceive::Own: return hears(source);
                default: return true;
            }
        }
    };
}
