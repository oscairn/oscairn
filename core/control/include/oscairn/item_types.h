// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace oscairn
{
    /// An item type (RFC 0023): what a kind of item is, read from its manifest, so the library,
    /// archives and pickers handle every type the same way instead of naming kinds. Oscairn's own
    /// (Performance, Sound, setlist and sample) are manifests built into the app.
    struct ItemType
    {
        std::string id, name, plural;
        // recognize: a folder with this suffix holding this document of this format, read at this
        // document version (0: any); or, for files, one of these extensions.
        std::string suffix, document, format;
        uint32_t version = 0;
        std::vector<std::string> extensions;
        std::string storage;  // "managed", "content" or "in-place"
        std::string folder;   // under the data folder for a managed type; the type's folder in archives
        std::string identity; // "id", "content", "payload" or "path"
        struct Field { std::string id, name; bool facet = false; };
        std::vector<Field> fields;
        // What library items, archive manifests and actions (oscairn.pick.<kind>) call it: its ID,
        // without "oscairn." for Oscairn's own ("performance").
        std::string kind() const;
        // An item kept as a folder holding a document with an info block, which the library indexes.
        bool indexed() const { return storage == "managed" && !document.empty(); }
    };

    // A type's manifest, checked (RFC 0023's rules); throws std::invalid_argument on a bad one.
    ItemType readItemType(std::string_view manifest);
    // The types this device knows: the built-in ones, then those added, in that order. Safe from
    // any thread: each call returns a copy.
    std::vector<ItemType> itemTypes();
    // A type by its ID or its kind; none for one not known.
    std::optional<ItemType> itemType(std::string_view idOrKind);
    // Adds a type; refuses (std::invalid_argument) one whose ID, kind or folder another type has.
    void addItemType(const ItemType& type);
}
