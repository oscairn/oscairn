# Setlist format, version 2

A setlist is a library item, of the item type `oscairn.setlist`
([RFC 0023](rfcs/0023-item-types.md)): named pages, in order, each an ordered list of slots, and
each slot a Performance by its library ID ([RFC 0021](rfcs/0021-sounds-performances-and-library.md)).
The Setlist app reads and writes setlists through the library (`setlists.get`, `setlists.save`,
[control protocol](control-protocol.md#setlists)); the engine never plays one by itself.

## Where it is

Each setlist is a folder whose name ends in `.osetlist`, holding one file, `setlist.json`:

| Where | Path |
| --- | --- |
| The user's own | `Setlists/<name>.osetlist/setlist.json` in the data folder (`OSCAIRN_DATA_HOME`, else Application Support on macOS, else `XDG_DATA_HOME/oscairn` or `~/.local/share/oscairn`) |
| An installed library's | `Libraries/<library ID>/Setlists/<name>.osetlist/setlist.json` |
| In an [archive](library-archive.md#setlists) | `Setlists/<name>.osetlist/setlist.json`, listed in `library.json`'s `items` with `kind` `setlist` |

The folder's name is only a name: the library finds a setlist by the `id` in its info block, and
names a new folder after the setlist (letters, digits, spaces, `-` and `_`; others become `_`).

## The document

```json
{"format":"oscairn.setlist","version":2,"info":{"id":"7d0c5e2a9b13f4c8","name":"Friday","description":"Two sets at the Crown","tags":["live"],"color":"","revision":"","parent":"","hash":"","origin":null},
 "display": {"mode": "grid", "columns": 4, "rows": 3},
 "pages": [
  {"name": "Set 1", "slots": [
    {"id": "3f9c0a1b2d4e5f60", "name": "Bass Split", "color": "#e5484d", "label": ""},
    null,
    {"id": "a81f00c2d9e4b736", "name": "Strings Pad", "color": "", "label": "Opener"}
  ]},
  {"name": "Encore", "slots": [
    {"id": "", "name": "Piano Ballad (CP80)", "color": "", "label": ""},
    {"id": "3f9c0a1b2d4e5f60", "name": "Bass Split", "color": "", "label": "Bass again"}
  ]}
 ]}
```

| Field | Meaning |
| --- | --- |
| `format`, `version` | Exactly `oscairn.setlist` and the integer `2`; any other is refused. Version 1 (the Setlist app's own files, below) is not read |
| `info` | As a Performance's ([format](performance-format.md)): `id` (1 to 64 bytes; any text, a library's own are 16 hex digits), `name` (at most 256 bytes), `description` (at most 4096), `tags` (at most 32, each 1 to 64 bytes), `color` (`#rrggbb` or empty), `revision`, `parent`, `hash` (each text, empty allowed) and `origin` (`null`, or `{library,item,hash}` for a copy installed from an archive). Every member is required |
| `display` | How the Setlist app shows it: `mode` is `list` (a list with Previous and Next), `scroll` (a list to scroll and tap) or `grid` (each page as `columns` × `rows` cells, with Previous and Next). `columns` and `rows` are integers 1 to 8, required in every mode |
| `pages` | 1 to 100 pages, in playing order: each `{name, slots}`, its name at most 256 bytes (empty allowed) |
| `slots` | A page's slots, in playing order; at most 1000 in the whole setlist. Each is `null` (an empty slot) or `{id, name, color, label}` |

A slot's members, all four required, all text:

| Member | Meaning |
| --- | --- |
| `id` | The Performance's library ID (at most 64 bytes); empty for a **placeholder** |
| `name` | The Performance's name when last seen, shown while it is not in the library; a placeholder's name (required, not empty) |
| `color` | `#rrggbb` to show the slot in that color in this setlist instead of the Performance's, or empty |
| `label` | Shown instead of the Performance's name when not empty (at most 256 bytes) |

- **An empty slot** (`null`) keeps a grid position: a page shows its slots in order, so in a grid
  the slot after an empty one stays in its cell. Previous and Next skip empty slots.
- **A placeholder** (`id` empty, `name` given) stands for a Performance that is not supplied, such
  as one a converter could not make. It is shown by its label, else its name, and does not open.
- **A slot whose Performance is not in the library** stays in place, shown by its label, else its
  name, marked as not in the library, and does not open. Once that Performance is installed, it
  opens.
- The same Performance may be in several slots.
- A reader refuses a member it does not know, at every level, and a missing one.

### Writing one by hand

- Write `info` straight after `"version":2`, exactly as `,"info":{` with no spaces: the library
  edits the info block in place and finds it by that text (the example's first line). The rest
  may be laid out freely.
- `revision`, `parent` and `hash` may be empty: the library makes them when it saves or copies the
  setlist. `origin` is `null`.
- UTF-8, at most 1 MiB. Names count in bytes, not characters.
- To change the setlist through the control API, `setlists.save` takes the same document; a
  request is at most 64 KiB, so a setlist written that way is smaller than that.

## Version 1

Before setlists were library items, the Setlist app kept one file per setlist in the data
folder's `Setlists/`, `<name>.json`: `{"format":"oscairn.setlist","version":1,"name","display","entries"}`,
each entry `{id,name,color}`. The engine converts each such file when it starts: one page,
`Page 1`, holding the entries in order, or, for a grid, a page per screen of `columns` × `rows`
(`Page 1`, `Page 2`...; screens are put together where there would be more than 100 pages); each
entry a slot with no label. The converted setlist is a new library item with a new ID, and the old
file is removed. A file that cannot be read is left where it is and said on the engine's standard
error.

Multi-device setlists ([RFC 0012](rfcs/0012-sync-external-midi-and-multi-device.md)) may replace
this format.
