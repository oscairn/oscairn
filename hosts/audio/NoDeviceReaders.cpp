// SPDX-License-Identifier: AGPL-3.0-or-later
//
// No device panel or keys built in (DeviceReaders.h).
#include "DeviceReaders.h"

namespace oscairn::host
{
    std::unique_ptr<DeviceReader> openPanel( const std::string&, const std::string&, PlayControl ) { return nullptr; }
    std::vector<KeyboardInput> keyboardInputs() { return {}; }
    std::unique_ptr<DeviceReader> openKeyboard( const std::string&, PlayMidi ) { return nullptr; }
}
