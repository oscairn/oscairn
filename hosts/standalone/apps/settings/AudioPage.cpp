// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AudioPage.h"

namespace oscairn::standalone::settings
{
    namespace
    {
        juce::String text(const std::string& s) { return juce::String::fromUTF8(s.c_str()); }
        juce::String hertz(double rate) { return juce::String(juce::roundToInt(rate)) + " Hz"; }
        // Where the device came from, in plain words (audio.devices' source).
        juce::String source(const std::string& s)
        {
            if(s == "user") return "Chosen here; it opens again at the next start.";
            if(s == "profile") return "Set when the engine started, by this device's profile or its options.";
            return "The system's default device.";
        }
        // Items 1, 2, 3... in order; the one matching `chosen` selected, none when it is not there.
        template<class T, class Name> void fill(juce::ComboBox& box, const std::vector<T>& items, const T& chosen, Name name)
        {
            box.clear(juce::dontSendNotification);
            for(size_t i = 0; i < items.size(); ++i) box.addItem(name(items[i]), int(i) + 1);
            const auto at = std::find(items.begin(), items.end(), chosen);
            box.setSelectedId(at == items.end() ? 0 : int(at - items.begin()) + 1, juce::dontSendNotification);
        }
    }

    AudioPage::AudioPage(Engine& engine) : engine_(engine)
    {
        outputRow_ = &addRow("Output", output);
        inputRow_ = &addRow("Input", input, "The audio inputs' device.");
        rateRow_ = &addRow("Sample rate", rate, "Samples a second. A new rate restarts every plugin at it, keeping its sound.");
        sizeRow_ = &addRow("Buffer size", size, "Smaller answers your playing sooner; larger is safer from dropouts.");
        inputRow_->setVisible(false);
        for(auto* box : {&output, &input, &rate, &size}) box->setTextWhenNothingSelected("Reading...");
        auto device = [](const std::vector<Control::AudioDevice>& list, juce::ComboBox& box) { const auto i = size_t(box.getSelectedId() - 1); return i < list.size() ? list[i].name : std::string(); };
        output.onChange = [this, device] { Control::AudioSetting s; s.output = device(devices_.outputs, output); apply(s, *outputRow_); };
        input.onChange = [this, device] { Control::AudioSetting s; s.input = device(devices_.inputs, input); apply(s, *inputRow_); };
        rate.onChange = [this] {
            const auto* d = [&]() -> const Control::AudioDevice* { for(const auto& o : devices_.outputs) if(o.name == devices_.output) return &o; return nullptr; }();
            if(const auto i = size_t(rate.getSelectedId() - 1); d && i < d->sampleRates.size()) { Control::AudioSetting s; s.sampleRate = d->sampleRates[i]; apply(s, *rateRow_); }
        };
        size.onChange = [this] {
            const auto* d = [&]() -> const Control::AudioDevice* { for(const auto& o : devices_.outputs) if(o.name == devices_.output) return &o; return nullptr; }();
            if(const auto i = size_t(size.getSelectedId() - 1); d && i < d->bufferSizes.size()) { Control::AudioSetting s; s.bufferSize = d->bufferSizes[i]; apply(s, *sizeRow_); }
        };
        seen_ = seen();
        list();
        startTimer(250);
    }

    AudioPage::Seen AudioPage::seen() const
    {
        const auto s = engine_.sessionStatus();
        return {s.audioDeviceList, s.audioDevice, s.sampleRate, s.bufferSize};
    }

    void AudioPage::list()
    {
        busy_ = true; show();
        auto listed = std::make_shared<Control::AudioDevices>();
        engine_.job([listed](Control& c) { *listed = c.audioDevices(); },
            [safe = juce::Component::SafePointer<AudioPage>(this), listed](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false; ++safe->listings_;
                safe->listError_ = text(error);
                if(error.empty()) safe->devices_ = *listed;
                safe->show();
            });
    }

    void AudioPage::apply(const Control::AudioSetting& setting, Row& changed)
    {
        if(busy_) return;
        busy_ = true;
        refusedRow_ = nullptr; refusal_.clear();
        show();
        changed.setExplanation(&changed == outputRow_ || &changed == inputRow_ ? "Opening it..." : "Restarting the audio device...");
        auto applied = std::make_shared<Control::AudioDevices>();
        engine_.job([setting, applied](Control& c) { *applied = c.setAudio(setting); },
            [safe = juce::Component::SafePointer<AudioPage>(this), applied, row = &changed](const std::string& error) {
                if(!safe) return;
                safe->busy_ = false;
                if(error.empty()) { safe->devices_ = *applied; safe->show(); return; }
                // Refused, or the device did not open (the last one plays again): said under it, and listed again as it is.
                safe->refusedRow_ = row; safe->refusal_ = text(error);
                safe->list();
            });
    }

    void AudioPage::show()
    {
        const auto& d = devices_;
        fill(output, [&] { std::vector<std::string> n; for(const auto& o : d.outputs) n.push_back(o.name); return n; }(), d.output, text);
        fill(input, [&] { std::vector<std::string> n; for(const auto& i : d.inputs) n.push_back(i.name); return n; }(), d.input, text);
        const Control::AudioDevice* open = nullptr;
        for(const auto& o : d.outputs) if(o.name == d.output) open = &o;
        fill(rate, open ? open->sampleRates : std::vector<double>{}, d.sampleRate, hertz);
        fill(size, open ? open->bufferSizes : std::vector<uint32_t>{}, d.bufferSize, [hz = d.sampleRate](uint32_t frames) {
            return juce::String(int(frames)) + " samples" + (hz > 0 ? " (" + juce::String(frames * 1000.0 / hz, 1) + " ms)" : juce::String());
        });
        output.setEnabled(!busy_ && !d.fixed && d.outputs.size() > 1);
        input.setEnabled(!busy_ && d.inputs.size() > 1);
        rate.setEnabled(!busy_ && open && open->sampleRates.size() > 1);
        size.setEnabled(!busy_ && open && open->bufferSizes.size() > 1);
        if(busy_) return; // what is being done stays said until it is done
        // The device: why it is the one open, and anything passed over on the way.
        juce::String said = d.source == "null" ? "No audio device: started with --audio null." // fixed too, but by the option
                          : d.fixed ? "Fixed by the device profile: its channels are wired to this device." : source(d.source);
        if(!d.notice.empty()) said = text(d.notice) + "\n" + said;
        if(listError_.isNotEmpty()) said = "The devices could not be read: " + listError_;
        outputRow_->setExplanation(said, !d.notice.empty() || listError_.isNotEmpty());
        inputRow_->setExplanation("The audio inputs' device.");
        rateRow_->setExplanation(d.fixed && open && open->sampleRates.size() == 1 ? "Fixed by the device profile." : "Samples a second. A new rate restarts every plugin at it, keeping its sound.");
        sizeRow_->setExplanation("Smaller answers your playing sooner; larger is safer from dropouts.");
        if(refusedRow_) refusedRow_->setExplanation(refusal_, true);
        if(inputRow_->isVisible() != !d.inputs.empty()) { inputRow_->setVisible(!d.inputs.empty()); heightChanged(); }
    }

    void AudioPage::timerCallback()
    {
        // The status changed (devices came or went, or the device, its rate or size changed, here or elsewhere): read again.
        if(const auto now = seen(); !(now == seen_) && !busy_) { seen_ = now; list(); }
    }
}
