// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Settings' pages, in the order the list shows them: each by its id, its title, where it applies
// and how it is made (README.md beside this says how to add one).
#include "AudioPage.h"
#include "MidiControllersPage.h"
#include "RepositoriesPage.h"
#include "SigningKeysPage.h"

namespace oscairn::standalone::settings
{
    namespace
    {
        // Where a page applies, from the device's profile: a device's own image (Oscairn OS) has a
        // runtime; the desktop app on a computer has none (Profile::desktop).
        bool onDevice(const Profile& p) { return !p.platform.runtime.empty(); }
        [[maybe_unused]] bool onDesktop(const Profile& p) { return p.platform.runtime.empty(); }
        template<class T> std::function<std::unique_ptr<Page>(Engine&)> made() { return [](Engine& e) { return std::make_unique<T>(e); }; }
        constexpr std::nullptr_t everywhere = nullptr, notBuilt = nullptr;
    }

    const std::vector<PageInfo>& pages()
    {
        static const std::vector<PageInfo> all{
            {"audio", "Audio", everywhere, made<AudioPage>()},
            {"repositories", "Package repositories", everywhere, made<RepositoriesPage>()},   // RFC 0031
            {"keys", "Signing keys", everywhere, made<SigningKeysPage>()},                   // RFC 0006
            {"controllers", "MIDI controllers", everywhere, made<MidiControllersPage>()},  // RFC 0024
            // Named by RFCs, not built yet.
            {"appearance", "Appearance", everywhere, notBuilt},                             // themes (design-language.md)
            {"content", "Installed content", everywhere, notBuilt},                         // RFC 0006
            {"storage", "Storage", everywhere, notBuilt},                                   // RFC 0037
            {"network", "Network", [](const Profile& p) { return onDevice(p) && (p.network.ethernet || p.network.wifi); }, notBuilt}, // RFC 0026, RFC 0039
#if OSCAIRN_BLUETOOTH // left out where it is not built (RFC 0036)
            {"bluetooth", "Bluetooth", onDevice, notBuilt},                                 // RFC 0036
#endif
            {"time", "Date and time", onDevice, notBuilt},                                  // RFC 0035
            {"about", "About", everywhere, notBuilt},                                       // RFC 0038, its licenses RFC 0035
        };
        return all;
    }
}
