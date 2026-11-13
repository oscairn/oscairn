// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/item_types.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <memory>
#include <mutex>
#include <stdexcept>

namespace oscairn
{
    namespace
    {
        using V = const json_value_s*;
        [[noreturn]] void bad(const std::string& why) { throw std::invalid_argument("not a usable item type: " + why); }
        std::string text(V v, const char* what)
        {
            if(!v || v->type != json_type_string) bad(std::string("its ") + what + " is not text");
            const auto* s = static_cast<const json_string_s*>(v->payload);
            return {s->string, s->string_size};
        }
        std::string number(V v) { const auto* n = static_cast<const json_number_s*>(v->payload); return {n->number, n->number_size}; }
        bool lowerWord(std::string_view s) { return !s.empty() && std::all_of(s.begin(), s.end(), [](char c) { return (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9') || c == '-'; }); }
        // Reverse-DNS: at least two lowercase words joined by dots.
        bool reverseDns(std::string_view id)
        {
            if(id.size() > 128 || id.find('.') == std::string_view::npos) return false;
            for(size_t at = 0;;)
            {
                const auto end = id.find('.', at);
                if(!lowerWord(id.substr(at, end == std::string_view::npos ? std::string_view::npos : end - at))) return false;
                if(end == std::string_view::npos) return true;
                at = end + 1;
            }
        }
        // A folder in the data folder and in archives: a plain name, never one Oscairn keeps there for itself.
        bool plainFolder(std::string_view f)
        {
            static const char* const reserved[] = {"Plugins", "Licenses", "Libraries", "AssetLibraries", "Keys", "AppData", "library", "assets"};
            return !f.empty() && f.size() <= 64 && std::isalnum(static_cast<unsigned char>(f.front())) &&
                   std::all_of(f.begin(), f.end(), [](char c) { return std::isalnum(static_cast<unsigned char>(c)) || c == '-' || c == '_'; }) &&
                   std::none_of(std::begin(reserved), std::end(reserved), [&](const char* r) { return f == r; });
        }
        // Oscairn's own types (RFC 0023 step 1), as manifests like any other.
        const char* const builtIn[] = {
            R"({"format":"oscairn.type","version":1,"id":"oscairn.performance","name":"Performance","plural":"Performances",
                "recognize":{"folder":".oscairn","document":"performance.json","format":"oscairn.performance","version":13},
                "storage":"managed","folder":"Performances","identity":"id","fields":[]})",
            R"({"format":"oscairn.type","version":1,"id":"oscairn.sound","name":"Sound","plural":"Sounds",
                "recognize":{"folder":".osound","document":"sound.json","format":"oscairn.sound","version":8},
                "storage":"managed","folder":"Sounds","identity":"id","fields":[]})",
            R"({"format":"oscairn.type","version":1,"id":"oscairn.setlist","name":"Setlist","plural":"Setlists",
                "recognize":{"folder":".osetlist","document":"setlist.json","format":"oscairn.setlist","version":2},
                "storage":"managed","folder":"Setlists","identity":"id","fields":[]})",
            R"({"format":"oscairn.type","version":1,"id":"oscairn.sample","name":"Sample","plural":"Samples",
                "recognize":{"extensions":[]},"storage":"content","folder":"Assets","identity":"content","fields":[]})",
        };
        std::mutex typesMutex; // the types are read from the control thread and from package jobs
        std::vector<ItemType>& types()
        {
            static std::vector<ItemType> all = [] { std::vector<ItemType> out; for(const auto* m : builtIn) out.push_back(readItemType(m)); return out; }();
            return all;
        }
    }

    std::string ItemType::kind() const { return id.starts_with("oscairn.") ? id.substr(8) : id; }

    ItemType readItemType(std::string_view manifest)
    {
        if(manifest.size() > 64 * 1024 || !detail::jsonPreflight(manifest, 8).empty()) bad("its manifest is not valid JSON");
        const std::unique_ptr<json_value_s, decltype(&std::free)> doc{json_parse(manifest.data(), manifest.size()), &std::free};
        V d = doc.get();
        if(!d || text(detail::member(d, "format"), "format") != "oscairn.type") bad("its format is not oscairn.type");
        if(const V v = detail::member(d, "version"); !v || v->type != json_type_number || number(v) != "1") bad("its version is not 1");
        ItemType t;
        t.id = text(detail::member(d, "id"), "ID"); t.name = text(detail::member(d, "name"), "name"); t.plural = text(detail::member(d, "plural"), "plural");
        t.storage = text(detail::member(d, "storage"), "storage"); t.folder = text(detail::member(d, "folder"), "folder"); t.identity = text(detail::member(d, "identity"), "identity");
        if(!reverseDns(t.id)) bad("its ID is not reverse-DNS in lowercase: " + t.id);
        if(t.name.empty() || t.name.size() > 64 || t.plural.empty() || t.plural.size() > 64) bad("its name or plural is empty or too long");
        if(t.storage != "managed" && t.storage != "content" && t.storage != "in-place") bad("its storage is not managed, content or in-place");
        if(t.identity != "id" && t.identity != "content" && t.identity != "payload" && t.identity != "path") bad("its identity is not id, content, payload or path");
        if(!plainFolder(t.folder)) bad("its folder is not a plain name: " + t.folder);
        const V r = detail::member(d, "recognize");
        if(!r || r->type != json_type_object) bad("it says nothing of how its items are recognized");
        if(const V folder = detail::member(r, "folder"))
        {
            t.suffix = text(folder, "recognize.folder"); t.document = text(detail::member(r, "document"), "recognize.document"); t.format = text(detail::member(r, "format"), "recognize.format");
            if(t.suffix.size() < 2 || t.suffix.size() > 16 || t.suffix[0] != '.' || !lowerWord(std::string_view(t.suffix).substr(1))) bad("its folder suffix is not a dot and a lowercase word");
            if(t.document.size() > 64 || !t.document.ends_with(".json") || t.document[0] == '.' || t.document.find('/') != std::string::npos) bad("its document is not a .json file name");
            if(t.format.empty() || t.format.size() > 128) bad("its document format is empty or too long");
            if(const V v = detail::member(r, "version"))
            {
                if(v->type != json_type_number) bad("its document version is not a number");
                const auto n = std::strtoul(number(v).c_str(), nullptr, 10);
                if(n < 1 || n > 1000000) bad("its document version is out of range");
                t.version = uint32_t(n);
            }
        }
        if(const V list = detail::member(r, "extensions"))
        {
            if(list->type != json_type_array) bad("its extensions are not a list");
            for(auto* e = static_cast<const json_array_s*>(list->payload)->start; e; e = e->next)
            {
                const auto x = text(e->value, "extension");
                if(x.size() < 2 || x.size() > 16 || x[0] != '.' || !lowerWord(std::string_view(x).substr(1))) bad("an extension is not a dot and a lowercase word: " + x);
                t.extensions.push_back(x);
            }
        }
        // A managed type's items are folders with a document holding their ID; files are recognized by extension.
        if(t.storage == "managed" && (t.document.empty() || t.identity != "id")) bad("a managed type's items are folders holding a document, identified by ID");
        if(t.storage != "managed" && !t.document.empty()) bad("only a managed type's items are folders holding a document");
        // Step 4: one of Oscairn's extractors reads its files' fields (and a payload identity).
        if(const V extractor = detail::member(d, "extractor"))
        {
            t.extractor = text(extractor, "extractor");
            if(t.extractor != "audio") bad("its extractor is not one Oscairn has: " + t.extractor);
            if(t.storage != "in-place") bad("only an in-place type's files are read by an extractor, so far");
        }
        if(const V fields = detail::member(d, "fields"))
        {
            if(fields->type != json_type_array) bad("its fields are not a list");
            for(auto* e = static_cast<const json_array_s*>(fields->payload)->start; e; e = e->next)
            {
                ItemType::Field f{text(detail::member(e->value, "id"), "field's ID"), text(detail::member(e->value, "name"), "field's name")};
                if(const V facet = detail::member(e->value, "facet")) f.facet = facet->type == json_type_true;
                if(!lowerWord(f.id) || f.name.empty() || f.name.size() > 64 || t.fields.size() >= 32) bad("a field's ID or name is wrong");
                t.fields.push_back(std::move(f));
            }
        }
        return t;
    }

    std::vector<ItemType> itemTypes()
    {
        std::lock_guard lock(typesMutex);
        return types();
    }

    std::optional<ItemType> itemType(std::string_view idOrKind)
    {
        std::lock_guard lock(typesMutex);
        for(const auto& t : types()) if(t.id == idOrKind || t.kind() == idOrKind) return t;
        return std::nullopt;
    }

    void addItemType(const ItemType& type)
    {
        auto lower = [](std::string s) { for(auto& c : s) c = char(std::tolower(static_cast<unsigned char>(c))); return s; };
        std::lock_guard lock(typesMutex);
        for(const auto& t : types())
            if(t.id == type.id || t.kind() == type.kind() || lower(t.folder) == lower(type.folder)) bad("another type has its ID or folder: " + t.id);
        types().push_back(type);
    }

    void removeItemType(std::string_view id)
    {
        std::lock_guard lock(typesMutex);
        std::erase_if(types(), [&](const ItemType& t) { return t.id == id && !t.owner.empty(); });
    }

    ItemType readDeclaredType(std::string_view manifest, const std::string& owner)
    {
        auto t = readItemType(manifest);
        if(owner.empty() || !t.id.starts_with(owner + ".")) bad(t.id + " is not under its package's ID, " + owner);
        auto lower = [](std::string s) { for(auto& c : s) c = char(std::tolower(static_cast<unsigned char>(c))); return s; };
        std::lock_guard lock(typesMutex);
        for(const auto& b : types())
            if(b.owner.empty() && (lower(b.folder) == lower(t.folder) || b.id == t.id)) bad(t.id + " takes the folder or ID of Oscairn's own " + b.id);
        t.owner = owner;
        return t;
    }
}
