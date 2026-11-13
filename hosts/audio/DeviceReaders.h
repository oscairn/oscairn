// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// A device's own panel and keys that do not speak MIDI (a profile's `panel` and `keyboard`, RFC
// 0024), read by that device's code in devices/, which defines these functions; an image whose
// profile has none links NoDeviceReaders.cpp instead, so the engine names no device here.

#include "oscairn/physical_controls.h" // ControlInput

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
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
    /// A pad pressed or released (an index into the Performance's pad map, from 0) goes to `pad`
    /// on the same thread.
    using PlayControl = std::function<void( const std::string& control, const ControlInput& )>;
    using PlayPad = std::function<void( uint32_t pad, bool on )>;
    std::unique_ptr<DeviceReader> openPanel( const std::string& device, const std::string& map, PlayControl play, PlayPad pad );

    /// The keyboard's MIDI inputs, in order: each one's name and the key its source number is kept
    /// by. Empty: this build reads no keyboard.
    struct KeyboardInput { std::string name, key; };
    std::vector<KeyboardInput> keyboardInputs();
    /// The keyboard at `device`, each MIDI message handed over by input (an index into
    /// keyboardInputs()) on the reader's thread; nullptr: this build reads no keyboard. With
    /// `localOff`, the instrument's own sound engine is told Local Switch off once, when the
    /// device is first opened.
    using PlayMidi = std::function<void( size_t input, const uint8_t* bytes, size_t size )>;
    std::unique_ptr<DeviceReader> openKeyboard( const std::string& device, PlayMidi play, bool localOff );

    /// Keys of the Linux input device named `name` (a profile's `inputKeys`: on the FANTOM-0,
    /// gpio-keys' WRITE, MENU and TEMPO), read-only, each code in `keys` played by its function
    /// name as a switch (1 pressed, 0 released) on the reader's thread, as the panel's switches
    /// are. nullptr: this build reads none.
    std::unique_ptr<DeviceReader> openInputKeys( const std::string& name, const std::map<uint16_t, std::string>& keys, PlayControl play );
}
