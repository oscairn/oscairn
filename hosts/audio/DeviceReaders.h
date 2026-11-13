// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// A device's own panel and keys that do not speak MIDI (a profile's `panel` and `keyboard`, RFC
// 0024), read by that device's code in devices/, which defines these functions; an image whose
// profile has none links NoDeviceReaders.cpp instead, so the engine names no device here.

#include "oscairn/physical_controls.h" // ControlInput

#include <cstddef>
#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <vector>

namespace oscairn::host
{
    /// A reader on a thread of its own; destroying it stops it.
    struct DeviceReader { virtual ~DeviceReader() = default; };

    /// The panel at `device`, its messages decoded through the map file `map`, each control it
    /// names played by identity on the reader's thread. Throws when the map is unreadable or
    /// refused; nullptr: this build reads no panel.
    using PlayControl = std::function<void( const std::string& control, const ControlInput& )>;
    std::unique_ptr<DeviceReader> openPanel( const std::string& device, const std::string& map, PlayControl play );

    /// The keyboard's MIDI inputs, in order: each one's name and the key its source number is kept
    /// by. Empty: this build reads no keyboard.
    struct KeyboardInput { std::string name, key; };
    std::vector<KeyboardInput> keyboardInputs();
    /// The keyboard at `device`, each MIDI message handed over by input (an index into
    /// keyboardInputs()) on the reader's thread; nullptr: this build reads no keyboard.
    using PlayMidi = std::function<void( size_t input, const uint8_t* bytes, size_t size )>;
    std::unique_ptr<DeviceReader> openKeyboard( const std::string& device, PlayMidi play );
}
