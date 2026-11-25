// SPDX-License-Identifier: AGPL-3.0-or-later
//
// RFC 0011's instant controls: the controller-mapping service on the engine's MIDI input threads.
#include "oscairn/controller_input.h"
#include "oscairn/session.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <map>

namespace oscairn
{
    namespace
    {
        constexpr int sendKey = 100; // a send's level among the values, beside the assignments' kinds: {sendKey, 0, bus, part}

        // A control's 0..1 as a 14-bit bend, 8192 at the middle (0.5), 0 and 16383 at the ends.
        int bendOf(double v) { return std::clamp(int(std::lround(std::clamp(v, 0.0, 1.0) * 16383)), 0, 16383); }

        // A sprung input's position with a dead zone of 1% of its throw (to either side of its rest) about the rest
        // position, the rest scaled to reach both ends: a panel's lever reads 512 of 1023 at rest, not 511.5, and its spring
        // never lands on one value, so what the spring returns to is exactly the rest.
        double conditioned(double x, double rest)
        {
            x = std::clamp(x, 0.0, 1.0);
            const double span = x >= rest ? 1 - rest : rest, zone = .01 * span, distance = std::abs(x - rest);
            if(span <= 0 || distance <= zone) return rest;
            return rest + std::copysign((distance - zone) / (span - zone) * span, x - rest);
        }
    }

    void ControllerInput::set(std::vector<ControlGroup> groups, std::vector<Control::ControllerPort> ports)
    {
        std::lock_guard lock(lock_);
        groups_ = std::move(groups); ports_ = std::move(ports);
        positions_.assign(groups_.size(), 0);
        links_.clear(); holding_.clear();
        for(const auto& r : resolveControls(AssignmentSource{}, groups_, 0)) if(!r.follows.empty() || !std::isnan(r.rest)) links_[r.control] = {r.follows, r.rest};
        mapping_ = {};
        dirty_ = true;
    }

    void ControllerInput::setUnlinked(std::set<std::string> unlinked)
    {
        std::lock_guard lock(lock_);
        unlinked_ = std::move(unlinked);
        holding_.clear();
    }

    std::map<std::string, std::string> ControllerInput::functions() const
    {
        std::lock_guard lock(lock_);
        std::map<std::string, std::string> out;
        for(const auto& r : resolveControls(AssignmentSource{}, groups_, 0)) if(!r.function.empty()) out[r.control] = r.function;
        return out;
    }

    void ControllerInput::view(const Control::ControlView& view)
    {
        std::lock_guard lock(lock_);
        if(view.part) part_ = *view.part;
        if(view.screen) { screen_ = *view.screen; source_.screen = screen_; }
        if(view.lists) lists_ = *view.lists;
        for(const auto& [group, position] : view.positions)
        {
            if(group >= groups_.size()) throw std::invalid_argument("no such control group");
            if(positions_.size() <= group) positions_.resize(group + 1);
            positions_[group] = position;
        }
    }

    Control::ControllerState ControllerInput::state() const
    {
        std::lock_guard lock(lock_);
        return {positions_, touched_, touches_, navigation_};
    }

