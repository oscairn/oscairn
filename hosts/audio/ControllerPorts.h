// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "../standalone/Profile.h"
#include "oscairn/control.h"
#include <cstdint>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

namespace oscairn::host
{
    /// External MIDI controllers as the engine plays them (RFC 0024): the controller profiles
    /// (built in, and the --controller files it was given), the musician's choice of profiles per
    /// port name and the ports they ignore, kept in the data folder (controllers.json), and what
    /// follows for the MIDI inputs connected now: which input is which profile's port, the joined
    /// profiles' control groups, and the inputs left out. The engine's main thread and the control
    /// thread both reach it: everything is under its lock.
    class ControllerPorts
    {
    public:
        /// Throws std::invalid_argument for a file that is not a controller's profile. The choices
        /// kept are read; with none kept yet, those the app kept before the engine did
        /// (controller-profiles.json in its configuration folder) are taken over, once.
        explicit ControllerPorts(const std::vector<std::string>& files = {});
        void setInputs(std::vector<std::string> names); // the MIDI inputs now, by name, in the system's order
        std::vector<Control::MidiPort> list() const;
        // A port's profiles, and whether the musician ignores it; kept first: a change that cannot be
        // written is refused (std::runtime_error) and not made.
        std::vector<Control::MidiPort> choose(const std::string& input, const std::optional<std::vector<std::string>>& profiles);
        std::vector<Control::MidiPort> ignore(const std::string& input, bool ignored);
        // What the engine plays: the joined profiles' groups (in their order, by id), which input is
        // which profile's port, and the inputs left out (the musician's, a duplicate port, a Mackie
        // Control port the app reads).
        struct Applied { std::vector<ControlGroup> groups; std::vector<Control::ControllerPort> ports; std::vector<std::string> ignored; };
        Applied applied() const;
        uint64_t changes() const; // counts the inputs, a choice or the ignored inputs changing: status's midiPortList

    private:
        void write() const; // under the lock
        mutable std::mutex lock_;
        std::vector<standalone::Profile> profiles_;
        standalone::ProfileChoices choices_;
        std::vector<std::string> ignored_, inputs_;
        uint64_t changes_ = 0;
    };
}
