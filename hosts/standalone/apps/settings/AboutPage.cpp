// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AboutPage.h"
#include <memory>

namespace oscairn::standalone::settings
{
    AboutPage::AboutPage(Engine& engine)
    {
        const juce::String commit(OSCAIRN_SOURCE_COMMIT);
        version.setText(OSCAIRN_RELEASE, juce::dontSendNotification);
        source.setText(commit.isEmpty() ? juce::String("Development build") : commit.substring(0, 12), juce::dontSendNotification);
        engineVersion.setText("Reading...", juce::dontSendNotification);
        license.setText("AGPL-3.0-or-later", juce::dontSendNotification);
        addRow("Oscairn", version);
        addRow("Source", source, commit.isEmpty() ? juce::String("Built from a working copy, not a release's commit.") : "https://github.com/oscairn/oscairn/tree/" + commit);
        auto& engineRow = addRow("Engine", engineVersion, "The audio engine this app is connected to.");
        addRow("License", license, "Free software under the GNU Affero General Public License, version 3 or later, with no warranty. "
               "Its text and the notices of what is built into Oscairn are in its Licenses folder (on macOS, Oscairn.app/Contents/Resources/Licenses, and on the disk image)"
               + juce::String(commit.isEmpty() ? "; in a development build, LICENSE and CREDITS.md in the source." : "."));
        auto release = std::make_shared<std::string>();
        engine.job([release](Control& c) { *release = c.preinstalled().release; },
            [safe = juce::Component::SafePointer<AboutPage>(this), release, &engineRow](const std::string& error) {
                if(!safe) return;
                safe->read_ = true;
                safe->engineVersion.setText(error.empty() ? juce::String::fromUTF8(release->c_str()) : juce::String("Unknown"), juce::dontSendNotification);
                if(!error.empty()) { engineRow.setExplanation("The engine's version could not be read: " + juce::String::fromUTF8(error.c_str()), true); safe->heightChanged(); }
            });
    }
}
