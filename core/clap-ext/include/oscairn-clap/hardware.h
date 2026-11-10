/* SPDX-License-Identifier: MIT */
#pragma once
#include <clap/clap.h>
#ifdef __cplusplus
extern "C" {
#endif

/* oscairn.hardware (draft): a plugin's own backends for a device's accelerators, such as a DSP,
 * an FPGA region, a dedicated sound chip or a real-time core (docs/rfcs/0004-hardware-services.md).
 *
 * The host lists the accelerators the device offers, as its capability records describe them: the
 * service ABI each speaks, the operations, firmware and banks it actually has, and its capacity.
 * A plugin with a backend for one of them reserves what it needs, submits work while it
 * processes, collects the results, and keeps its CPU path for everything else: a device without
 * the accelerator, a refused reservation, offline rendering, and an accelerator that fails while
 * playing. The host never runs plugin code on the accelerator. The plugin's backend asks for the
 * operations the accelerator's service ABI defines, through buffers the host owns and validates,
 * never registers or device memory. Nothing of an accelerator belongs in the plugin's state.
 *
 *  1. [main] count() and get() list the accelerators; supports() says whether one offers an
 *     operation. The plugin picks one whose abi it has a backend for. One with available false is
 *     in the device's profile but cannot be used (no service for it in this build, its firmware
 *     missing, or failed).
 *  2. [main] reserve(), before the plugin's activate returns. The host counts capacity across
 *     every plugin; an exclusive region is held by one lease at a time. A refusal says why, and
 *     the plugin uses its CPU path. cpu_fallback_us asks the host to reserve, beside the
 *     accelerator, the CPU time the plugin's CPU path takes, so that path can take over without
 *     an interruption; lease.cpu_fallback says whether it could.
 *  3. [audio] submit() a job for the block being processed, then collect() its result. With
 *     pipeline_frames 0 the result is ready in the same process call. With more, collect()
 *     returns OSCAIRN_HARDWARE_PENDING until a later call, and the plugin reports that delay
 *     through clap.latency. One job per lease is outstanding at a time. Neither call allocates or
 *     waits for the accelerator beyond a bounded time.
 *  4. On OSCAIRN_HARDWARE_LATE the result is discarded: the plugin renders that block on its CPU
 *     path, or mutes it. On OSCAIRN_HARDWARE_FAILED the lease is lost until it is released: with
 *     lease.cpu_fallback the plugin continues on its CPU path from that block; without it the
 *     plugin mutes, and the host reports the interruption. The host records every failure for the
 *     device's status (Oscairn: the control API's hardware.capabilities).
 *  5. [main] release(), at the latest in the plugin's deactivate. The host releases what a plugin
 *     still holds when it destroys it.
 *
 * A job's time is the plugin's own timeline: clap_process.steady_time plus the frame the job
 * starts at. Each activation begins a new epoch; a job submitted before it collects as
 * OSCAIRN_HARDWARE_STALE_EPOCH.
 *
 * An operation is named "<abi family>.<name>/<version>" and its job's input and output bytes are
 * defined by the service ABI, not by this header. Backends of one engine share its parameters
 * and state, and must match its CPU path within the engine's stated tolerance; an audibly
 * different algorithm is a separate saved mode, not a backend.
 *
 * (Oscairn: a host without accelerators, every desktop and every product without one, offers
 * this extension with count() 0. A plugin in a worker process (RFC 0010) sees count() 0 for now.)
 */
static const char OSCAIRN_EXT_HARDWARE[] = "oscairn.hardware/draft";

enum oscairn_hardware_status {
    OSCAIRN_HARDWARE_OK = 0,
    OSCAIRN_HARDWARE_PENDING = 1,          /* collect: not ready yet, ask again in a later process call */
    OSCAIRN_HARDWARE_LATE = 2,             /* missed its deadline; the result is discarded */
    OSCAIRN_HARDWARE_FAILED = 3,           /* the accelerator failed; the lease is lost */
    OSCAIRN_HARDWARE_STALE_EPOCH = 4,      /* submitted before the current activation */
    OSCAIRN_HARDWARE_INVALID = 5,          /* not this plugin's lease, an unknown operation, a buffer too large */
    OSCAIRN_HARDWARE_BUSY = 6,             /* submit: the lease's previous job is not collected */
    OSCAIRN_HARDWARE_NOTHING = 7,          /* collect: no job submitted */
    /* reserve's refusals */
    OSCAIRN_HARDWARE_UNAVAILABLE = 16,     /* no service for it in this build, its firmware missing, or failed */
    OSCAIRN_HARDWARE_UNSUPPORTED = 17,     /* it lacks the operation, bank or region asked for */
    OSCAIRN_HARDWARE_OVER_CAPACITY = 18,   /* not enough voices, parts or memory left */
    OSCAIRN_HARDWARE_REGION_BUSY = 19,     /* the exclusive region is held by another lease */
    OSCAIRN_HARDWARE_UNKNOWN_CAPACITY = 20 /* its capacity is not known, so the host does not promise a fit */
};

/* A capacity the device's capability record does not state. */
#define OSCAIRN_HARDWARE_UNKNOWN UINT32_MAX
#define OSCAIRN_HARDWARE_UNKNOWN_BYTES UINT64_MAX

typedef struct oscairn_hardware_accelerator {
    char id[64];               /* the accelerator's ID in the device's capability record ("dsp0") */
    char kind[32];             /* "dsp", "fpga", "sound-chip", "realtime-core" or "simulated" */
    char abi[64];              /* the service ABI it speaks, "<family>/<version>"; what a backend is written for */
    char firmware[64];         /* the installed firmware's identity; empty when it has none */
    char firmware_version[32];
    uint32_t voices;           /* capacity, before any reservation; OSCAIRN_HARDWARE_UNKNOWN when not stated */
    uint32_t parts;
    uint64_t memory_bytes;     /* its own memory; OSCAIRN_HARDWARE_UNKNOWN_BYTES when not stated */
    uint32_t pipeline_frames;  /* results are this many frames after their input; 0: in the same process call */
    bool offline;              /* it can render faster than real time */
    bool available;            /* false: listed, but reserve() refuses it as OSCAIRN_HARDWARE_UNAVAILABLE */
} oscairn_hardware_accelerator_t;

typedef struct oscairn_hardware_request {
    const char* operation;     /* required: an operation the accelerator offers */
    const char* bank;          /* a resident bank the backend needs, or NULL */
    const char* region;        /* an exclusive region the backend needs, or NULL */
    uint32_t voices;
    uint32_t parts;
    uint64_t memory_bytes;
    uint32_t max_input_bytes;  /* the largest job input and output; the host's transfer buffers are this large */
    uint32_t max_output_bytes;
    uint32_t cpu_fallback_us;  /* a block's time on the plugin's CPU path, reserved as its fallback; 0: none */
} oscairn_hardware_request_t;

typedef struct oscairn_hardware_lease {
    uint32_t id;               /* nonzero */
    uint32_t pipeline_frames;
    bool cpu_fallback;         /* the CPU time for the fallback is reserved */
} oscairn_hardware_lease_t;

typedef struct oscairn_hardware_job {
    int64_t steady_time;       /* the plugin's timeline position of its first frame */
    uint32_t frames;
    const void* input;         /* copied by submit */
    uint32_t input_bytes;
} oscairn_hardware_job_t;

typedef struct oscairn_hardware_result {
    int64_t steady_time;       /* out: the job's */
    void* output;              /* in: where collect copies the result */
    uint32_t output_bytes;     /* in: the room at output; out: the bytes written */
} oscairn_hardware_result_t;

typedef struct oscairn_host_hardware {
    /* [main] The accelerators, available or not. */
    uint32_t (CLAP_ABI *count)(const clap_host_t* host);
    bool (CLAP_ABI *get)(const clap_host_t* host, uint32_t index, oscairn_hardware_accelerator_t* out);
    /* [main] Whether accelerator index offers operation. */
    bool (CLAP_ABI *supports)(const clap_host_t* host, uint32_t index, const char* operation);
    /* [main] An enum oscairn_hardware_status: OK with *lease filled, or a refusal. */
    int32_t (CLAP_ABI *reserve)(const clap_host_t* host, uint32_t index,
                                const oscairn_hardware_request_t* request, oscairn_hardware_lease_t* lease);
    /* [main] */
    void (CLAP_ABI *release)(const clap_host_t* host, uint32_t lease);
    /* [audio] An enum oscairn_hardware_status. */
    int32_t (CLAP_ABI *submit)(const clap_host_t* host, uint32_t lease, const oscairn_hardware_job_t* job);
    int32_t (CLAP_ABI *collect)(const clap_host_t* host, uint32_t lease, oscairn_hardware_result_t* result);
} oscairn_host_hardware_t;

#ifdef __cplusplus
}
#endif
