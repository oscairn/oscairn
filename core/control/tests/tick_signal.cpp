// SPDX-License-Identifier: AGPL-3.0-or-later
// ignoreTickSignal: a process that called it survives signal 42 (SIGRTMIN + 8), which Roland's
// tickgen module sends to a stale pid on a FANTOM-0 (docs/fantom-0.md); one that did not, dies of it.
#include "oscairn/plugin_host.h"
#include <csignal>
#include <cstdio>
#include <sys/wait.h>
#include <unistd.h>

static int run(bool ignore)
{
    const pid_t child = fork();
    if(child == 0) { if(ignore) oscairn::ignoreTickSignal(); raise(SIGRTMIN + 8); _exit(0); }
    int status = 0;
    waitpid(child, &status, 0);
    return status;
}

int main()
{
    int failed = 0;
    const int plain = run(false), ignoring = run(true);
    if(!(WIFSIGNALED(plain) && WTERMSIG(plain) == SIGRTMIN + 8)) { std::puts("FAIL: control: signal 42 should kill a plain process"); failed = 1; }
    if(!(WIFEXITED(ignoring) && WEXITSTATUS(ignoring) == 0)) { std::puts("FAIL: a process that called ignoreTickSignal was killed by signal 42"); failed = 1; }
    if(!failed) std::puts("ok");
    return failed;
}
