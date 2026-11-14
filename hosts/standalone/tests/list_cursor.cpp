// SPDX-License-Identifier: AGPL-3.0-or-later
//
// The list cursor (Theme.h's ListCursor and CursorList), through a hidden window: it moves without
// activating, confirming activates, panes take the left and right knobs by pane number, a page turn
// of the list keeps it and a page change drops it, a move repaints the two rows it touches (counted
// by PaintCounter) and no more, the arrow keys, Return and Escape do the same on a computer; then
// what a device's panel does reaches it through the engine: a control a device's map makes a
// navigation (here an invented device's, "jog", "dial:3", "ok": the names are the device's and no
// code here or in the app knows them) is counted by the engine, read by the app, and moves the
// cursor, and with a list on screen it plays nothing.
//   oscairn_list_cursor_test PLUGIN_DIR THEMES
#include "../Engine.h"
#include "../PaintCounter.h"
#include "../Theme.h"
#include "oscairn/local_socket.h"
#include "oscairn/session.h"
#include <atomic>
#include <chrono>
#include <cstdio>
#include <filesystem>
#include <thread>
#include <unistd.h>

using namespace oscairn;
using standalone::CursorList;
using standalone::ListAction;
using standalone::ListCursor;

namespace
{
    // Rows that paint themselves, with the cursor over them, and count what is done to them.
    struct Rows : juce::ListBoxModel
    {
        int count = 100, painted = 0;
        CursorList* list = nullptr;
        std::vector<int> confirmed;
        int getNumRows() override { return count; }
        void paintListBoxItem(int row, juce::Graphics& g, int w, int h, bool selected) override
        {
            ++painted;
            g.fillAll(juce::Colour(row % 2 ? 0xff20252d : 0xff2c333d));
            if(selected) g.fillAll(juce::Colours::darkorange);
            if(list) list->cursor.drawRow(g, row, {0, 0, w, h});
        }
    };
    struct Pane
    {
        Rows rows;
        CursorList list{"pane", &rows};
        explicit Pane() { rows.list = &list; list.setRowHeight(40); list.onConfirm = [this](int row) { rows.confirmed.push_back(row); }; }
    };
    void settle(int ms) { const auto end = juce::Time::getMillisecondCounter() + juce::uint32(ms); while(juce::Time::getMillisecondCounter() < end) juce::MessageManager::getInstance()->runDispatchLoopUntil(5); }
}

