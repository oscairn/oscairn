// SPDX-License-Identifier: AGPL-3.0-or-later
#include "oscairn/setlist.h"
#include "sheredom/json.h"
#include "JsonInput.h"
#include "oscairn/library.h"
#include <algorithm>
#include <cstdlib>
#include <initializer_list>
#include <memory>
#include <set>
#include <stdexcept>

namespace oscairn
{
    namespace
    {
        using V = const json_value_s*;
        constexpr size_t maxDocument = 1024 * 1024, maxPages = 100, maxSlots = 1000, maxName = 256;
        [[noreturn]] void bad(const std::string& why) { throw std::runtime_error("not a usable setlist: " + why); }
        using Doc = std::unique_ptr<json_value_s, decltype(&std::free)>;
        Doc parse(std::string_view json)
        {
            if(json.size() > maxDocument) bad("it is larger than 1 MiB");
            if(!detail::jsonPreflight(json, 8).empty()) bad("it is not valid JSON");
            Doc doc{json_parse(json.data(), json.size()), &std::free};
            if(!doc || doc->type != json_type_object) bad("it is not a JSON object");
            return doc;
        }
        std::string text(V v, const char* what)
        {
            if(!v || v->type != json_type_string) bad(std::string(what) + " is not text");
            const auto* s = static_cast<const json_string_s*>(v->payload);
            return {s->string, s->string_size};
        }
        uint32_t side(V v, const char* what)
        {
            if(!v || v->type != json_type_number) bad(std::string(what) + " is not a number");
            const std::string n(static_cast<const json_number_s*>(v->payload)->number, static_cast<const json_number_s*>(v->payload)->number_size);
            if(n.empty() || n.size() > 1 || n[0] < '1' || n[0] > '8') bad(std::string(what) + " is not 1 to 8");
            return uint32_t(n[0] - '0');
        }
        // An object with these members and no others, as every reader of Oscairn's documents refuses one it does not know.
        V only(V v, const char* what, std::initializer_list<std::string_view> keys)
        {
            if(!v || v->type != json_type_object) bad(std::string(what) + " is not an object");
            for(auto* e = static_cast<const json_object_s*>(v->payload)->start; e; e = e->next)
                if(std::find(keys.begin(), keys.end(), std::string_view(e->name->string, e->name->string_size)) == keys.end())
                    bad(std::string(what) + " has a member it does not take: " + std::string(e->name->string, e->name->string_size));
            for(const auto key : keys) if(!detail::member(v, key)) bad(std::string(what) + " has no " + std::string(key));
            return v;
        }
        const json_array_element_s* elements(V v, const char* what)
        {
            if(!v || v->type != json_type_array) bad(std::string(what) + " is not a list");
            return static_cast<const json_array_s*>(v->payload)->start;
        }
        const char* const modes[] = {"list", "scroll", "grid"};
        std::string slotJson(const Control::SetlistSlot& s)
        {
            if(s.empty) return "null";
            return "{\"id\":" + detail::quote(s.id) + ",\"name\":" + detail::quote(s.name) + ",\"color\":" + detail::quote(s.color) + ",\"label\":" + detail::quote(s.label) + '}';
        }
        // The document around its info block: what goes before it and after it.
        std::pair<std::string, std::string> around(const Control::Setlist& s)
        {
            std::string pages;
            for(const auto& p : s.pages)
            {
                std::string slots;
                for(const auto& slot : p.slots) slots += (slots.empty() ? "" : ",") + slotJson(slot);
                pages += std::string(pages.empty() ? "" : ",") + "{\"name\":" + detail::quote(p.name) + ",\"slots\":[" + slots + "]}";
            }
            return {"{\"format\":\"oscairn.setlist\",\"version\":2,\"info\":",
                    ",\"display\":{\"mode\":" + detail::quote(s.mode) + ",\"columns\":" + std::to_string(s.columns) + ",\"rows\":" + std::to_string(s.rows) + "},\"pages\":[" + pages + "]}"};
        }
    }

