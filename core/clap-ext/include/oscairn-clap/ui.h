/* SPDX-License-Identifier: MIT */
#pragma once
#include <clap/clap.h>

#ifdef __cplusplus
extern "C" {
#endif

/* oscairn.ui (draft): how a plugin's parameters are best shown and played, for hosts that
 * draw editors themselves on screens of any size (Oscairn's RFC 0019) rather than open the
 * plugin's window. A hint per parameter: the control it wants, and a priority, which decides
 * what a small screen shows first and the order physical knobs take parameters in.
 *
 * Parameters that belong together (an envelope's four stages) share a group: a host draws
 * them joined, edge to edge in their priority order, and keeps them together when the screen
 * is too narrow for all of them in a row.
 *
 * A parameter may also name its role (cutoff, attack, an effect's mix): what it does in terms
 * every instrument shares, so a host can put it on the same physical knob whatever the plugin
 * (Oscairn's role banks, RFC 0011). Oscairn guesses roles from the parameters' names for a plugin
 * that names none, and only then; where no role fills a bank and the part has no controls of its
 * own, knobs go by priority.
 *
 * A parameter may also be part of a rich view, which a host draws over the parameters' values
 * and edits by setting them (an envelope's curve, a key map, an FM operator view): the
 * parameters with the same nonzero view are one view, wherever their modules are, and each
 * names its field in it (its place: an envelope's attack, a map's breakpoint, an operator's
 * level). A view is data: it says which parameters make it and how to read them, never how to
 * draw it. What a view is follows from its fields:
 *
 * - An FM operator view: any view with OSCAIRN_FIELD_ALGORITHM. Its operators are its fields'
 *   indexes (1 to 8): each operator's level, ratio and so on, and its envelope and key scaling
 *   as envelope and map fields with the operator's index. algorithm() gives each algorithm's
 *   routing, so the host draws it.
 * - An envelope: envelope fields (DELAY to RELEASE, or LEVEL_n with TIME_n or RATE_n), one
 *   envelope per index. Named stages are an ADSR: from zero, a delay, an attack to full, a
 *   hold, a decay to the sustain level, held until the note ends, and a release to zero; any
 *   but attack and release may be missing (no decay: the attack reaches the sustain level). A
 *   multi-point envelope (DX7, AWM2, FM-X) runs segments 1 to n, the highest with a time or
 *   rate: segment k takes TIME_k (or RATE_k, faster when higher) to reach LEVEL_k; a segment
 *   without a level holds the level it starts at (a hold), except the last, which falls to
 *   zero. It starts at LEVEL_0, or without one at the last segment's level (DX7's level 4), or
 *   zero; it holds at the end of segment n - 1 until the note ends, and segment n is its
 *   release.
 * - A key or velocity map: map fields. Each index is a zone (an element's or a layer's key
 *   and velocity range, 1 to 32); the breakpoint, depths and curves scale a level across the keys
 *   (DX7's keyboard level scaling, with the curves' value texts saying their shape: "-LIN",
 *   "+EXP"); velocity depth and curve scale it by velocity. A host lays key parameters across
 *   their ranges, as their value texts name the keys.
 *
 * A host draws a view where its parameters' tiles are, beside them: views add a way to edit,
 * they never take a parameter's tile or physical control away.
 *
 * A plugin may also name and order its own pages and tabs: two levels, as a host draws them (a
 * first level of pages, and under each a second level, a row of tabs), each with a display
 * name, and say which page or tab each parameter is on, whatever its CLAP module path and its
 * place in the parameters' order. The host draws tabs in one style of its own, themed with the
 * rest of its interface: a plugin says what the tabs are, never how they look. A plugin that
 * declares none, and a parameter it leaves out, get the host's own pages and tabs from the
 * module paths: the first level of a path is a page, the second a tab.
 *
 * A stepped parameter's values may each name an icon from a closed set of the host's own
 * (a waveform's shape, for an oscillator's or an LFO's Wave): the host draws the icon in its
 * own style, beside or instead of the value's text. A plugin gives no picture, only which icon;
 * a host that does not know it, or a value without one, draws as it would without.
 *
 * Hints only shape the editor; every parameter stays reachable whatever its hint. A host
 * that does not know this extension ignores it, and a parameter without a hint is drawn as
 * the host decides from its metadata (stepped values, ranges, module path).
 *
 * param_hint is main-thread; the host asks it when it reads parameter info (at every activation
 * and after a rescan of the parameters' info), so hints may change only with that info. Oscairn
 * ignores this extension when param_hint is null.
 *
 * Draft 4 added the view, field and index at the end of the hint, and algorithm(); draft 5
 * added seconds(); draft 6 added tab_count(), tab(), param_tab() and value_icon(), and left the
 * hint as it was. Oscairn still reads a draft 5 extension, without tabs and icons (pages and
 * tabs then come from the module paths, and an icon from a value's text where it names a
 * waveform), a draft 4 one, without seconds() (an envelope's times then come from value texts
 * with units, "31 ms" or "1.2 s", or are drawn by each parameter's range), and a draft 3 one,
 * without views. A draft 6 table is a draft 5 table with members after its last, so a plugin
 * may return it for the older IDs as well: a host reads only the members its draft has. Draft 6
 * also lets an element's or a zone's index go to 32 (an operator's stays 1 to 8): a draft 5 host
 * keeps a field only with an index up to 8 and ignores the others, so a view with more elements
 * or zones shows its first eight there, and every parameter stays a tile.
 */
static const char OSCAIRN_EXT_UI[] = "oscairn.ui/draft-6";
static const char OSCAIRN_EXT_UI_DRAFT_5[] = "oscairn.ui/draft-5";
static const char OSCAIRN_EXT_UI_DRAFT_4[] = "oscairn.ui/draft-4";
static const char OSCAIRN_EXT_UI_DRAFT_3[] = "oscairn.ui/draft-3";

enum oscairn_widget
{
    OSCAIRN_WIDGET_AUTO = 0,    /* the host decides */
    OSCAIRN_WIDGET_SLIDER = 1,
    OSCAIRN_WIDGET_KNOB = 2,
    OSCAIRN_WIDGET_BUTTONS = 3, /* stepped: each value a button, side by side */
    OSCAIRN_WIDGET_SWITCH = 4,  /* stepped, two values: off and on */
    OSCAIRN_WIDGET_LIST = 5,    /* stepped: a list of the values' texts */
    OSCAIRN_WIDGET_RANGE = 6,   /* the low end of a range; range_high names its high end, and
                                   both are one two-sided control */
};

/* Roles, in the order of the role banks they fill (Filter, Amp EG, EQ, Levels). */
enum oscairn_role
{
    OSCAIRN_ROLE_NONE = 0,
    OSCAIRN_ROLE_CUTOFF = 1,
    OSCAIRN_ROLE_RESONANCE = 2,
    OSCAIRN_ROLE_FILTER_ENVELOPE = 3, /* the filter envelope's amount */
    OSCAIRN_ROLE_KEY_TRACKING = 4,    /* the filter's */
    OSCAIRN_ROLE_FILTER_ATTACK = 5,
    OSCAIRN_ROLE_FILTER_DECAY = 6,
    OSCAIRN_ROLE_FILTER_SUSTAIN = 7,
    OSCAIRN_ROLE_FILTER_RELEASE = 8,
    OSCAIRN_ROLE_ATTACK = 9,          /* the amplitude envelope's stages */
    OSCAIRN_ROLE_DECAY = 10,
    OSCAIRN_ROLE_SUSTAIN = 11,
    OSCAIRN_ROLE_RELEASE = 12,
    OSCAIRN_ROLE_HOLD = 13,
    OSCAIRN_ROLE_VELOCITY = 14,       /* velocity's effect on level */
    OSCAIRN_ROLE_AMP_KEY_TRACKING = 15,
    OSCAIRN_ROLE_LEVEL = 16,          /* the instrument's output level */
    OSCAIRN_ROLE_LOW_GAIN = 17,       /* EQ */
    OSCAIRN_ROLE_MID_GAIN = 18,
    OSCAIRN_ROLE_HIGH_GAIN = 19,
    OSCAIRN_ROLE_MID_FREQUENCY = 20,
    OSCAIRN_ROLE_LOW_FREQUENCY = 21,
    OSCAIRN_ROLE_HIGH_FREQUENCY = 22,
    OSCAIRN_ROLE_MID_Q = 23,
    OSCAIRN_ROLE_EQ_OUTPUT = 24,
    OSCAIRN_ROLE_LEVEL_1 = 25,        /* element, operator or oscillator levels, 1 to 8 */
    OSCAIRN_ROLE_LEVEL_8 = 32,
    OSCAIRN_ROLE_MIX = 33,            /* an effect's main control: its mix, or its amount */
    OSCAIRN_ROLE_PAN = 34,
    OSCAIRN_ROLE_OUTPUT_BUS = 35,     /* stepped, from 0: the output port a sound plays on, by its index (0 the main port); a host
                                         sharing one instance among parts gives each part's sound its own (RFC 0021) */
};

/* A parameter's place in its rich view. Values a host does not know mean NONE. */
enum oscairn_field
{
    OSCAIRN_FIELD_NONE = 0,
    /* An envelope's named stages: times, and the sustain level. */
    OSCAIRN_FIELD_DELAY = 1,
    OSCAIRN_FIELD_ATTACK = 2,
    OSCAIRN_FIELD_HOLD = 3,
    OSCAIRN_FIELD_DECAY = 4,
    OSCAIRN_FIELD_SUSTAIN = 5,
    OSCAIRN_FIELD_RELEASE = 6,
    /* A multi-point envelope: levels 0 (the start) to 8, and each segment's time or rate. */
    OSCAIRN_FIELD_LEVEL_0 = 16,
    OSCAIRN_FIELD_LEVEL_8 = 24,
    OSCAIRN_FIELD_TIME_1 = 32,
    OSCAIRN_FIELD_TIME_8 = 39,
    OSCAIRN_FIELD_RATE_1 = 40,  /* a rate: the segment is faster when it is higher */
    OSCAIRN_FIELD_RATE_8 = 47,
    /* A key or velocity map. */
    OSCAIRN_FIELD_KEY_LOW = 64, /* a zone's lowest key */
    OSCAIRN_FIELD_KEY_HIGH = 65,
    OSCAIRN_FIELD_VELOCITY_LOW = 66,
    OSCAIRN_FIELD_VELOCITY_HIGH = 67,
    OSCAIRN_FIELD_BREAKPOINT = 68, /* the key level scaling turns at */
    OSCAIRN_FIELD_LEFT_DEPTH = 69, /* how far the level changes below the breakpoint */
    OSCAIRN_FIELD_RIGHT_DEPTH = 70,
    OSCAIRN_FIELD_LEFT_CURVE = 71, /* stepped: its value text says the shape ("-LIN", "+EXP") */
    OSCAIRN_FIELD_RIGHT_CURVE = 72,
    OSCAIRN_FIELD_VELOCITY_DEPTH = 73, /* how much velocity changes the level */
    OSCAIRN_FIELD_VELOCITY_CURVE = 74,
    /* An FM operator view. */
    OSCAIRN_FIELD_ALGORITHM = 96, /* stepped, at most 128 values; the view's own (index 0) */
    OSCAIRN_FIELD_FEEDBACK = 97,
    OSCAIRN_FIELD_OPERATOR_LEVEL = 98, /* an operator's output level */
    OSCAIRN_FIELD_RATIO = 99,          /* its frequency ratio, or the coarse part when FINE is its own */
    OSCAIRN_FIELD_FINE = 100,
    OSCAIRN_FIELD_DETUNE = 101,
    OSCAIRN_FIELD_FIXED = 102,         /* two values: ratio, or a fixed frequency */
};

/* The icons a value may name: a waveform's shape. Values a host does not know mean NONE. */
enum oscairn_icon
{
    OSCAIRN_ICON_NONE = 0,
    OSCAIRN_ICON_SINE = 1,
    OSCAIRN_ICON_TRIANGLE = 2,
    OSCAIRN_ICON_SAW = 3,           /* a ramp up, then a drop */
    OSCAIRN_ICON_SAW_DOWN = 4,      /* a ramp down, then a jump up; the same wave as SAW upside down */
    OSCAIRN_ICON_SQUARE = 5,        /* half high, half low */
    OSCAIRN_ICON_PULSE = 6,         /* a narrow pulse, high for a quarter of the cycle or less */
    OSCAIRN_ICON_NOISE = 7,
    OSCAIRN_ICON_SAMPLE_AND_HOLD = 8, /* random steps */
};

typedef struct oscairn_param_hint
{
    uint32_t widget;    /* enum oscairn_widget; values a host does not know mean AUTO */
    uint32_t priority;  /* 1 is the most important; 0 is none, after every numbered one */
    clap_id range_high; /* OSCAIRN_WIDGET_RANGE: the range's high parameter; else CLAP_INVALID_ID */
    uint32_t group;     /* parameters with the same nonzero group, in one module, are drawn joined */
    uint32_t role;      /* enum oscairn_role; values a host does not know mean NONE */
    /* Draft 4: */
    uint32_t view;      /* nonzero: the rich view this parameter is part of, with every parameter of the same view */
    uint32_t field;     /* enum oscairn_field: its place in the view */
    uint32_t index;     /* whose field it is: an operator's, 1 to 8; an element's or a zone's, 1 to 32 (draft 6; a draft 5
                           host draws only 1 to 8 and ignores a field with a larger index); 0: the view's own */
} oscairn_param_hint_t;

/* One algorithm of an FM operator view: which operators modulate which, and which are heard.
 * Operators are numbered from 1; bit k of a mask is operator k + 1. */
typedef struct oscairn_fm_algorithm
{
    uint8_t inputs[8];     /* the operators that modulate operator n + 1 */
    uint8_t carriers;      /* the operators heard at the output */
    uint8_t feedback_from; /* feedback runs from this operator back into feedback_to; 0: none */
    uint8_t feedback_to;   /* the same operator: self feedback */
} oscairn_fm_algorithm_t;

/* One page or tab of the editor, from tab(). Ids are the plugin's own and nonzero. */
typedef struct oscairn_ui_tab
{
    uint32_t id;     /* unique among all the entries */
    uint32_t parent; /* 0: a page; else the id of a page, and this is a tab under it */
    char name[64];   /* UTF-8, NUL-terminated: the display name, at most 63 bytes */
} oscairn_ui_tab_t;

typedef struct oscairn_plugin_ui
{
    /* [main-thread] Fill `hint` for the parameter `param_id`; false when it has none. */
    bool(CLAP_ABI* param_hint)(const clap_plugin_t* plugin, clap_id param_id, oscairn_param_hint_t* hint);
    /* [main-thread] Draft 4, may be null: fill `algorithm` with the routing the algorithm
     * parameter `param_id` (OSCAIRN_FIELD_ALGORITHM) has at the value `value`; false when it has
     * none. Asked with the hints, for every value of the parameter. */
    bool(CLAP_ABI* algorithm)(const clap_plugin_t* plugin, clap_id param_id, uint32_t value, oscairn_fm_algorithm_t* algorithm);
    /* [main-thread] Draft 5, may be null: set `seconds` to how long the envelope stage of the
     * time or rate parameter `param_id` (DELAY, ATTACK, HOLD, DECAY, RELEASE, TIME_n or RATE_n)
     * takes at the value `value`, any value in its range; for a rate, the time a segment across
     * the envelope's whole range takes; false when it has none. Asked with the hints, at each
     * value of a stepped parameter of at most 128 values, else at 33 evenly spaced values, so the
     * host draws stages by time; it drops them all if one is missing. Approximate is enough. */
    bool(CLAP_ABI* seconds)(const clap_plugin_t* plugin, clap_id param_id, double value, double* seconds);
    /* [main-thread] Draft 6, may be null: how many entries tab() has, pages and tabs together;
     * 0 declares none. Asked with the hints. The host reads at most 16 pages and 16 tabs under
     * each, and skips any entry past them. */
    uint32_t(CLAP_ABI* tab_count)(const clap_plugin_t* plugin);
    /* [main-thread] Draft 6, may be null: fill `tab` with entry `index`, below tab_count(); false
     * when it has none. The pages are the entries with parent 0, in the order they come; a page's
     * tabs are the entries naming it as parent, in the order they come, wherever they are among
     * the others. A tab under anything but a page, an id used twice and an empty name leave an
     * entry out. A page with fewer than two tabs is drawn without them. Asked with the hints. */
    bool(CLAP_ABI* tab)(const clap_plugin_t* plugin, uint32_t index, oscairn_ui_tab_t* tab);
    /* [main-thread] Draft 6, may be null: set `tab_id` to the page or tab the parameter `param_id`
     * is on, the id of an entry of tab(); false when it has none, and the host puts it where its
     * module path does. A parameter on a page itself, not a tab, is drawn on that page above its
     * tabs, whichever tab is open; one on a tab is drawn on that tab only. The host sets no limit
     * on how many parameters a plugin puts on a page itself: they take room from the tabs' area on
     * a small screen (a host may scroll the page), and keeping them few is the plugin's lookout. A view is drawn once,
     * on the first of its parameters' tabs in the order of tab(), beside the tiles there; its
     * parameters stay on their own tabs. Asked with the hints, for every parameter. */
    bool(CLAP_ABI* param_tab)(const clap_plugin_t* plugin, clap_id param_id, uint32_t* tab_id);
    /* [main-thread] Draft 6, may be null: set `icon` (enum oscairn_icon) to the icon of the stepped
     * parameter `param_id` at `value`: its plain value, the same number the plugin's value_to_text
     * takes (a stepped parameter's integers, from its own minimum, which may be negative); false
     * when it has none. Asked with the hints, only for stepped parameters of at most 128 values,
     * at each of their step values. Where any value has an icon the host draws no icons it
     * guesses for the others. */
    bool(CLAP_ABI* value_icon)(const clap_plugin_t* plugin, clap_id param_id, double value, uint32_t* icon);
} oscairn_plugin_ui_t;

#ifdef __cplusplus
}
#endif
