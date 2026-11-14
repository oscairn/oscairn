// SPDX-License-Identifier: AGPL-3.0-or-later
#include "SessionSetup.h"
#include "AppManifest.h"
#include <algorithm>
#include <cstdio>

namespace oscairn::standalone
{
    namespace
    {
        juce::File captureFile(const juce::File& configuration) { return configuration.getChildFile("capture"); }
    }

    void setUpSession(Control& c, const Profile& profile, uint64_t shell)
    {
        // RFC 0025: with the profile's memory classes, the device's memory less what is reserved.
        Control::Budget budget{ profile.cpuBudget(), profile.memoryBudget() };
        budget.available = uint64_t( profile.compute.availableMemory ) << 20; // RFC 0045: instances kept warm give it back
        std::vector<Control::AppMemory> always;
        if( profile.memoryClasses() )
        {
            constexpr uint64_t MiB = 1ull << 20;
            budget.device = uint64_t( profile.compute.memory ) * MiB;
            budget.system = uint64_t( profile.compute.osMemory ) * MiB;
            budget.floor = uint64_t( profile.compute.performanceMemory ) * MiB;
            budget.margin = profile.memoryMargin();
            // An app that is a process, at its manifest's reservation; Oscairn's in-process apps at their placeholders.
            const auto manifests = findApps( appsFolder() );
            for( const auto& a : profile.alwaysAvailable )
            {
                const auto m = std::find_if( manifests.begin(), manifests.end(), [&]( const AppManifest& x ) { return x.id == a; } );
                always.push_back( { a, ( m != manifests.end() ? m->reserve : uint64_t( Profile::appMemory( a ) ) ) * MiB } );
            }
            if( budget.system + shell + budget.floor + budget.margin > budget.device )
            {
                std::fprintf( stderr, "oscairn: memory classes off: the OS, the UI, the floor and the margin exceed the device's memory\n" );
                budget.device = budget.system = budget.floor = budget.margin = 0; always.clear();
            }
        }
        c.setBudget( budget );
        if( shell ) c.setShellMemory( shell );
        // RFC 0031: the package repositories its image ships, and whether it hides the official one.
        std::vector<Control::Repository> shipped;
        for( const auto& r : profile.repositories ) shipped.push_back( { r.address, r.name, r.key, "device" } );
        c.setDeviceRepositories( shipped, profile.hideOfficial );
        if( !always.empty() ) c.setAlwaysAvailable( always ); // last: refused when they do not fit (std::invalid_argument)
    }

    double readCaptureSeconds(const juce::File& configuration, const Profile& profile)
    {
        const auto saved = captureFile(configuration).loadFileAsString().trim();
        if(saved.isNotEmpty() && saved.containsOnly("0123456789.")) return std::clamp(saved.getDoubleValue(), 0.0, 3600.0);
        if(profile.compute.captureSeconds >= 0) return profile.compute.captureSeconds; // the device's own
        return profile.compute.memory > 0 && profile.compute.memory <= 2048 ? 120 : 600; // RFC 0020's proposed defaults
    }

    void writeCaptureSeconds(const juce::File& configuration, double seconds)
    {
        configuration.createDirectory();
        captureFile(configuration).replaceWithText(juce::String(seconds, 0) + "\n");
    }

    juce::File configurationFolder()
    {
        if(const auto env = juce::SystemStats::getEnvironmentVariable("OSCAIRN_CONFIG_HOME", {}); env.isNotEmpty()) return juce::File(env);
#if JUCE_MAC
        return juce::File::getSpecialLocation(juce::File::userApplicationDataDirectory).getChildFile("Application Support/Oscairn");
#elif JUCE_LINUX
        const auto xdg = juce::SystemStats::getEnvironmentVariable("XDG_CONFIG_HOME", {});
        return (xdg.isNotEmpty() ? juce::File(xdg) : juce::File::getSpecialLocation(juce::File::userHomeDirectory).getChildFile(".config")).getChildFile("Oscairn");
#else
        return juce::File::getSpecialLocation(juce::File::userApplicationDataDirectory).getChildFile("Oscairn");
#endif
    }
}
