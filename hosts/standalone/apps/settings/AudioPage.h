// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "Frame.h"

namespace oscairn::standalone::settings
{
    /// Audio (RFC 0024's user choice): the output device, the input device where the engine opens
    /// inputs, the sample rate and the buffer size, as audio.devices offers them, each changed
    /// with audio.set. Where the device came from and anything passed over are said under the
    /// device; a device the profile fixes is shown so and cannot be changed. Listed again when the
    /// status says the devices, the device or its rate or size changed. A change refused says why
    /// under what was changed. The engine is asked off the message thread (Engine::job).
    class AudioPage : public Page, private juce::Timer
    {
    public:
        explicit AudioPage(Engine& engine);
        juce::ComboBox output, input, rate, size;
        const Row& outputRow() const { return *outputRow_; }
        const Row& rateRow() const { return *rateRow_; }
        const Row& sizeRow() const { return *sizeRow_; }
        const Row& inputRow() const { return *inputRow_; }
        const Control::AudioDevices& devices() const { return devices_; }
        bool busy() const { return busy_; }        // a listing or a change is out
        int listings() const { return listings_; } // how many listings have arrived
        void apply(const Control::AudioSetting& setting, Row& changed); // ignored while busy, when the choices are disabled

    private:
        void list();
        void show();
        void timerCallback() override;
        struct Seen { uint64_t list = 0; std::string device; double rate = 0; uint32_t size = 0; bool operator==(const Seen&) const = default; };
        Seen seen() const;

        Engine& engine_;
        Control::AudioDevices devices_;
        bool busy_ = false;
        int listings_ = 0;
        Seen seen_;
        Row* refusedRow_ = nullptr; // where the last refusal is said, until a change goes through
        juce::String refusal_, listError_;
        Row *outputRow_, *inputRow_, *rateRow_, *sizeRow_;
    };
}
