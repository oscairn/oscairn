// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once

#include "oscairn/control.h"

#include <optional>
#include <string_view>

namespace oscairn
{
    class JsonRpc;
    /// Messages between apps (RFC 0022), which a transport holding several clients relays: it
    /// gives each client's adapter one of these. Each returns its result's JSON, or throws
    /// std::invalid_argument with why it was refused. The relay never interprets an action.
    class AppRelay
    {
    public:
        virtual ~AppRelay() = default;
        virtual std::string connect( JsonRpc& from, const std::string& app ) = 0;
        virtual std::string request( JsonRpc& from, const std::string& app, const std::string& action, const std::string& argument ) = 0;
        virtual std::string answer( JsonRpc& from, uint64_t request, const std::string& result ) = 0;
        virtual std::string pong( JsonRpc& from, uint64_t ping ) = 0;
    };

    /// Transport-independent JSON-RPC adapter. Call only on Control's thread, never
    /// from audio. No I/O or threads; one complete JSON document per dispatch call.
    /// nullopt means notification(s) only: a transport must send no response.
    /// See docs/control-protocol.md for methods, limits and partial queue acceptance.
    class JsonRpc
    {
    public:
        static constexpr size_t maxMessageBytes = 65536;
        static constexpr size_t maxDepth = 32;
        static constexpr size_t maxBatch = 64;
        static constexpr size_t maxParameterChanges = 128;

        explicit JsonRpc( Control& control ) : control_( control ) {}
        std::optional<std::string> dispatch( std::string_view message );
        /// One adapter per client. Host supplies monotonic time; no clock reads in core.
        /// Default notifications are throttled to 60 Hz; allChanges bypasses coalescing.
        std::vector<std::string> notifications( uint64_t nowNanoseconds );
        void subscription( std::string_view topic, bool enabled, bool allChanges = false );
        /// A managed engine's lifetime: this client is a UI that keeps it (engine.attach) and has
        /// quit normally (engine.release). The transport decides what they mean for the process.
        void attach() { attached_ = true; released_ = false; }
        void release() { released_ = true; }
        bool attached() const { return attached_; }
        bool released() const { return released_; }
        /// The apps.* methods go to `relay`; without one they are refused.
        void relayApps( AppRelay* relay ) { apps_ = relay; }
        AppRelay* apps() const { return apps_; }

    private:
        Control& control_;
        AppRelay* apps_ = nullptr;
        bool params_ = false, transport_ = false, graph_ = false, allChanges_ = false;
        bool status_ = false, attached_ = false, released_ = false;
        uint64_t paramsCursor_ = 0, graphCursor_ = 0;
        std::optional<uint64_t> lastParams_, lastTransport_;
        std::string lastTransportValue_;
        std::optional<uint64_t> lastStatus_;
        std::string lastStatusValue_;
        bool record_ = false;
        std::optional<uint64_t> lastRecord_;
        std::string lastRecordValue_;
        std::map<uint32_t, std::string> recordStates_; // takes notified, by their state then
        bool controllers_ = false, values_ = false;
        std::optional<uint64_t> lastValues_;
        std::optional<Control::LiveValues> valuesSent_;
        std::optional<uint64_t> lastControllers_;
        std::string lastControllersValue_;
    };
}