    Control::Setlist readSetlist(std::string_view json)
    {
        const auto doc = parse(json);
        V d = only(doc.get(), "the setlist", {"format", "version", "info", "display", "pages"});
        Control::Setlist s;
        try { s.info = readInfo(json, "oscairn.setlist", false); } catch(const std::runtime_error& e) { bad(std::string("its info: ") + e.what()); }
        V display = only(detail::member(d, "display"), "its display", {"mode", "columns", "rows"});
        s.mode = text(detail::member(display, "mode"), "its display's mode");
        if(std::find(std::begin(modes), std::end(modes), s.mode) == std::end(modes)) bad("its display's mode is not list, scroll or grid");
        s.columns = side(detail::member(display, "columns"), "its grid's columns"); s.rows = side(detail::member(display, "rows"), "its grid's rows");
        size_t slots = 0;
        for(auto* p = elements(detail::member(d, "pages"), "its pages"); p; p = p->next)
        {
            V page = only(p->value, "a page", {"name", "slots"});
            Control::SetlistPage out{text(detail::member(page, "name"), "a page's name"), {}};
            if(out.name.size() > maxName) bad("a page's name is longer than 256 bytes");
            for(auto* e = elements(detail::member(page, "slots"), "a page's slots"); e; e = e->next)
            {
                if(++slots > maxSlots) bad("it has more than 1000 slots");
                if(e->value->type == json_type_null) { out.slots.push_back({{}, {}, {}, {}, true}); continue; }
                V slot = only(e->value, "a slot", {"id", "name", "color", "label"});
                Control::SetlistSlot x{text(detail::member(slot, "id"), "a slot's ID"), text(detail::member(slot, "name"), "a slot's name"),
                                       text(detail::member(slot, "color"), "a slot's color"), text(detail::member(slot, "label"), "a slot's label")};
                if(x.id.size() > 64) bad("a slot's ID is longer than 64 bytes");
                if(x.name.size() > maxName || x.label.size() > maxName) bad("a slot's name or label is longer than 256 bytes");
                if(!validColor(x.color)) bad("a slot's color is not #rrggbb or empty");
                // A placeholder: no ID, a name only, for a Performance a converter could not supply.
                if(x.id.empty() && x.name.empty()) bad("a slot has neither an ID nor a name; an empty slot is null");
                out.slots.push_back(std::move(x));
            }
            s.pages.push_back(std::move(out));
            if(s.pages.size() > maxPages) bad("it has more than 100 pages");
        }
        if(s.pages.empty()) bad("it has no pages");
        return s;
    }

    std::string setlistJson(const Control::Setlist& s)
    {
        const auto [head, tail] = around(s);
        return head + infoJson(s.info) + tail;
    }

    Control::PerformanceInfo writeSetlist(const std::filesystem::path& folder, Control::Setlist s)
    {
        readSetlist(setlistJson(s)); // what is written reads back
        const auto [head, tail] = around(s);
        s.info.hash = contentHash(head + tail + "\n", folder / "nodes");
        writeAtomically(folder / "setlist.json", head + infoJson(s.info) + tail + "\n");
        return s.info;
    }

    Control::Setlist setlistFromVersion1(std::string_view json)
    {
        const auto doc = parse(json);
        V d = doc.get();
        if(text(detail::member(d, "format"), "its format") != "oscairn.setlist") bad("its format is not oscairn.setlist");
        if(const V v = detail::member(d, "version"); !v || v->type != json_type_number || std::string_view(static_cast<const json_number_s*>(v->payload)->number, static_cast<const json_number_s*>(v->payload)->number_size) != "1")
            bad("its version is not 1");
        Control::Setlist s;
        s.info.name = text(detail::member(d, "name"), "its name").substr(0, maxName);
        V display = detail::member(d, "display");
        s.mode = text(detail::member(display, "mode"), "its display's mode");
        if(std::find(std::begin(modes), std::end(modes), s.mode) == std::end(modes)) bad("its display's mode is not list, scroll or grid");
        s.columns = side(detail::member(display, "columns"), "its grid's columns"); s.rows = side(detail::member(display, "rows"), "its grid's rows");
        std::vector<Control::SetlistSlot> entries;
        for(auto* e = elements(detail::member(d, "entries"), "its entries"); e && entries.size() < maxSlots; e = e->next)
        {
            Control::SetlistSlot slot{text(detail::member(e->value, "id"), "an entry's ID"), text(detail::member(e->value, "name"), "an entry's name"), text(detail::member(e->value, "color"), "an entry's color"), {}};
            if(slot.id.empty() || slot.id.size() > 64 || !validColor(slot.color)) continue; // version 1 refused these; none is lost that it read
            slot.name = slot.name.substr(0, maxName);
            entries.push_back(std::move(slot));
        }
        // A grid showed a screen of columns × rows at a time: each screen becomes a page, so every
        // entry keeps its place (screens put together where there would be more than 100 pages).
        const size_t perPage = s.mode == "grid" ? std::max(size_t(s.columns * s.rows), (entries.size() + maxPages - 1) / maxPages) : std::max<size_t>(entries.size(), 1);
        for(size_t i = 0; i < entries.size(); ++i)
        {
            if(i % perPage == 0) s.pages.push_back({"Page " + std::to_string(s.pages.size() + 1), {}});
            s.pages.back().slots.push_back(std::move(entries[i]));
        }
        if(s.pages.empty()) s.pages.push_back({"Page 1", {}});
        return s;
    }

    std::vector<std::string> setlistPerformances(const Control::Setlist& s)
    {
        std::vector<std::string> out;
        std::set<std::string> seen;
        for(const auto& p : s.pages) for(const auto& slot : p.slots) if(!slot.empty && !slot.id.empty() && seen.insert(slot.id).second) out.push_back(slot.id);
        return out;
    }
}