    void ControllerInput::midi(uint8_t source, const uint8_t* bytes, size_t size) noexcept
    {
        std::function<void()> wake;
        try
        {
            std::lock_guard lock(lock_);
            const auto bound = bindings_.find(source);
            if(bound == bindings_.end()) return;
            const auto moves = moves_;
            for(const auto& [device, port] : bound->second)
                for(const auto& action : mapping_.handle(source_, groups_, part_, positions_, device, port, bytes, size, [this](const Assignment& a) { return value(a); }))
                    play(action);
            if(moves_ != moves) wake = wake_;
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        if(wake) wake();
    }

    // A panel's control by identity. An input with a rest position (sprung) is conditioned about it. An extra input that
    // follows a logical control (a lever's axis, `follows`) while the Performance leaves it linked drives that control, with the
    // control's own input (a wheel) when it has one: the one holding the value is the latest to move, except that a sprung input
    // takes over only when its position reaches or crosses the value, measured from its rest (so it does not drag back a value
    // another input was left at), and then follows it, back to its rest when released; an input that stays where it is left takes
    // over at once. A control alone is played as it came.
    bool ControllerInput::control(const std::string& control, ControlInput input) noexcept
    {
        std::string logical = control;
        bool declared = false; // it follows a control or has a rest position: a lever or wheel
        try
        {
            std::lock_guard lock(lock_);
            const auto own = links_.find(control);
            declared = own != links_.end();
            const bool linked = own != links_.end() && !own->second.follows.empty() && !unlinked_.count(control);
            if(linked) logical = own->second.follows;
            const auto rest = own != links_.end() ? own->second.rest : std::numeric_limits<double>::quiet_NaN();
            if(input.kind == ControlInput::Kind::Absolute && !std::isnan(rest)) input.value = conditioned(input.value, rest);
            const bool shared = linked || std::any_of(links_.begin(), links_.end(), [&](const auto& l) { return l.second.follows == control && !unlinked_.count(l.first); });
            if(shared && input.kind == ControlInput::Kind::Absolute)
            {
                auto& held = holding_[logical];
                bool takes = std::isnan(held.value) || held.holder == control || std::isnan(rest);
                if(!takes)
                {
                    const double v = held.value;
                    takes = std::abs(v - rest) < 1e-9 || (v >= rest ? input.value >= v : input.value <= v);
                }
                if(!takes) { touched_ = control; return false; }
                held.value = input.value; held.holder = control;
            }
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        const bool played = this->played(logical, input, control);
        if(!played && declared)
        {
            // A lever or wheel that plays nothing is still the control moved last, for learn and for a client to see which one it is.
            std::lock_guard lock(lock_);
            touched_ = control;
        }
        return played;
    }

    bool ControllerInput::played(const std::string& control, ControlInput input, const std::string& physical) noexcept
    {
        std::function<void()> wake;
        bool played = false;
        int controller = -1;
        std::string function;
        bool mapped = false; // an assignment played it (and counted it)
        try
        {
            std::lock_guard lock(lock_);
            const auto moves = moves_;
            for(const auto& action : mapping_.handle(source_, groups_, part_, positions_, control, input, [this](const Assignment& a) { return value(a); }))
                { play(action); played = mapped = true; }
            if(moves_ != moves) wake = wake_;
            // A panel control (a pedal, footswitch, wheel, strip...) with a standard controller's function (a
            // FANTOM's HOLD jack: sustain; its lever: pitchBend, modulation) sends that controller to the parts,
            // from the built-in source, as a MIDI controller's own message reaches them, unless the Performance
            // (or a learn) has reassigned the control itself. A Sound's own assignment of the control (its
            // `parts`: the mod wheel to a filter) drives its target as well, as on a hardware instrument.
            {
                static const std::map<std::string, int> numbers{{"modulation", 1}, {"breath", 2}, {"foot", 4}, {"expression", 11}, {"sustain", 64}, {"portamento", 65}, {"sostenuto", 66}, {"soft", 67}};
                for(const auto& r : resolveControls(source_, groups_, part_, positions_))
                    if(r.control == control && (r.target.kind == Assignment::Kind::None || r.passes))
                        {
                            if(const auto n = numbers.find(r.function); n != numbers.end()) controller = n->second;
                            else if(r.function == "pitchBend" || r.function == "aftertouch") function = r.function;
                        }
            }
        }
        catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        if(wake) wake();
        if(!function.empty())
        {
            try
            {
                const double v = std::clamp(input.value, 0.0, 1.0);
                played = function == "pitchBend" ? session_.pitchBend(bendOf(v)) : session_.channelPressure(int(std::lround(v * 127)));
            }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        }
        if(controller >= 0)
        {
            const double v = input.kind == ControlInput::Kind::Switch ? (input.value >= .5 ? 1 : 0) : std::clamp(input.value, 0.0, 1.0);
            try { played = session_.controller(controller, int(std::lround(v * 127))); }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        }
        if(played)
        {
            // Counted as any played control is: the control moved last (by the input's own identity, a lever's axis not the control it
            // drove), for learn, and the moves so far.
            std::lock_guard lock(lock_);
            touched_ = physical;
            if(!mapped && (controller >= 0 || !function.empty())) ++touches_;
        }
        return played;
    }

    bool ControllerInput::control(const std::string& name, ControlInput input, const Navigation& navigation) noexcept
    {
        bool lists = false;
        { std::lock_guard lock(lock_); lists = lists_; }
        const bool played = lists ? false : control(name, input);
        if(!played || lists)
        {
            try
            {
                std::lock_guard lock(lock_);
                navigation_[navigationKey(navigation.action, navigation.pane)] += navigation.action == Navigation::Action::Move ? navigation.steps : 1;
            }
            catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
        }
        return played;
    }

    double ControllerInput::value(const Assignment& a) const
    {
        switch(a.kind)
        {
        case Assignment::Kind::Control: for(const auto& m : macros_) if(m.id == a.id) return m.value; return 0;
        case Assignment::Kind::Scene: return scene_ == a.id ? 1 : 0;
        case Assignment::Kind::None: case Assignment::Kind::Panic: return 0;
        default: { const auto it = values_.find(key(a)); return it == values_.end() ? 0 : it->second; }
        }
    }

    void ControllerInput::setFader(uint32_t part, std::optional<double> level, std::optional<bool> mute)
    {
        const Key l{int(Assignment::Kind::PartLevel), 0, 0, part}, m{int(Assignment::Kind::PartMute), 0, 0, part};
        if(level) { values_[l] = *level; unapplied_.insert(l); }
        if(mute) { values_[m] = *mute ? 1 : 0; unapplied_.insert(m); }
        // A live fader is heard at the next block; any other waits for the control thread.
        if(const auto slot = slots_.find(part); slot != slots_.end()) write(slot->second, values_[m] >= .5 ? 0 : values_[l]);
    }

    void ControllerInput::write(uint8_t slot, double gain)
    {
        session_.graph_.setLiveGain(slot, float(gain));
        gains_[slot] = float(gain);
    }

    // A parameter heard at the next block, its plain value as Session::applyParameter makes it.
    void ControllerInput::send(const Key& k, bool rounded)
    {
        const auto range = ranges_.find(k);
        if(range == ranges_.end()) return;
        const auto& r = range->second;
        double v = std::clamp(r.minimum + values_[k] * (r.maximum - r.minimum), r.minimum, r.maximum);
        if(r.stepped) v = rounded ? std::round(v) : std::trunc(v);
        queue(k, v);
    }

    void ControllerInput::queue(const Key& k, double plain)
    {
        sent_[k] = plain;
        session_.graph_.queueParameter(std::get<1>(k), std::get<2>(k), plain); // full: the control thread's value follows
    }

    void ControllerInput::setPlain(const Key& k, double plain)
    {
        const auto range = ranges_.find(k);
        if(range == ranges_.end()) return;
        const auto& r = range->second;
        plain = std::clamp(plain, r.minimum, r.maximum);
        if(r.stepped) plain = std::trunc(plain);
        const double v = r.maximum > r.minimum ? (plain - r.minimum) / (r.maximum - r.minimum) : 0;
        if(const auto now = values_.find(k); now != values_.end() && std::abs(now->second - v) < 1e-12) return;
        values_[k] = v; unapplied_.insert(k);
        queue(k, plain);
    }

    // A control and its targets by the summing rule, as the Session drives them: part levels,
    // mutes, send levels, parameters and morphs now.
    void ControllerInput::drive(uint32_t control, double value)
    {
        auto current = [&](const MacroTarget& m) -> std::optional<double> {
            switch(m.kind)
            {
            case MacroTarget::Kind::Parameter: { const auto it = values_.find({int(Assignment::Kind::Parameter), m.node, m.param, 0}); if(it == values_.end()) return {}; return it->second; }
            case MacroTarget::Kind::PartLevel: { const auto it = values_.find({int(Assignment::Kind::PartLevel), 0, 0, m.part}); return it == values_.end() ? 1.0 : std::clamp(it->second, 0.0, 1.0); }
            case MacroTarget::Kind::PartMute: { const auto it = values_.find({int(Assignment::Kind::PartMute), 0, 0, m.part}); return it == values_.end() ? 0.0 : it->second; }
            case MacroTarget::Kind::SendLevel: { const auto it = values_.find({sendKey, 0, m.bus, m.part}); if(it == values_.end()) return {}; return std::clamp(it->second, 0.0, 1.0); }
            case MacroTarget::Kind::Macro: for(const auto& x : macros_) if(x.id == m.macro) return x.value; return 0.0;
            case MacroTarget::Kind::Morph: return 0.0;
            }
            return 0.0;
        };
        auto apply = [&](const MacroTarget& m, double x) {
            switch(m.kind)
            {
            case MacroTarget::Kind::Parameter: { const Key k{int(Assignment::Kind::Parameter), m.node, m.param, 0}; if(values_.count(k)) { values_[k] = x; unapplied_.insert(k); send(k, true); } break; }
            case MacroTarget::Kind::PartLevel: setFader(m.part, x, {}); break;
            case MacroTarget::Kind::PartMute: setFader(m.part, {}, x >= .5); break;
            case MacroTarget::Kind::SendLevel:
            {
                const Key k{sendKey, 0, m.bus, m.part};
                if(!values_.count(k)) break;
                values_[k] = x; unapplied_.insert(k);
                if(const auto slot = sends_.find({m.part, m.bus}); slot != sends_.end()) write(slot->second, x); // a live send is heard at the next block
                break;
            }
            case MacroTarget::Kind::Morph: morph(m.sceneFrom, m.sceneTo, x); break;
            case MacroTarget::Kind::Macro: break;
            }
        };
        // A relative target's CLAP modulation, heard at the next block; the Session sends the same when it plays the move.
        auto modulate = [&](const MacroTarget& m, double x) {
            if(const auto r = ranges_.find({int(Assignment::Kind::Parameter), m.node, m.param, 0}); r != ranges_.end())
                session_.graph_.queueModulation(m.node, m.param, x * (r->second.maximum - r->second.minimum));
        };
        Session::drive(macros_, written_, mappingScene_, control, value, current, apply, modulate);
    }

    // As Session::morphScenes, of what blend() sets: the control thread then switches what does
    // not play live.
    void ControllerInput::morph(uint32_t fromId, uint32_t toId, double t)
    {
        if(blending_) return;
        const auto f = std::find_if(scenes_.begin(), scenes_.end(), [&](const Scene& s) { return s.id == fromId; });
        const auto g = std::find_if(scenes_.begin(), scenes_.end(), [&](const Scene& s) { return s.id == toId; });
        if(f == scenes_.end() || g == scenes_.end()) return;
        if(g->scope & Scene::Mappings) mappingScene_ = t >= .5 ? g->id : f->id;
        blend(*f, *g, t, g->scope);
    }

    // What Session::blendScenes sets at t between two scenes that plays without a plan: the
    // controls the scenes hold, live faders, pans, sends and the master, and parameter values
    // (a stepped one switching halfway); at an end, that scene as it is. Lanes, user wires,
    // solos, faders that are not live, and plugins and wires switched on or off wait for the
    // control thread.
    void ControllerInput::blend(const Scene& from, const Scene& to, double t, uint32_t scope)
    {
        const Scene* end = t >= 1 ? &to : t <= 0 ? &from : nullptr;
        const Scene& mid = end ? *end : to;
        const double eased = to.curve == Scene::Curve::Ease ? t * t * (3 - 2 * t) : t;
        const auto mix = [&](double a, double b) { return a + (b - a) * eased; };
        const auto gain = [&](double a, double b) { return to.curve == Scene::Curve::EqualPower ? std::sqrt((1 - t) * a * a + t * b * b) : mix(a, b); };
        blending_ = true;
        struct Restore { bool& flag; ~Restore() { flag = false; } } restore{blending_};
        if(scope & Scene::Parameters)
            for(const auto& [id, value] : mid.macros)
            {
                const auto m = std::find_if(macros_.begin(), macros_.end(), [&](const Macro& x) { return x.id == id; });
                if(m == macros_.end() || std::any_of(m->targets.begin(), m->targets.end(), [](const MacroTarget& x) { return x.kind == MacroTarget::Kind::Morph; })) continue;
                double v = value;
                if(!end) for(const auto& [other, was] : from.macros) if(other == id) v = mix(was, value);
                if(v != m->value) { drive(id, v); unapplied_.insert({int(Assignment::Kind::Control), 0, 0, id}); }
            }
        for(const auto& p : mid.parts)
        {
            if(!values_.count({int(Assignment::Kind::PartLevel), 0, 0, p.part})) continue;
            const Scene::PartState* f = nullptr;
            if(!end) for(const auto& x : from.parts) if(x.part == p.part) f = &x;
            std::optional<double> level; std::optional<bool> mute;
            if(scope & Scene::Mix) level = f ? gain(f->mute ? 0 : f->level, p.mute ? 0 : p.level) : p.level;
            if(scope & Scene::Mutes) mute = f ? f->mute && p.mute : p.mute;
            if(level || mute) setFader(p.part, level, mute);
            if(!(scope & Scene::Mix)) continue;
            if(const auto s = pans_.find(p.part); s != pans_.end())
            {
                const double pan = f ? mix(f->pan, p.pan) : p.pan;
                write(s->second[0], std::min(1.0, 1 - pan)); write(s->second[1], std::min(1.0, 1 + pan));
            }
            for(const auto& x : p.sends)
                if(const auto s = sends_.find({p.part, x.bus}); s != sends_.end())
                {
                    double v = x.level;
                    if(f) for(const auto& was : f->sends) if(was.bus == x.bus) v = gain(was.level, x.level);
                    write(s->second, v);
                }
        }
        if((scope & Scene::Mix) && master_)
        {
            const double level = end ? end->masterLevel : gain(from.masterLevel, to.masterLevel), pan = end ? end->masterPan : mix(from.masterPan, to.masterPan);
            write(Session::masterSlots[0], level * (stereo_ ? std::min(1.0, 1 - pan) : 1)); write(Session::masterSlots[1], level * std::min(1.0, 1 + pan));
        }
        if(scope & Scene::Parameters)
            for(size_t i = 0; i < mid.values.size(); ++i)
            {
                const auto& v = mid.values[i];
                const Key k{int(Assignment::Kind::Parameter), v.at.node, v.at.param, 0};
                const auto r = ranges_.find(k);
                if(r == ranges_.end()) continue;
                double value = v.value;
                if(!end)
                {
                    // Scenes list their values in one order: the same place first.
                    auto was = i < from.values.size() && from.values[i].at == v.at ? from.values.begin() + std::ptrdiff_t(i) : std::find_if(from.values.begin(), from.values.end(), [&](const Scene::Value& x) { return x.at == v.at; });
                    if(was != from.values.end()) value = r->second.stepped ? (t >= .5 ? v.value : was->value) : mix(was->value, v.value);
                }
                setPlain(k, value);
            }
    }

    void ControllerInput::play(const ControllerMapping::Action& action)
    {
        if(!action.control.empty()) { touched_ = action.control; ++touches_; }
        ++moves_;
        const auto& target = action.target;
        const double v = action.value;
        switch(target.kind)
        {
        case Assignment::Kind::PartLevel: setFader(target.id, v, {}); break;
        case Assignment::Kind::PartMute: setFader(target.id, {}, v >= .5); break;
        case Assignment::Kind::Parameter: values_[key(target)] = v; unapplied_.insert(key(target)); send(key(target)); break;
        case Assignment::Kind::Control: drive(target.id, v); unapplied_.insert(key(target)); break;
        case Assignment::Kind::Panic: session_.allNotesOff(); break; // here on the MIDI thread: two atomic flags, heard at the next block
        case Assignment::Kind::Scene:
            // Selected at once (no beat to wait for) with nothing to switch or glide: heard at the next block.
            if(const auto scene = std::find_if(scenes_.begin(), scenes_.end(), [&](const Scene& s) { return s.id == target.id; });
               scene != scenes_.end() && liveScenes_.count(scene->id) && (scene->launch == Scene::Launch::Now || !session_.statusPlaying_.load()))
            {
                if(scene->scope & Scene::Mappings) mappingScene_ = scene->id;
                scene_ = scene->id;
                blend(*scene, *scene, 1, scene->scope);
            }
            break;
        case Assignment::Kind::None: break;
        }
        played_.push_back({target, v});
    }

    void ControllerInput::service()
    {
        std::vector<Played> played;
        uint64_t moves = 0;
        { std::lock_guard lock(lock_); played.swap(played_); moves = moves_; unapplied_.clear(); sent_.clear(); gains_.clear(); }
        if(!played.empty())
        {
            // The latest value per continuous target, mutes and scenes each in order.
            std::vector<ControllerMapping::Action> actions;
            for(const auto& p : played) actions.push_back({{}, p.target, p.value});
            for(const auto& a : ControllerMapping::latest(std::move(actions)))
                try { apply({a.target, a.value}); }
                catch(const std::exception& e) { std::fprintf(stderr, "oscairn: controller input: %s\n", e.what()); }
            // The model has every move now. With no newer move on its way, the live gains take the
            // model's values: the same, unless a control was rebased by something else moving its
            // destination. Otherwise those the newer moves wrote win over the older a scene's
            // edit has just written.
            std::lock_guard lock(lock_);
            if(moves_ == moves) session_.writeLiveGains();
            else for(const auto& [slot, gain] : gains_) session_.graph_.setLiveGain(slot, gain);
        }
        refresh(!played.empty());
    }

    void ControllerInput::resend()
    {
        std::lock_guard lock(lock_);
        for(const auto& k : unapplied_)
            // A value still waiting in the Session is replaced by this one at the next service().
            if(const auto sent = sent_.find(k); sent != sent_.end() && !session_.pending_.count({std::get<1>(k), std::get<2>(k)})) queue(k, sent->second);
    }

    void ControllerInput::apply(const Played& p)
    {
        const auto& t = p.target;
        auto part = [&](uint32_t id) -> const Part* { for(const auto& x : session_.parts_) if(x.id == id) return &x; return nullptr; };
        switch(t.kind)
        {
        case Assignment::Kind::Parameter:
            if(session_.hasParameter(t.node, t.param))
            {
                const auto info = session_.parameterInfo(t.node, t.param);
                session_.setParameter(t.node, t.param, info.minimum + p.value * (info.maximum - info.minimum));
            }
            break;
        case Assignment::Kind::Control: session_.playMacro(t.id, p.value, false); break;
        case Assignment::Kind::PartLevel: if(const auto* x = part(t.id)) session_.playPartLevel(t.id, p.value, x->mute); break;
        case Assignment::Kind::PartMute: if(const auto* x = part(t.id)) session_.playPartLevel(t.id, x->level, p.value >= .5); break;
        case Assignment::Kind::Scene: session_.selectScene(t.id); break;
        case Assignment::Kind::None: case Assignment::Kind::Panic: break; // a panic was played at once
        }
    }

    // Whether selecting the scene now changes only what blend() plays (Session::editMix's
    // liveOnly, with no plugin or wire switched and no graph edit staged) and at once.
    bool ControllerInput::liveScene(const Scene& s) const
    {
        const auto& x = session_;
        if(s.glide || x.staged_) return false;
        const auto has = [&](Scene::Kind kind) { return (s.scope & kind) != 0; };
        if(has(Scene::Enabled))
        {
            for(const auto& [node, name] : x.nodes_) if((std::find(s.off.begin(), s.off.end(), node) != s.off.end()) != (x.disabled_.count(node) != 0)) return false;
            for(const auto& w : x.wires_) if(w.enabled == (std::find(s.offWires.begin(), s.offWires.end(), WireRef::of(w)) != s.offWires.end())) return false;
        }
        if(has(Scene::Mix) && (s.masterLevel != x.masterLevel_ || s.masterPan != x.masterPan_) && x.staticMaster_) return false;
        for(const auto& state : s.parts)
            for(const auto& p : x.parts_)
            {
                if(p.id != state.part) continue;
                if(has(Scene::Mutes) && (state.solo != p.solo || (state.mute != p.mute && !x.liveFader(p.id)))) return false;
                if(!has(Scene::Mix)) continue;
                if((state.level != p.level && !x.liveFader(p.id)) || (state.pan != p.pan && !x.livePan(p.id))) return false;
                for(const auto& send : p.sends)
                    for(const auto& was : state.sends) if(was.bus == send.bus && was.level != send.level && !x.liveSend(p.id, send.bus)) return false;
                for(const auto& sp : p.splits)
                    for(const auto& lane : state.lanes)
                        if(lane.split == sp.id && lane.lane < sp.lanes.size() && !(Scene::LaneState{lane.split, lane.lane, sp.lanes[lane.lane].level, sp.lanes[lane.lane].pan, sp.lanes[lane.lane].mute, sp.lanes[lane.lane].solo} == lane)) return false;
            }
        if(has(Scene::Mix))
            for(const auto& w : x.wires_)
                if(w.owner == WireOwner::User)
                    for(const auto& m : s.wires) if(m.wire == WireRef::of(w) && !(Scene::WireMix{m.wire, w.level, w.pan, w.mute, w.solo} == m)) return false;
        // A control the scene moves drives its targets: each must play live too.
        std::function<bool(uint32_t)> live = [&](uint32_t id) {
            for(const auto& m : x.macros_)
                if(m.id == id)
                    return std::all_of(m.targets.begin(), m.targets.end(), [&](const MacroTarget& t) {
                        return t.kind == MacroTarget::Kind::Parameter || t.kind == MacroTarget::Kind::Morph ||
                               (t.kind == MacroTarget::Kind::Macro ? live(t.macro) : t.kind == MacroTarget::Kind::SendLevel ? x.liveSend(t.part, t.bus) : x.liveFader(t.part));
                    });
            return true;
        };
        if(has(Scene::Parameters))
            for(const auto& [id, value] : s.macros)
                for(const auto& m : x.macros_)
                    if(m.id == id && m.value != value && std::none_of(m.targets.begin(), m.targets.end(), [](const MacroTarget& t) { return t.kind == MacroTarget::Kind::Morph; }) && !live(id)) return false;
        return true;
    }

    // The snapshot: compiled again when the Session, the inputs or the groups changed.
    void ControllerInput::refresh(bool moved)
    {
        const auto& inputs = session_.hostStatus_.midiInputs;
        auto live = session_.liveValues();
        if(!moved && !dirty_ && session_.sequence_ == sequence_ && session_.revision_ == revision_ && inputs == inputs_ && live == liveSeen_) return;
        liveSeen_ = std::move(live);
        std::vector<Control::ControllerPort> ports;
        { std::lock_guard lock(lock_); ports = ports_; }
        // shortcut: compiled whole on every change of the Session (a parameter turned on screen too); compile only what changed if this shows in the control thread's load.
        const auto state = session_.graphState();
        auto source = assignmentSource(state, session_.assignments_, [&](NodeId n) { return session_.parameters(n); });
        std::map<Key, double> values;
        for(const auto& p : state.parts)
        {
            values[{int(Assignment::Kind::PartLevel), 0, 0, p.id}] = p.level;
            values[{int(Assignment::Kind::PartMute), 0, 0, p.id}] = p.mute ? 1 : 0;
            for(const auto& x : p.sends) values[{sendKey, 0, x.bus, p.id}] = x.level;
        }
        std::map<Key, Range> ranges;
        for(const auto& [id, node] : source.nodes)
            for(const auto& param : node.params)
            {
                const auto info = session_.parameterInfo(id, param.id);
                values[{int(Assignment::Kind::Parameter), id, param.id, 0}] = info.maximum > info.minimum ? (session_.parameter(id, param.id) - info.minimum) / (info.maximum - info.minimum) : 0;
                if(!(info.flags & CLAP_PARAM_IS_READONLY)) ranges[{int(Assignment::Kind::Parameter), id, param.id, 0}] = {info.minimum, info.maximum, info.stepped};
            }
        std::map<uint32_t, uint8_t> slots;
        for(const auto& [part, slot] : session_.faderSlots_) if(session_.liveFader(part)) slots[part] = slot;
        std::map<uint32_t, std::array<uint8_t, 2>> pans;
        for(const auto& [part, slot] : session_.panSlots_) if(session_.livePan(part)) pans[part] = slot;
        std::map<std::pair<uint32_t, uint32_t>, uint8_t> sends;
        for(const auto& [send, slot] : session_.sendSlots_) if(session_.liveSend(send.first, send.second)) sends[send] = slot;
        const auto& outputs = session_.graph_.audioLayout().outputs;
        // shortcut: every scene copied and checked at each compile (as Session::model() copies them for undo); only those morphs and buttons reach if this shows in the control thread's load.
        auto scenes = session_.scenes_;
        std::set<uint32_t> liveScenes;
        for(const auto& scene : scenes) if(liveScene(scene)) liveScenes.insert(scene.id);
        std::map<uint8_t, std::vector<std::pair<std::string, uint32_t>>> bindings;
        for(const auto& input : inputs)
            for(const auto& port : ports) if(port.input == input.name) bindings[input.source].push_back({port.device, port.port});
        std::lock_guard lock(lock_);
        source.screen = screen_;
        source.momentary = session_.momentary_;
        source_ = std::move(source);
        // A value a MIDI thread set since the moves were taken is newer than the model's.
        for(const auto& k : unapplied_) if(const auto it = values_.find(k); it != values_.end()) values[k] = it->second;
        values_ = std::move(values);
        auto macros = session_.macros_;
        for(auto& m : macros)
            if(unapplied_.count({int(Assignment::Kind::Control), 0, 0, m.id}))
                for(const auto& old : macros_) if(old.id == m.id) m.value = old.value;
        macros_ = std::move(macros);
        written_ = session_.written_;
        mappingScene_ = session_.mappingScene_; scene_ = session_.scene_;
        slots_ = std::move(slots); pans_ = std::move(pans); sends_ = std::move(sends);
        master_ = !session_.staticMaster_; stereo_ = !outputs.empty() && outputs.front().channels == 2;
        scenes_ = std::move(scenes); liveScenes_ = std::move(liveScenes);
        ranges_ = std::move(ranges);
        bindings_ = std::move(bindings);
        positions_.resize(groups_.size());
        sequence_ = session_.sequence_; revision_ = session_.revision_; inputs_ = inputs; dirty_ = false;
    }
}
