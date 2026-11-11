# The Settings app

Settings is an app of its own ([RFC 0022](../../../../docs/rfcs/0022-apps-and-requests.md#the-built-in-apps)),
`Oscairn Settings`, started by the shell from `app.json` with a connection held to `system`.

| File | What it is |
| --- | --- |
| `app.json` | The manifest: `system` alone |
| `SettingsApp.cpp` | The process: its connection, the profile, the window, the shell's show request, `--snapshot` |
| `Frame.h`, `Frame.cpp` | The frame (the list of pages and the page chosen, scrolling), `Page` and `Row` |
| `Pages.cpp` | **The one list of pages**, each with where it applies |
| `AudioPage.h`, `AudioPage.cpp` | The Audio page: the example to copy |
| `SigningKeysPage.h`, `SigningKeysPage.cpp` | The Signing keys page: the keys and Make, asked with the dialog's own `askToMakeKey` (`../../SigningKeys.h`) |
| `MidiControllersPage.h`, `MidiControllersPage.cpp` | The MIDI controllers page: each MIDI input's controller profile and Ignore switch, kept by the engine |

## Adding a page

1. **A class:** `FooPage.h` and `FooPage.cpp` here, a `Page` whose constructor takes the
   `Engine&`. Make its controls members, and add a `Row` for each with `addRow(label, control,
   explanation)`: a label, a control and an optional line under them. Change a row's line with
   `setExplanation(text, danger)`; a failure goes there, in the danger color, under what failed.
   Hide a row with `setVisible(false)` and call `heightChanged()`.
2. **The engine:** a quick read through the cached `Engine` (`sessionStatus()`); anything that
   may take long through `engine.job(work, done)`, which runs `work(Control&)` off the message
   thread on a connection of its own and calls `done(error)` on the message thread. `work` touches
   only what it captures (a `std::shared_ptr` for the result); `done` checks a
   `SafePointer` to the page, which may have closed. Disable the controls while a job is out.
3. **Register it** in `Pages.cpp`: `{"foo", "Foo", applies, made<FooPage>()}`, in the order the
   list shows. If it replaces a placeholder, put `made<FooPage>()` in place of `notBuilt`.
4. **Build it:** add `apps/settings/FooPage.cpp` to `settings_sources` in
   `hosts/standalone/CMakeLists.txt` (the app and the `desktop-settings` test both use it).
5. **Test it** in `tests/desktop_settings.cpp`, against the socket session there, on an app
   connection granted what the manifest grants (as the Audio page is).

## Where a page applies

`applies` takes the device's `Profile` (RFC 0024) and says whether the page belongs there;
`everywhere` (null) for all:

- **The OS:** `p.platform.os` is `macos`, `linux` or `windows`, for example
  `[](const Profile& p) { return p.platform.os == "linux"; }`.
- **A device or the desktop:** `onDevice` (a device's own image, which has a
  `platform.runtime`) and `onDesktop` (the app on a computer).
- **Hardware:** anything the profile has, for example `p.network.wifi`, `p.battery`,
  `p.midi.usbHost`.
- **The build:** a page whose code a build may leave out is left out of the list there too, with
  `#if` around its entry and its source (RFC 0036's Bluetooth: `#if OSCAIRN_BLUETOOTH`).

A page with `notBuilt` in place of a class is one an RFC names that is not written yet; it is
never shown. The profile comes from `OSCAIRN_PROFILE`, which the shell sets from its `--profile`
(a built-in's id or a file's full path); without it, this computer's.

## Permissions

The app's connection may call what `system` gives and what every app may
([control protocol](../../../../docs/control-protocol.md#permissions)). A page that needs more
(`library`, say) adds it to `app.json`'s `permissions`, and the `desktop-settings` test's check
of the manifest's permissions changes with it. The connection reads the status alone
(`Engine(fd, false)`): the graph, the transport and the controllers are not read.
