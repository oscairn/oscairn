// SPDX-License-Identifier: AGPL-3.0-or-later
#pragma once
namespace oscairn::standalone
{
    /// What a panel, or any input, asks of the lists on screen (ListCursor::act). The app knows only
    /// these: which control of which device asks which is the device's panel map (`navigate`).
    struct ListAction
    {
        enum class Kind { Move, Confirm, Cancel } kind = Kind::Move;
        int pane = -1;  // the list pane from the left (the last for a number past it); -1: the pane last touched
        int steps = 1;  // a move's, negative towards the start
    };
}
