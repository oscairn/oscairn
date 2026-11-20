// SPDX-License-Identifier: AGPL-3.0-or-later
// RFC 0046's contract in the SDK helper (voice-budget-plugin.hpp): an engine lowered by n gives up the n voices its last
// importance report listed first, though the order has moved since the list was made (the SDK owner's review, blocker
// 3). B released at -30 dB, 2 s old; A released at -40 dB, struck 48 ms ago: the list says [B, A] (A just struck goes
// after the others of its tier). A block later A is 51 ms old, no longer struck, and the order alone (victim()) would
// give up A, the less audible; Importance::next() gives up B, then A. A listed voice that ended, or whose slot a new
// voice took, before the drop is skipped, not mis-dropped; past the list, victim()'s order.
#include <oscairn-clap/voice-budget-plugin.hpp>
#include <array>
#include <cmath>
#include <cstdio>
#include <string>
using namespace oscairn::voices;
static int failures = 0;
static void check(bool ok, const std::string& why) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", why.c_str()); ++failures; } }

int main()
{
    const auto gain = [](double db) { return std::pow(10.0, db / 20); };
    Limits limits; // no reserve, no limit of its own: the drop is the caller's
    std::array<Candidate, 4> v{};
    std::array<bool, 4> live{};
    auto get = [&](size_t i) -> const Candidate* { return live[i] ? &v[i] : nullptr; };
    // Slot 1 holds B, slot 3 holds A; serials (age) 7 and 9.
    v[1] = {0, true, false, false, false, gain(-30), 7, false, 2000};
    v[3] = {0, true, false, false, false, gain(-40), 9, false, 48};
    live[1] = live[3] = true;
    Importance<1> report;
    report.list(v.size(), get, limits);
    oscairn_voice_budget_importance_t out{};
    check(report.write(&out, 1) == 1 && out.count == 2, "one context, two listed");
    check(out.least[0].level_db > -31 && out.least[1].level_db < -39 && (out.least[1].flags & OSCAIRN_VOICE_BUDGET_STRUCK), "listed [B, A], A struck");
    // A block (3 ms) on: A is no longer struck.
    v[1].ageMs += 3; v[3].ageMs += 3;
    check(victim(v.size(), get, limits, none) == 3, "the order alone now gives up A (the reviewer's case)");
    const int first = report.next(v.size(), get, limits);
    check(first == 1, "next() gives up B, listed first: " + std::to_string(first));
    v[1].ending = true; // fading out
    check(report.next(v.size(), get, limits) == 3, "then A");
    v[3].ending = true;
    check(report.next(v.size(), get, limits) == -1, "then nothing: every voice fading");

    // A listed voice that ended before the drop is skipped; one whose slot a new voice took is too.
    v[1] = {0, true, false, false, false, gain(-30), 7, false, 2000};
    v[3] = {0, true, false, false, false, gain(-40), 9, false, 48};
    v[0] = {0, true, true, false, false, gain(-6), 5, false, 3000}; // a held key, listed last
    live = {true, true, false, true};
    report.list(v.size(), get, limits);
    live[1] = false; // B ended
    check(report.next(v.size(), get, limits) == 3, "B ended: A next");
    v[3] = {0, true, true, false, false, gain(-50), 12, false, 0}; // A's slot now plays a new note
    check(report.next(v.size(), get, limits) == 0, "A replaced: the held key, listed third");
    v[0].ending = true;
    check(report.next(v.size(), get, limits) == 3, "past the list: victim()'s, the new note");
    if(!failures) std::printf("ok: the helper gives up what it listed, in its listed order\n");
    return failures ? 1 : 0;
}
