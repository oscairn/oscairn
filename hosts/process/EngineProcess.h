// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
#include "oscairn/local_socket.h"
#include "../audio/AudioConfiguration.h"
#include <memory>
#include <string>
#include <vector>
#include <sys/types.h>

namespace oscairn::host
{
    /// A helper started as the engine is: no shell, stdin from /dev/null, stdout and stderr to
    /// `output`, in a process group of its own, with this environment plus `environment`
    /// ("NAME=value" each). `inherit`, when not -1, is the child's descriptor 3 (an app's control
    /// connection, RFC 0022). Throws on failure.
    pid_t spawnChild(const std::vector<std::string>& arguments, int output, const std::vector<std::string>& environment = {}, int inherit = -1);
    /// Stops a child of ours that has not been reaped: SIGTERM, then SIGKILL after graceMs, and
    /// reaps it. Never a PID read from disk.
    void stopChild(pid_t child, unsigned graceMs = 200);

    /// Lifetime lock for an engine started by the desktop. Under this lock only,
    /// a refused connection permits removing a stale socket, never another file.
    class EngineLease
    {
    public:
        explicit EngineLease(const std::string& socketPath);
        ~EngineLease();
        EngineLease(const EngineLease&)=delete;
        EngineLease& operator=(const EngineLease&)=delete;
    private:
        int lock_=-1;
    };

    /// Desktop-owned launcher, not audio ownership. Destruction leaves a running
    /// helper alive. All operations belong to one control thread.
    class EngineProcess
    {
    public:
        EngineProcess(std::string executable,std::string pluginDirectory,std::string runtimeDirectory={},const AudioConfiguration& audio={});
        ~EngineProcess();
        EngineProcess(const EngineProcess&)=delete;
        EngineProcess& operator=(const EngineProcess&)=delete;
        /// Attach first; serialize startup and wait up to timeoutMs for readiness.
        /// Never replaces a reachable server or replays mutations. On startup failure,
        /// stops only its own newly spawned child; an existing helper is left alone.
        void connectOrStart(LocalClient&,unsigned timeoutMs=5000);
        bool running();
        const std::string& socketPath()const;
        const std::string& logPath()const;
    private:
        struct Impl;
        std::unique_ptr<Impl> impl_;
    };
}
