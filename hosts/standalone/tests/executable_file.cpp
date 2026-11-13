// SPDX-License-Identifier: AGPL-3.0-or-later
//
// executableFile() (Executable.h) when the program is started by its bare name through PATH,
// from another working directory, as a script starts the app ("Oscairn"): its own file, where
// its helpers are found beside it, not a file of that name in the working directory. Linux.
//   oscairn-executable-file-test
#include "../Executable.h"

#include <cstdio>
#include <cstdlib>
#include <string>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char** argv)
{
    const auto self = oscairn::standalone::executableFile();
    if(argc > 1 && std::string(argv[1]) == "--child")
    {
        std::printf("started as %s, from %s: %s\n", argv[0], juce::File::getCurrentWorkingDirectory().getFullPathName().toRawUTF8(), self.getFullPathName().toRawUTF8());
        return self.existsAsFile() && self.getFileName() == juce::String(argv[0]) && self.getParentDirectory().getFullPathName() == juce::String(argv[2]) ? 0 : 1;
    }
    // By its bare name through PATH, from /.
    const auto folder = self.getParentDirectory().getFullPathName().toStdString(), name = self.getFileName().toStdString();
    const pid_t child = ::fork();
    if(child == 0)
    {
        if(::chdir("/") != 0) ::_exit(3);
        ::setenv("PATH", folder.c_str(), 1);
        ::execlp(name.c_str(), name.c_str(), "--child", folder.c_str(), static_cast<char*>(nullptr));
        ::_exit(4);
    }
    int status = 0;
    ::waitpid(child, &status, 0);
    const bool ok = WIFEXITED(status) && WEXITSTATUS(status) == 0;
    std::fprintf(stderr, ok ? "ok: started by its bare name, it finds its own file\n" : "FAIL: started by its bare name, it does not find its own file\n");
    return ok ? 0 : 1;
}
