// SPDX-License-Identifier: AGPL-3.0-or-later
#include "AppManifest.h"
#include "Executable.h"
#include "oscairn/package.h"
#include <algorithm>
#include <cstdio>
#include <stdexcept>

namespace oscairn::standalone
{
    namespace
    {
        bool named(const juce::String& s)
        {
            return s.isNotEmpty() && s.length() <= 128 && s.containsOnly("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789.-_");
        }
    }

    AppManifest readAppManifest(const juce::File& folder)
    {
        const auto file = folder.getChildFile("app.json");
        if(!file.existsAsFile()) throw std::runtime_error("no app.json in " + folder.getFullPathName().toStdString());
        if(file.getSize() > 256 * 1024) throw std::runtime_error("app.json is too large");
        juce::var doc;
        if(juce::JSON::parse(file.loadFileAsString(), doc).failed() || !doc.isObject()) throw std::runtime_error("app.json is not a JSON object");
        if(doc["format"] != juce::var("oscairn.app") || !doc["version"].isInt() || int(doc["version"]) != 1) throw std::runtime_error("app.json is not a version 1 app manifest");
        AppManifest m;
        m.folder = folder;
        const auto id = doc["id"].toString(), name = doc["name"].toString();
        if(!doc["id"].isString() || !named(id)) throw std::runtime_error("an app's id is 1 to 128 letters, digits, dots, hyphens and underscores");
        if(!doc["name"].isString() || name.trim().isEmpty() || name.length() > 64) throw std::runtime_error("an app's name is 1 to 64 characters");
        m.id = id.toStdString(); m.name = name.toStdString(); m.version = doc["appVersion"].toString().toStdString();
        if(const auto* builds = doc["builds"].getDynamicObject())
            if(const auto path = builds->getProperty(juce::String(platform())).toString(); path.isNotEmpty())
            {
                // Within its folder only: no absolute paths, no way out.
                if(juce::File::isAbsolutePath(path) || path.contains("..")) throw std::runtime_error("an app's build is a path within its folder");
                m.executable = folder.getChildFile(path);
            }
        if(const auto* offers = doc["offers"].getArray())
            for(const auto& o : *offers)
            {
                if(!named(o["action"].toString())) throw std::runtime_error("an offer names its action");
                m.offers.push_back({o["action"].toString().toStdString(), o["name"].toString().toStdString()});
            }
        if(const auto* asks = doc["asks"].getArray())
            for(const auto& a : *asks) if(named(a.toString())) m.asks.push_back(a.toString().toStdString());
        if(const auto* permissions = doc["permissions"].getArray())
            for(const auto& p : *permissions) m.permissions.push_back(p.toString().toStdString()); // checked by the engine, which refuses a name it does not know
        const auto memory = doc["memory"];
        if(memory["reserve"].isInt()) m.reserve = uint64_t(std::max(0, int(memory["reserve"])));
        if(memory["max"].isInt()) m.maximum = uint64_t(std::max(0, int(memory["max"])));
        return m;
    }

    std::vector<AppManifest> findApps(const juce::File& folder)
    {
        std::vector<AppManifest> out;
        for(const auto& child : folder.findChildFiles(juce::File::findDirectories, false))
        {
            if(!child.getChildFile("app.json").existsAsFile()) continue;
            try { out.push_back(readAppManifest(child)); }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: the app in %s: %s\n", child.getFullPathName().toRawUTF8(), e.what()); }
        }
        std::sort(out.begin(), out.end(), [](const auto& a, const auto& b) { return a.id < b.id; });
        return out;
    }

    juce::File appsFolder()
    {
        const auto binary = executableFile().getParentDirectory();
        return binary.getChildFile( "Apps" ).isDirectory() ? binary.getChildFile( "Apps" ) : binary.getSiblingFile( "share" ).getChildFile( "oscairn/apps" );
    }
}
