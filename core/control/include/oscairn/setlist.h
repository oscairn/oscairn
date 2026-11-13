// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "oscairn/control.h"
#include <filesystem>
#include <string>
#include <string_view>

namespace oscairn
{
    /// Setlists (docs/setlist-format.md): an item type of the library (RFC 0023), each a folder
    /// `<name>.osetlist` in the data folder's Setlists/ holding setlist.json, version 2.

    // A setlist document, checked; throws std::invalid_argument on one that is not version 2 or
    // breaks a rule of the format.
    Control::Setlist readSetlist(std::string_view json);
    // The document, written with its info block, which goes where writeInfo and contentHash find it.
    std::string setlistJson(const Control::Setlist& setlist);
    // Writes setlist.json into `folder`, made if needed, its info's hash made from what is written.
    // Returns the info as written.
    Control::PerformanceInfo writeSetlist(const std::filesystem::path& folder, Control::Setlist setlist);
    // A version 1 setlist (the Setlist app's own file before setlists were library items) as a
    // version 2 one with no ID: its entries as one page, or a page per screen of a grid.
    Control::Setlist setlistFromVersion1(std::string_view json);
    // The Performance IDs a setlist's slots name, each once, in order; placeholders and empty slots name none.
    std::vector<std::string> setlistPerformances(const Control::Setlist& setlist);
}
