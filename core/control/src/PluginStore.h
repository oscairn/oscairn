// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
// The plugin store (RFC 0006), as performance.cpp keeps it, for the other parts of the Session
// that read it (versions.cpp, RFC 0043).
#include "oscairn/control.h"
#include "oscairn/signing.h"
#include <filesystem>
#include <map>

namespace oscairn::store
{
    // Each platform's build of a plugin version, by platform; this platform's, or empty.
    std::map<std::string, std::filesystem::path> builds(const std::string& format, const std::string& id, const std::string& version);
    std::filesystem::path build(const std::string& format, const std::string& id, const std::string& version);
    // Its plugin.json: name, license and the publisher's claim; an empty ID when it is not there.
    Control::InstalledPlugin identity(const std::string& format, const std::string& id, const std::string& version);
    // The signatures beside a build (<build>.sig) that verify over it.
    std::vector<signing::Signature> signatures(const std::filesystem::path& build, const std::string& format, const std::string& id, const std::string& version, const std::string& platform);
}