int main(int argc, char** argv)
{
    if(argc < 3) return 2;
    juce::ScopedJuceInitialiser_GUI gui;
    // The theme paints the windows, and counts the pixels it is asked to (PaintCounter).
    const auto configuration = juce::File::createTempFile("oscairn-cursor-config");
    standalone::Theme theme(configuration, juce::File(argv[2]));
    juce::LookAndFeel::setDefaultLookAndFeel(&theme);
    theme.apply("oscairn-block");
    int failures = 0;
    auto check = [&](bool ok, const std::string& what) { if(!ok) { std::fprintf(stderr, "FAIL: %s\n", what.c_str()); ++failures; } };
    auto wait = [](auto done) { for(int i = 0; i < 500 && !done(); ++i) juce::MessageManager::getInstance()->runDispatchLoopUntil(10); return done(); };
    auto act = [](ListAction::Kind kind, int pane, int steps) { ListAction a; a.kind = kind; a.pane = pane; a.steps = steps; return ListCursor::act(a); };
    using Kind = ListAction::Kind;

    juce::Component window;
    window.setOpaque(true);
    Pane left, right;
    window.addAndMakeVisible(left.list); window.addAndMakeVisible(right.list);
    constexpr int height = 720;
    window.setBounds(0, 0, 820, height);
    left.list.setBounds(0, 0, 400, height); right.list.setBounds(420, 0, 400, height);
    window.addToDesktop(juce::ComponentPeer::windowIsTemporary);
    window.setVisible(true);
    settle(200);
    const int row = left.list.getRowHeight(), shown = left.list.getViewport()->getViewHeight() / row;
    check(shown >= 8 && shown < 100, "the list shows some of its rows: " + std::to_string(shown));

    // Moving opens nothing: the first move shows the cursor at the first row shown, later ones step; the selection and the
    // taps (onConfirm) are not touched.
    check(!act(Kind::Cancel, -1, 0), "cancel with no cursor is not taken (the shell goes back)");
    check(act(Kind::Move, 0, 1) && left.list.cursor.row() == 0, "the first move shows the cursor at the first row");
    act(Kind::Move, 0, 3);
    check(left.list.cursor.row() == 3 && left.list.getSelectedRow() == -1 && left.rows.confirmed.empty(), "it moved 3 more; nothing selected or opened");
    act(Kind::Move, 0, -10);
    check(left.list.cursor.row() == 0, "held at the first row");
    act(Kind::Move, 0, 3);
    check(act(Kind::Confirm, 0, 0) && left.rows.confirmed == std::vector<int>{3} && right.rows.confirmed.empty(), "confirm does what a tap does, on that row, in that pane");

    // Two panes: pane 0 the left, pane 1 the right, a number past the last the last; no pane: the one touched last.
    check(act(Kind::Move, 1, 5) && right.list.cursor.row() == 0 && left.list.cursor.row() == 3, "pane 1 moves the right list; the left keeps its cursor");
    act(Kind::Move, 1, 4); act(Kind::Move, 7, 1);
    check(right.list.cursor.row() == 5, "a pane past the last is the last: " + std::to_string(right.list.cursor.row()));
    act(Kind::Move, -1, 2);
    check(right.list.cursor.row() == 7 && left.list.cursor.row() == 3, "no pane: the one last touched");
    act(Kind::Move, 0, 1); act(Kind::Move, -1, 1);
    check(left.list.cursor.row() == 5 && right.list.cursor.row() == 7, "and after the left, the left");
    act(Kind::Confirm, 1, 0);
    check(right.rows.confirmed == std::vector<int>{7} && left.rows.confirmed == std::vector<int>{3}, "confirm in pane 1 is the right list's row");
    check(act(Kind::Cancel, -1, 0) && left.list.cursor.row() == -1 && right.list.cursor.row() == -1 && !act(Kind::Cancel, -1, 0), "cancel drops every cursor, and then is not taken");

    // Paging: out of the rows shown the view turns a page at a time, the cursor in sight; a page turn keeps the cursor.
    auto* view = left.list.getViewport();
    act(Kind::Move, 0, 1);
    for(int i = 0; i < shown + 3; ++i) act(Kind::Move, 0, 1);
    const int at = left.list.cursor.row();
    const int page = shown - 1;
    check(at == shown + 3 && view->getViewPositionY() == page * row, "past the last row shown, a page turned, not a scroll to the row: at " + std::to_string(view->getViewPositionY()));
    const auto r = left.list.getRowPosition(at, true);
    check(r.getY() >= 0 && r.getBottom() <= view->getViewHeight(), "the cursor is in sight");
    standalone::pageList(left.list, 1);
    check(left.list.cursor.row() == at && left.rows.confirmed.size() == 1, "a page turn of the list keeps the cursor and opens nothing");
    act(Kind::Move, 0, -(at + 5));
    check(left.list.cursor.row() == 0 && view->getViewPositionY() == 0, "back to the first row, the view back a page at a time to the top");
    act(Kind::Move, 0, 500);
    check(left.list.cursor.row() == 99 && view->getViewPositionY() + view->getViewHeight() >= 100 * row - 1, "held at the last row, in sight");
    act(Kind::Cancel, -1, 0);
    view->setViewPosition(0, 0);

    // A page change drops it: a view taken out and put back (Panel::overlay), or hidden.
    act(Kind::Move, 0, 2); act(Kind::Move, 1, 2);
    window.removeChildComponent(&right.list); window.addAndMakeVisible(right.list);
    check(right.list.cursor.row() == -1 && left.list.cursor.row() == 0, "a pane taken out of its parent and put back loses its cursor, the other keeps it");
    left.list.setVisible(false);
    act(Kind::Move, 1, 1);
    check(left.list.cursor.row() == -1, "a pane hidden has none");
    left.list.setVisible(true);
    check(act(Kind::Move, 0, 1) && left.list.cursor.row() == 0 && !left.list.getSelectedRows().size(), "and shown again begins afresh");
    act(Kind::Cancel, -1, 0);

    // Keys, as a computer's: arrows, page keys, Return, Escape; the same moves and the same tap.
    left.rows.confirmed.clear();
    left.list.grabKeyboardFocus();
    check(left.list.keyPressed(juce::KeyPress(juce::KeyPress::downKey)) && left.list.cursor.row() == 0, "down shows the cursor");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::downKey)); left.list.keyPressed(juce::KeyPress(juce::KeyPress::downKey));
    check(left.list.cursor.row() == 2 && left.list.getSelectedRow() == -1, "down twice: no selection");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::pageDownKey));
    check(left.list.cursor.row() == 2 + shown - 1, "page down: a page");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::upKey)); left.list.keyPressed(juce::KeyPress(juce::KeyPress::returnKey));
    check(left.rows.confirmed == std::vector<int>{shown}, "return does a tap's work on the row");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::escapeKey));
    check(left.list.cursor.row() == -1 && !left.list.keyPressed(juce::KeyPress(juce::KeyPress::escapeKey)), "escape drops it, and with none is left to others");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::endKey));
    check(left.list.cursor.row() == 99, "end: the last row");
    left.list.keyPressed(juce::KeyPress(juce::KeyPress::homeKey));
    check(left.list.cursor.row() == 0, "home: the first");
    left.list.cursor.clear(); view->setViewPosition(0, 0);

    // What a move draws: the row it left and the one it entered, not the list.
    {
        standalone::PaintCounter counter;
        counter.watch();
        settle(300);
        counter.reset();
        left.list.repaint(); settle(150);
        const auto full = counter.counts(window);
        check(full.frames >= 1 && full.pixels > 0, "the counter sees a repaint of the list");
        counter.reset();
        act(Kind::Move, 0, 1); settle(80);                       // the cursor appears: one row
        const auto appears = counter.counts(window);
        counter.reset();
        const int before = left.rows.painted;
        for(int i = 0; i < 10; ++i) { act(Kind::Move, 0, 1); settle(80); }
        const auto ten = counter.counts(window);
        const int rowsPainted = left.rows.painted - before;
        const double rowShare = double(row) / height, listPixels = double(full.pixels);
        std::printf("a repaint of the list (%d rows): %lld pixels in %d frames\n", shown, (long long) full.pixels, full.frames);
        std::printf("the cursor appearing: %lld pixels; ten moves: %lld pixels in %d frames (%.1f%% of ten list repaints; twenty rows are %.1f%%), %d rows painted\n",
                    (long long) appears.pixels, (long long) ten.pixels, ten.frames, 100.0 * ten.pixels / (10 * listPixels), 100.0 * 2 * rowShare, rowsPainted);
        check(ten.frames >= 1 && ten.frames <= 10, "ten moves drawn in at most ten frames: " + std::to_string(ten.frames));
        check(rowsPainted == 20, "ten moves paint twenty rows, two each: " + std::to_string(rowsPainted));
        check(double(ten.pixels) <= 10 * 2 * rowShare * listPixels * 1.1 + 1, "ten moves draw ten times two rows, not ten lists: " + std::to_string(ten.pixels) + " against " + std::to_string(10 * listPixels));
        check(double(appears.pixels) <= rowShare * listPixels * 1.1 + 1, "the cursor appearing draws one row");
        counter.reset();
        settle(500);
        check(counter.counts(window).frames == 0, "at rest, with the cursor shown, nothing is drawn");
        left.list.cursor.clear(); settle(80);
    }

    // The panel: an invented device's controls through a session (jog dial, dial:3, ok), whose map the engine's
    // reader would apply (devices/); the app is told only navigations.
    {
        const auto socket = std::filesystem::temp_directory_path() / ("oscairn-cursor-" + std::to_string(::getpid()) + ".sock");
        const auto data = juce::File::createTempFile("oscairn-cursor-data");
        ::setenv("OSCAIRN_DATA_HOME", data.getFullPathName().toRawUTF8(), 1);
        auto session = std::make_unique<Session>(48000, 128);
        auto server = std::make_unique<LocalServer>(*session);
        server->listen(socket.string());
        std::atomic<bool> running = true;
        std::thread control([&] {
            std::array<float, 128> l{}, r{}; float* out[2]{l.data(), r.data()};
            while(running)
            {
                server->poll(uint64_t(std::chrono::steady_clock::now().time_since_epoch().count()));
                session->service();
                session->process(nullptr, 0, out, 2, 128);
                std::this_thread::sleep_for(std::chrono::milliseconds(1));
            }
            server.reset(); session.reset();
        });
        {
            standalone::Engine engine(argv[1], socket.string());
            engine.onList = ListCursor::act; engine.listsShowing = ListCursor::anyShowing;
            std::atomic<int> goneBack = 0;
            engine.goBack = [&] { ++goneBack; };
            check(wait([&] { return engine.connected(); }), "the engine is reached");
            settle(600); // its first reading of the controllers is the baseline
            left.rows.confirmed.clear(); right.rows.confirmed.clear();
            using In = ControlInput;
            auto navigate = [&](const char* control, In input, Navigation n) { return session->controllerControl(control, input, n); };
            Navigation move; move.steps = 1;
            navigate("jog", {In::Kind::Relative, .01}, move);
            check(wait([&] { return left.list.cursor.row() >= 0; }), "a jog dial's step reaches the focused list (the pane touched last)");
            const int first = left.list.cursor.row();
            Navigation back; back.steps = 4; back.pane = 1;
            navigate("dial:3", {In::Kind::Relative, .01}, back);
            check(wait([&] { return right.list.cursor.row() >= 0; }) && left.list.cursor.row() == first, "a navigation for pane 1 moves the right list and not the left");
            const int rightAt = right.list.cursor.row();
            navigate("dial:3", {In::Kind::Relative, .01}, back);
            check(wait([&] { return right.list.cursor.row() == rightAt + 4; }), "its steps are summed: 4 more");
            Navigation ok; ok.action = Navigation::Action::Confirm; ok.pane = 1;
            navigate("ok", {In::Kind::Switch, 1}, ok);
            check(wait([&] { return !right.rows.confirmed.empty(); }) && right.rows.confirmed.back() == rightAt + 4 && left.rows.confirmed.empty(), "a confirm taps the right pane's row");
            Navigation cancel; cancel.action = Navigation::Action::Cancel;
            navigate("back", {In::Kind::Switch, 1}, cancel);
            check(wait([&] { return left.list.cursor.row() == -1 && right.list.cursor.row() == -1; }) && goneBack == 0, "a cancel drops the cursors, and does not go back");
            navigate("back", {In::Kind::Switch, 1}, cancel);
            check(wait([&] { return goneBack == 1; }), "and with none, goes back as the shell's Back does");
            // Lists on screen: a control that plays something navigates and plays nothing; with none it plays.
            session->addPart("A"); session->commit();
            session->setControllers({[] { ControlGroup g; g.role = ControlGroup::Role::Assign; g.kind = "buttons"; g.count = 1; return g; }()}, {});
            settle(300);
            Navigation step; step.steps = 2;
            const auto touches = session->controllerState().touches;
            navigate("button:101", {In::Kind::Switch, 1}, step);
            check(wait([&] { return left.list.cursor.row() >= 0 || right.list.cursor.row() >= 0; }) && session->controllerState().touches == touches, "with lists on screen a button the map makes a navigation moves the cursor and plays nothing");
            window.setVisible(false);
            check(wait([&] { return !ListCursor::anyShowing(); }), "the lists are off screen");
            settle(600); // the engine is told
            const int cursorAt = left.list.cursor.row();
            navigate("button:101", {In::Kind::Switch, 1}, step);
            check(wait([&] { return session->controllerState().touches == touches + 1; }) && left.list.cursor.row() == cursorAt, "with none it plays what it is assigned, and moves no list");
            window.setVisible(true);
        }
        running = false; control.join();
    }
    window.removeFromDesktop();
    juce::LookAndFeel::setDefaultLookAndFeel(nullptr);
    std::printf(failures ? "FAILED\n" : "ok: the list cursor\n");
    return failures ? 1 : 0;
}
