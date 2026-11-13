Oscairn for Linux
=================

This folder runs from wherever it is unpacked: the program, its engine, plugin worker, scan
helper, bundled plugins, themes and the Setlist and Settings apps are found beside bin/Oscairn.
Nothing is installed and no root is needed.

    bin/Oscairn

Start it from there, or link it onto your PATH:

    ln -s "$PWD/bin/Oscairn" ~/.local/bin/Oscairn

It needs a desktop with X11 (or XWayland) and these packages, on Ubuntu 24.04 or Debian 13 or
later (x86_64 or arm64):

    sudo apt-get install libasound2t64 libcurl4t64 libfreetype6 libfontconfig1 libx11-6 \
        libxext6 libxinerama1 libxrandr2 libxcursor1 libxi6

Older systems have an older C++ runtime than the build needs.

share/applications/oscairn.desktop and share/icons/ are for a desktop that lists Oscairn in
its menu: copy them under ~/.local/share/, with Oscairn on your PATH.

To try it without sound or a MIDI device: bin/Oscairn --audio null

This release comes without the instruments Oscairn FM, Analog, the Sampler, Effects and the
Arpeggiator: the package repository has no Linux builds of them yet.

Licenses, and where the source of this exact build is: Licenses/.
https://github.com/oscairn/oscairn
