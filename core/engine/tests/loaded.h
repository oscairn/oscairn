// SPDX-License-Identifier: AGPL-3.0-or-later
// A test's plugin, loaded as the engine loads one (oscairn::loadPlugin: the CLAP host in this process), by the
// descriptor a scan gave; null when it does not load, the reason on stderr.
#pragma once
#include <kick_clap/Host.h>
#include <oscairn/plugin_host.h>
#include <cstdio>
#include <memory>

inline std::shared_ptr<oscairn::PluginHost> loaded(const kick::clap::Descriptor& plugin, double rate, uint32_t frames)
{
    try { return oscairn::loadPlugin(plugin.path, plugin.id, rate, frames); }
    catch(const std::exception& e) { std::fprintf(stderr, "cannot load %s: %s\n", plugin.id.c_str(), e.what()); return nullptr; }
}
