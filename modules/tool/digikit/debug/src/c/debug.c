/* digikit/debug: the runtime's event log, its level filter, and the commands that read it (docs/findings/46-digikit-debug.md).
 *
 * The runtime calls the sink (`dk_debug_log_sink_set`) with every event it logs, with interrupts masked, in whatever task logs. The sink applies the level
 * filter and appends to a ring of RING events (static memory). When the sink is set the runtime first hands over the events of its small ring (the boot and the
 * errors so far). An event is 32 bytes: seq (from 1, assigned here), time (the block counter, 1500 per second), kind, flags (bit 0: an error, bits 1 to 3: the
 * level, bits 8 to 15: the source module, record index + 1) and five words. An error event is always kept; the others pass when their level is not above the
 * level of their source module, or the default level.
 *
 *   0x20 LOG     body: u32 from_seq        reply: u32 oldest, u32 newest, u8 n, then n events (32 bytes each, big endian) from `from_seq` or the oldest kept
 *   0x21 PEEK    body: u32 addr, u16 len   reply: u32 addr, u16 len, the bytes (RAM inside the device file's `[digikit] peek` ranges, 1 to 512 bytes)
 *   0x22 LOGCFG  body: u8 op (0 get, 1 set; a set adds u8 default level and 8 bytes, the level of module record 0 to 7, 0xff: the default)
 *                reply: u8 default level, 8 module levels, u8 1 (the debug module is loaded)
 *                op 2 get the card log, op 3 set it (adds u8 on); reply: u8 on, u8 file (0 a, 1 b), u8 state (0 off, 1 on, 2 full, 3 error), u8 0, u32 writes,
 *                i32 last status, u32 ms since the last write (0xffffffff: none yet). The card log writes the ring to a file of the store (see the card log below).
 *
 *   0x1e PROFILE_STATE  body: u8 op (0 get, 1 start, 2 stop, 3 reset, 4 start trace)   reply: u8 state (0 stopped, 1 running), u32 clock Hz, u16 rows, u64 elapsed ticks of the span clock while running since the last reset,
 *                       then the 4 poll slots, each u8 owner (0 empty, 1 + module record, 0xff firmware image) and u32 function address, then u8 trace version (0 unsupported); op 4 also returns u32 generation
 *   0x1f PROFILE_READ   body: u16 first row   reply: u16 first, u8 n, then n rows of u32 calls, u64 sum, u32 max (ticks of the span clock; big endian)
 *                       trace body: u8 version (1), u32 generation (0 any), u16 first event
 *                       trace reply: u8 version, u32 generation, u16 total, u32 dropped, u32 clock Hz, u16 first, u8 n, then n records of u32 owner, u64 start, u32 duration
 *   0x0a SETUP          body: u8 op (1 machine type, 2 filter type), u8 track (0 to 15), u8 type   reply: u8 seq (1 to 255, then from 1 again)
 *                       Queues the request for the next pass of the UI task, which sets the type through the firmware's own kit commit (`dk_debug_setup`, the call a
 *                       chooser makes) and logs SETUP_DONE: op, track, type, seq and the status (0, or a negative code: the firmware kept the old type, or the type is
 *                       not one a chooser offers; DK_E_AGAIN: cancelled by fini with the request queued). DK_E_BUSY while a request is queued, DK_E_STATE until the track watch has its baseline.
 * Start sets the span sink and clock (`dk_span_sink_set`); stop, and fini, clear them. The table has one row per module record, per service phase (3) and per poll (4).
 *
 * The tick (once per pass of the UI task, set with the sink) reads the machine type of each of the 16 tracks every 20 ms from the firmware (`dk_debug_track_type`) and logs a change (MACHINE_CHANGE). Each pass it also reads the Sound words of the selected track (`dk_debug_selected_params`) and logs a changed word (PARAM_CHANGE, level 3). The card log decisions run in the module's own tick (`dk_module.tick`, once per UI pass). */

#include <dk/core/module.h>
#include <dk/core/bytes.h>
#include <dk/core/log.h>
#include <dk/core/sink.h>
#include <dk/core/text.h>
#include <dk/gen/limits.h>
#include <dk/core/time.h>
#include <dk/platform/debug.h>
#include <dk/platform/files.h>
#include <dk/platform/host.h>
#include <dk/platform/io.h>
#include <dk/platform/settings.h>
#include <dk/platform/ui.h>

typedef uint32_t u32;
typedef uint16_t u16;
typedef uint8_t u8;

/* The byte at a device address; the host test reads a fake memory instead. */
#ifndef DEBUG_READ8
#define DEBUG_READ8(a) (*(const volatile u8 *)(uintptr_t)(a))
#endif

/* The span clock counter (a device address); the host test reads a fake counter instead. */
#ifndef DEBUG_READ32
#define DEBUG_READ32(a) (*(volatile u32 *)(uintptr_t)(a))
#endif

#define E_MSG (-50)
#define E_PROTO (-60)
#define E_PEEK (-63)

#define CMD_FIRST 0x20u
#define CMD_COUNT 3u
#define CMD_LOG 0x20u
#define CMD_PEEK 0x21u
#define CMD_LOGCFG 0x22u

/* The profiling commands: the `debug_profile` range of sdk/sdk.toml (PROFILE_STATE, PROFILE_READ). */
/* SETUP: the `debug_setup` range of sdk/sdk.toml. */
#define CMD_SETUP 0x0au

#define CMD_PROFILE_FIRST 0x1eu
#define CMD_PROFILE_COUNT 2u
#define CMD_PROFILE_STATE 0x1eu
#define CMD_PROFILE_READ 0x1fu
/* PROFILE_DSP: the `debug_profile_dsp` range of sdk/sdk.toml, the DSP stats window (docs/plans/profiling.md 13.1). */
#define CMD_PROFILE_DSP 0x1bu
#define DSP_ROWS 16u
#define DSP_WINDOW_WORDS 100u
#define DSP_READ_WORDS                                                                             \
    (DSP_WINDOW_WORDS +                                                                            \
     3u) /* the window, then the mirror's counters: sweeps, gaps, ignored records */

#if !defined(DK_MAX_MODULES)
#error                                                                                             \
    "DK_MAX_MODULES (module records) is not in <dk/gen/limits.h>: generate it from [limits.max] modules in sdk/sdk.toml"
#endif
#define SPAN_MODULES ((u32)DK_MAX_MODULES)
#define SPAN_PHASES 3u /* owner 0x100 | p: the service phases */
#define SPAN_POLLS 4u  /* owner 0x200 | i: the polls */
#define SPAN_ROWS (SPAN_MODULES + SPAN_PHASES + SPAN_POLLS)
#define SPAN_PER_REPLY ((REPLY_MAX - 3u) / 16u)
#define TRACE_VERSION 1u
#define TRACE_CAPACITY 256u
#define TRACE_HEADER 18u
#define TRACE_EVENT 16u
#define TRACE_PER_REPLY ((REPLY_MAX - TRACE_HEADER) / TRACE_EVENT)

#define RING 512u
#define REPLY_MAX 879u /* the reply body of a service command */
#define LOG_PER_REPLY ((REPLY_MAX - 9u) / 32u)
#define PEEK_MAX 512u
#define MAX_RANGES 4u
#define MODULES 8u /* the module records LOGCFG has a level for */

#define F_ERROR 1u
#define LEVEL_SHIFT 1u
#define LEVEL_MASK 7u
#define SOURCE_SHIFT 8u
#define L_WARN 1u
#define L_INFO 2u
#define L_DEBUG 3u
#define LEVEL_NONE 0xffu
#define LEVEL_MAX 4u

#define K_MACHINE_CHANGE 26u
#define K_SETUP_DONE 49u
#define K_PARAM_CHANGE 42u
#define PARAMS                                                                                     \
    101u /* Sound words of the larger device (Digitone II); the runtime zeroes the words a device does not have */
#define TRACKS 16u
#define TRACKS_EVERY 30u  /* blocks: 20 ms */
#define TRACKS_FROM 3000u /* blocks after the first pass: the project exists by then */

typedef struct Ev {
    u32 seq;
    u32 time;
    u16 kind;
    u16 flags;
    u32 a[5];
} Ev;

/* The ring is module bss, not a `dk_alloc` block: the arena need is then what `dkneeds` reads from the file (docs/design/digikit-dkm.md, "Needs are inferred from the contents"). */
static Ev ring[RING];
static volatile u32 count; /* events appended: the newest sequence number */
static volatile u32
    base; /* sequence numbers up to here were lost before this module loaded (the runtime's small ring overflowed) */
static volatile u32 level_default = L_INFO;
static volatile u8 level_module[MODULES];

/* The card log's state (below). The sink only sets `urgent`. */
static struct {
    int32_t on;        /* the setting; an int32_t for the menu toggle */
    volatile u8 fault; /* 0, CL_FULL or CL_ERROR: no more writes until the log is turned on again */
    volatile u8 urgent; /* an incident (warn or error event) arrived since the last tick */
    volatile u8 save;   /* the setting is to be saved by the tick (set by a host op) */
    volatile u32 inc_first,
        inc_last; /* block times of the first and the last incident event since the last tick */
    u8 avail, file, last_file,
        saved; /* `file`: this boot's file (0 a, 1 b); `saved`: last_file = `file` was saved before the first write */
    u8 busy, has_try, has_write;
    u8 kind, cap, stage, has_roll,
        has_inc; /* kind of the write in flight (0 rolling, 1 incident); incident capture: open, stage (1 first write, 2 aftermath) */
    u32 writes, written[2], last_try, last_write, boot_seq, saved_at;
    u32 last_roll, cap_at,
        inc_prev; /* last rolling write; first incident event of the capture; last incident event so far */
    int32_t last_status;
    /* the I/O job (the step and the tick pass these on) */
    volatile u8 io_req, io_done;
    volatile int32_t io_result;
    u32 newest,
        prev_n;     /* newest sequence number copied; events in the buffer after the last copy */
    int32_t h, err; /* the open write session (0: none); the status to report after its abort */
} cl;

static u32 now(void)
{
    return dk_time_blocks();
}

/* The sink: interrupts are masked, so the append needs no lock. */
static void sink(const Ev *e)
{
    u32 flags = e->flags;
    u32 n, i;
    Ev *s;
    if (!(flags & F_ERROR)) {
        u32 limit = level_default;
        u32 src = (flags >> SOURCE_SHIFT) & 0xffu;
        if (src > 0u && src <= MODULES && level_module[src - 1u] != LEVEL_NONE)
            limit = level_module[src - 1u];
        if (((flags >> LEVEL_SHIFT) & LEVEL_MASK) > limit)
            return;
    }
    n = count + 1u;
    if (e->seq > n &&
        count ==
            0u) { /* the runtime's replay starts after lost events: keep its numbering, so the reader sees the gap */
        n = e->seq;
        base = n - 1u;
    }
    s = &ring[(n - 1u) % RING];
    s->seq = 0; /* a reader that is cut off here sees no event, not a half-written one */
    s->time = e->time;
    s->kind = e->kind;
    s->flags = e->flags;
    for (i = 0; i < 5u; i++)
        s->a[i] = e->a[i];
    s->seq = n;
    count = n;
    if (((flags >> LEVEL_SHIFT) & LEVEL_MASK) <= L_WARN || (flags & F_ERROR)) {
        if (!cl.urgent)
            cl.inc_first = e->time;
        cl.inc_last = e->time;
        cl.urgent = 1;
    }
}

/* Copy event `seq` to `o` (32 bytes, big endian). False when it is gone, or was overwritten while it was copied (a writer in an interrupt). */
static int copy_event(u32 seq, u8 *o)
{
    const volatile Ev *e = &ring[(seq - 1u) % RING];
    u32 i, k, f;
    if (e->seq != seq)
        return 0;
    dk_bytes_put32(o, seq);
    dk_bytes_put32(o + 4, e->time);
    k = e->kind;
    f = e->flags;
    o[8] = (u8)(k >> 8);
    o[9] = (u8)k;
    o[10] = (u8)(f >> 8);
    o[11] = (u8)f;
    for (i = 0; i < 5u; i++)
        dk_bytes_put32(o + 12u + 4u * i, e->a[i]);
    return e->seq == seq;
}

static int32_t cmd_log(const u8 *body, u32 len, u8 *out)
{
    u32 from, newest, oldest, n = 0, s;
    if (len < 4u)
        return E_MSG;
    from = dk_bytes_get32(body);
    newest = count;
    oldest = newest > RING ? newest - RING + 1u : (newest ? 1u : 0u);
    if (newest != 0u && oldest <= base)
        oldest = base + 1u;
    dk_bytes_put32(out, oldest);
    dk_bytes_put32(out + 4, newest);
    if (newest != 0u) {
        s = from < oldest ? oldest : from;
        while (s <= newest && n < LOG_PER_REPLY) {
            if (copy_event(s, out + 9u + 32u * n))
                n++;
            s++;
        }
    }
    out[8] = (u8)n;
    return (int32_t)(9u + 32u * n);
}

static int peek_allowed(u32 addr, u32 len)
{
    u32 end = addr + len;
    u32 i, n, r[2];
    if (end < addr)
        return 0;
    n = dk_debug_peek(0u, r);
    if (n > MAX_RANGES)
        n = MAX_RANGES;
    for (i = 0; i < n; i++) {
        dk_debug_peek(i, r);
        if (addr >= r[0] && end <= r[1])
            return 1;
    }
    return 0;
}

static int32_t cmd_peek(const u8 *body, u32 len, u8 *out)
{
    u32 addr, n, i;
    if (len != 6u)
        return E_MSG;
    addr = dk_bytes_get32(body);
    n = ((u32)body[4] << 8) | body[5];
    if (n == 0u || n > PEEK_MAX || !peek_allowed(addr, n))
        return E_PEEK;
    dk_bytes_put32(out, addr);
    out[4] = body[4];
    out[5] = body[5];
    for (i = 0; i < n; i++)
        out[6u + i] = DEBUG_READ8(addr + i);
    return (int32_t)(6u + n);
}

/* ---- the card log ------------------------------------------------------------------------------------------------------------- */

/* The ring goes to a file of the store: 64 bytes of header and RING records of 32 bytes (those of LOG), always the full size. A boot writes one of two files
 * (cardlog-a.bin, cardlog-b.bin) and never the one the boot before wrote (the setting "cardlog": u8 version 2, u8 on, u8 last file, u8 0, u32 boot
 * number). Before the first write of a boot the tick saves the setting with this boot's file and waits 3 s, so the card owns the choice. A boot has a rolling
 * file (cardlog-a.bin) and an incident file (cardlog-a-inc.bin), same format, one staging buffer. The module tick decides when to write (UI task); the I/O job writes.
 * Rolling: when there are new events and no rolling write yet or 60 s since the last one. Incident: an event with the error flag or level warn or lower opens a
 * capture (if none is open and the last incident event is 60 s old or more): write the incident file at once, and once more 10 s after the first incident event.
 * Header (big endian): u32 magic 'DKCL', u16 version 1, u16 header bytes 64, u16 event bytes 32, u16 events,
 * u32 write number (from 1 in this boot), u8 file, u8 kind (0 rolling, 1 incident), 2 bytes 0, u32 oldest seq, u32 newest seq, u32 block clock, u32 ring size, u32 boot number, then zeros. */
#define CL_HEADER 64u
#define CL_SIZE (CL_HEADER + RING * 32u)
#define CL_MAGIC 0x444b434cu
#define CL_KEY "cardlog"
#define CL_BLOCKS_S 1500u
#define CL_GAP (2u * CL_BLOCKS_S)   /* between two writes */
#define CL_ROLL (60u * CL_BLOCKS_S) /* between two rolling writes */
#define CL_BURST                                                                                   \
    (60u * CL_BLOCKS_S) /* incident events this close to the last one do not open a capture */
#define CL_AFTER                                                                                   \
    (10u * CL_BLOCKS_S) /* from the first incident event to the second incident write */
#define CL_SETTLE                                                                                  \
    (3u *                                                                                          \
     CL_BLOCKS_S) /* from the setting put to the first write: the settings reach the card 2 s after the last change */
#define CL_FULL 2u
#define CL_ERROR 3u
#define CL_LETTER 27u /* the index of the file letter in cl_path */

static u8 cl_raw[CL_SIZE + 15u];
static u8 *cl_buf;
static char cl_path[2][40] = {"data/digikit/debug/cardlog-a.bin",
                              "data/digikit/debug/cardlog-a-inc.bin"};
static dk_files *files;
static dk_settings *settings;
static int32_t job_id;

static int32_t cl_save(void)
{
    u8 v[8];
    v[0] = 2u;
    v[1] = cl.on != 0;
    v[2] = cl.last_file;
    v[3] = 0;
    dk_bytes_put32(v + 4, cl.boot_seq);
    if (!cl.avail || !settings)
        return DK_E_STATE;
    return dk_settings_put(settings, CL_KEY, v, 8u);
}

/* Save the setting with this boot's file as the last file. */
static int32_t cl_save_boot(void)
{
    cl.last_file = cl.file;
    return cl_save();
}

/* Turning the log on clears full and error. */
static void cl_set_on(int32_t on)
{
    cl.on = on != 0;
    if (on)
        cl.fault = 0;
}

/* Copy the ring into the buffer. Records after the events copied are zero: the buffer only grows, and `prev_n` clears what a shorter copy leaves. */
static void cl_fill(void)
{
    u32 newest = count, oldest, s, n = 0, i;
    oldest = newest > RING ? newest - RING + 1u : 1u;
    if (oldest <= base)
        oldest = base + 1u;
    for (s = oldest; s <= newest; s++)
        if (copy_event(s, cl_buf + CL_HEADER + 32u * n))
            n++;
    for (i = n * 8u; i < cl.prev_n * 8u; i++)
        dk_bytes_put32(cl_buf + CL_HEADER + 4u * i, 0u);
    cl.prev_n = n;
    cl.newest = newest;
    dk_bytes_put32(cl_buf, CL_MAGIC);
    cl_buf[4] = 0;
    cl_buf[5] = 1;
    cl_buf[6] = 0;
    cl_buf[7] = CL_HEADER;
    cl_buf[8] = 0;
    cl_buf[9] = 32;
    cl_buf[10] = (u8)(n >> 8);
    cl_buf[11] = (u8)n;
    dk_bytes_put32(cl_buf + 12, cl.writes + 1u);
    cl_buf[16] = cl.file;
    cl_buf[17] = cl.kind;
    dk_bytes_put32(cl_buf + 20, oldest);
    dk_bytes_put32(cl_buf + 24, newest);
    dk_bytes_put32(cl_buf + 28, now());
    dk_bytes_put32(cl_buf + 32, RING);
    dk_bytes_put32(cl_buf + 36, cl.boot_seq);
}

static int32_t cl_finish(int32_t r)
{
    cl.io_result = r;
    cl.io_req = 0;
    cl.io_done = 1;
    return DK_IO_WAIT;
}

/* The I/O job: one write of the file per request. After an error with the session open, the next step aborts it and reports the error. */
static int32_t cl_step(void *arg)
{
    int32_t r;
    (void)arg;
    if (!cl.io_req)
        return DK_IO_WAIT;
    if (cl.h == 0) {
        cl_fill();
        r = dk_files_create(files, DK_FILES_STORE, cl_path[cl.kind], CL_SIZE, 0, DK_FILE_REPLACE);
        if (r <= 0)
            return cl_finish(r < 0 ? r : DK_E_STATE);
        cl.h = r;
        r = dk_files_write(files, cl.h, 0u, cl_buf, CL_SIZE);
        if (r == 0) {
            r = dk_files_close(files, cl.h, 0);
            if (r != DK_E_BUSY) /* the session is closed whatever the result */
                cl.h = 0;
            if (r == 0)
                cl.newest = count; /* events of our own store write are not new */
        }
        if (cl.h == 0)
            return cl_finish(r);
        cl.err = r;
        return DK_IO_MORE;
    }
    if (dk_files_abort(files, cl.h) == DK_E_BUSY)
        return DK_IO_MORE;
    cl.h = 0;
    return cl_finish(cl.err);
}

static void cl_done(void *arg, int32_t status)
{
    (void)arg;
    (void)status;
}

static int cl_retry(int32_t r)
{
    return r == DK_E_BUSY || r == DK_E_FILE_IN_USE;
}

/* In the UI task, from the module tick: take the result of the job, save the setting, and wake the job when a write is due. */
static void cl_tick(u32 b)
{
    u32 newest;
    int k = -1;
    if (!cl.avail)
        return;
    if (cl.save) {
        cl.save = 0;
        cl_save();
    }
    if (cl.io_done) {
        int32_t r = cl.io_result;
        cl.io_done = 0;
        cl.busy = 0;
        cl.last_status = r;
        if (r == 0) {
            cl.writes++;
            cl.written[cl.kind] = cl.newest;
            cl.last_write = b;
            cl.has_write = 1;
            if (cl.kind == 0u) {
                cl.last_roll = b; /* incident writes do not reset it */
                cl.has_roll = 1;
            } else if (cl.stage == 1u) {
                cl.stage = 2;
            } else {
                cl.cap = 0;
                cl.stage = 0;
            }
        } else if (r == DK_E_FULL) {
            cl.fault = CL_FULL;
            dk_log(DK_LOG_WARN, "card log: store full", 0);
        } else if (!cl_retry(r)) {
            cl.fault = CL_ERROR;
        }
    }
    if (cl.urgent) {
        u32 first, last;
        cl.urgent = 0;
        first = cl.inc_first;
        last = cl.inc_last;
        if (cl.on) {
            if (!cl.cap && (!cl.has_inc || first - cl.inc_prev >= CL_BURST)) {
                cl.cap = 1; /* a burst, or an incident while a capture is open, adds nothing */
                cl.stage = 1;
                cl.cap_at = first;
            }
            cl.has_inc = 1;
            cl.inc_prev = last;
        }
    }
    if (cl.on && !cl.fault &&
        !cl.saved) { /* the card must say that this boot owns the file before the first write */
        int32_t r = cl_save_boot();
        if (r != 0) {
            cl.fault = CL_ERROR;
            cl.last_status = r;
            return;
        }
        cl.saved = 1;
        cl.saved_at = b;
    }
    newest = count;
    if (!cl.on || cl.fault || cl.busy || !cl.saved || b - cl.saved_at < CL_SETTLE)
        return;
    if (cl.has_try && b - cl.last_try < CL_GAP)
        return;
    if (cl.cap && (cl.stage == 1u || b - cl.cap_at >= CL_AFTER)) {
        if (cl.stage == 2u && newest <= cl.written[1]) {
            cl.cap = 0; /* nothing new since the first write: the capture is done */
            cl.stage = 0;
        } else {
            k = 1;
        }
    }
    if (k < 0 && newest > cl.written[0] && (!cl.has_roll || b - cl.last_roll >= CL_ROLL))
        k = 0;
    if (k < 0)
        return;
    cl.kind = (u8)k;
    cl.busy = 1;
    cl.has_try = 1;
    cl.last_try = b;
    cl.io_req = 1;
    dk_io_wake(job_id);
}

/* Open the services and read the setting (init). The card log stays off (`avail` 0) when the job or the files client is missing. */
static void cl_init(dk_module_ctx *ctx)
{
    u8 v[8];
    cl.last_file = 1u;
    if (dk_settings_open(ctx, &settings) != DK_OK)
        settings = 0;
    if (dk_files_open(ctx, &files) != DK_OK)
        files = 0;
    cl_buf = (u8 *)(((u32)cl_raw + 15u) & ~15u);
    if (settings && dk_settings_get(settings, CL_KEY, v, 8u) == 8 && v[0] == 2u) {
        cl.on = v[1] != 0u;
        cl.last_file = v[2] & 1u;
        cl.boot_seq = dk_bytes_get32(v + 4);
    }
    cl.boot_seq++;
    cl.file = cl.last_file ^ 1u;
    cl_path[0][CL_LETTER] = cl_path[1][CL_LETTER] = (char)('a' + cl.file);
    if (files) {
        job_id = dk_io_post(cl_step, cl_done, 0);
        cl.avail = job_id > 0;
    }
}

/* The reply of op 2 and op 3 (16 bytes). */
static int32_t cl_reply(u8 *out)
{
    u32 d = now() - cl.last_write;
    out[0] = cl.on != 0;
    out[1] = cl.file;
    out[2] = !cl.on ? 0u : cl.fault ? cl.fault : 1u;
    out[3] = 0;
    dk_bytes_put32(out + 4, cl.writes);
    dk_bytes_put32(out + 8, (u32)cl.last_status);
    dk_bytes_put32(out + 12, cl.has_write ? d - d / 3u : 0xffffffffu); /* blocks to ms */
    return 16;
}

/* The menu: DIGIKIT > DEBUG > SETTINGS... */
static void cl_toggled(void *arg, int32_t value)
{
    (void)arg;
    cl_set_on(value);
    cl_save();
}

static int cl_put_str(char *s, int n, const char *t)
{
    while (*t)
        s[n++] = *t++;
    return n;
}

static void menu(dk_module_ctx *ctx, dk_ui_menu *m)
{
    char t[DK_UI_ROW];
    int n = cl_put_str(t, 0, "STATUS ");
    (void)ctx;
    if (!cl.avail) {
        n = cl_put_str(t, n, "N/A");
    } else if (!cl.on) {
        n = cl_put_str(t, n, "OFF");
    } else if (cl.fault == CL_FULL) {
        n = cl_put_str(t, n, "FULL");
    } else if (cl.fault == CL_ERROR) {
        n = cl_put_str(t, n, "ERR ");
        n += (int)dk_text_itoa(t + n, cl.last_status);
    } else {
        n = cl_put_str(t, n, "ON ");
        n += (int)dk_text_utoa(t + n, cl.writes);
        n = cl_put_str(t, n, " WR");
    }
    t[n] = 0;
    if (cl.avail)
        dk_ui_menu_add_toggle(m, "CARD LOG", &cl.on, cl_toggled, 0);
    dk_ui_menu_add_text(m, t);
}

static void span_elapse(void);

/* The module tick (once per UI pass): the elapsed window of the profile, then the card log decisions. */
static void mod_tick(dk_module_ctx *ctx)
{
    (void)ctx;
    span_elapse();
    cl_tick(now());
}

static int32_t cmd_logcfg(const u8 *body, u32 len, u8 *out)
{
    u32 i;
    if (len < 1u)
        return E_MSG;
    if (body[0] == 2u || body[0] == 3u) {
        if (!cl.avail)
            return E_PROTO;
        if (len != (body[0] == 2u ? 1u : 2u))
            return E_MSG;
        if (body[0] == 3u) {
            if (body[1] > 1u)
                return E_PROTO;
            cl_set_on(body[1]);
            cl.save = 1;
        }
        return cl_reply(out);
    }
    if (body[0] == 1u) {
        if (len != 10u)
            return E_MSG;
        if (body[1] > LEVEL_MAX)
            return E_PROTO;
        for (i = 0; i < MODULES; i++)
            if (body[2u + i] > LEVEL_MAX && body[2u + i] != LEVEL_NONE)
                return E_PROTO;
        level_default = body[1];
        for (i = 0; i < MODULES; i++)
            level_module[i] = body[2u + i];
    } else if (body[0] != 0u) {
        return E_PROTO;
    }
    out[0] = (u8)level_default;
    for (i = 0; i < MODULES; i++)
        out[1u + i] = level_module[i];
    out[9] = 1u; /* the debug log is on while this module is loaded */
    return 10;
}

/* ---- the span table (profiling) ------------------------------------------------------------------------------------------------------ */

/* One row per owner id of the runtime's spans: a module record (0 to MAX_MODULES - 1), a service phase (0x100 | p) or a poll (0x200 | i). */
typedef struct SpanRow {
    u32 calls;
    uint64_t sum; /* ticks; 64 bits, so a long run does not wrap */
    u32 max;
} SpanRow;

static SpanRow span_rows[SPAN_ROWS];
static u32 clk_addr, clk_hz; /* the span clock counter (a device address, 0: none) and its rate */
static u8 profiling;
static u8 trace_version;

typedef struct SpanEvent {
    uint64_t start;
    u32 duration;
    u32 owner;
} SpanEvent;

static struct {
    SpanEvent events[TRACE_CAPACITY];
    uint64_t origin;
    u32 generation;
    u32 dropped;
    u16 count;
    u8 present; /* the current profiling window has a trace, running or stopped */
} trace;

/* The UI tick and the host commands share the span clock and table with the IRQ-masked sink. */
static inline u16 span_lock(void)
{
#ifdef __m68k__
    u16 sr;
    __asm__ volatile("move.w %%sr,%0\n\tmove.w #0x2700,%%sr" : "=d"(sr) : : "memory", "cc");
    return sr;
#else
    return 0;
#endif
}

static inline void span_unlock(u16 sr)
{
#ifdef __m68k__
    __asm__ volatile("move.w %0,%%sr" : : "d"(sr) : "memory", "cc");
#else
    (void)sr;
#endif
}

/* The elapsed window: ticks of the span clock while profiling, since the last reset. Extend the wrapping 32-bit counter at every UI pass, PROFILE_STATE
 * and trace event. At least one read per wrap period (about 32.5 s at 132 MHz) keeps it exact. */
static uint64_t el_ticks;
static u32 el_last;

static void span_elapse_locked(void)
{
    u32 n;
    if (!profiling)
        return;
    n = DEBUG_READ32(clk_addr);
    el_ticks += (u32)(n - el_last);
    el_last = n;
}

static void span_elapse(void)
{
    u16 sr;
    if (!profiling)
        return;
    sr = span_lock();
    span_elapse_locked();
    span_unlock(sr);
}

/* Start a generation at the current clock sample, skipping the wildcard generation. */
static void trace_clear(void)
{
    if (++trace.generation == 0u)
        trace.generation = 1u;
    trace.count = 0;
    trace.dropped = 0;
    trace.origin = el_ticks;
}

static void trace_append(const dk_span *s)
{
    uint64_t since_start, elapsed;
    SpanEvent *e;

    if (trace.count == TRACE_CAPACITY)
        goto dropped;

    span_elapse_locked();
    elapsed = el_ticks - trace.origin;
    /* Emission may be delayed after the runtime read the end tick. Unwrap that delay separately from the duration. */
    since_start = (uint64_t)s->cycles + (u32)(el_last - (s->start_tick + s->cycles));
    /* A span can already be in flight when the trace starts or resets. */
    if (since_start > elapsed)
        goto dropped;

    e = &trace.events[trace.count++];
    e->start = elapsed - since_start;
    e->duration = s->cycles;
    e->owner = s->owner;
    return;

dropped:
    if (trace.dropped < UINT32_MAX)
        trace.dropped++;
}

/* Nested spans can complete out of order. Order the frozen capture by start, keeping ties in completion order. */
static void trace_order(void)
{
    u32 i;
    for (i = 1; i < trace.count; i++) {
        SpanEvent e = trace.events[i];
        u32 j = i;
        while (j > 0u && trace.events[j - 1u].start > e.start) {
            trace.events[j] = trace.events[j - 1u];
            j--;
        }
        trace.events[j] = e;
    }
}

static u32 span_row(u32 owner)
{
    if (owner < SPAN_MODULES)
        return owner;
    if ((owner & ~0xffu) == 0x100u && (owner & 0xffu) < SPAN_PHASES)
        return SPAN_MODULES + (owner & 0xffu);
    if ((owner & ~0xffu) == 0x200u && (owner & 0xffu) < SPAN_POLLS)
        return SPAN_MODULES + SPAN_PHASES + (owner & 0xffu);
    return SPAN_ROWS;
}

/* The span sink: interrupts are masked, so the update needs no lock. */
static void span_sink(const dk_span *s)
{
    u32 r = span_row(s->owner);
    SpanRow *w;
    if (!profiling || r >= SPAN_ROWS)
        return;
    w = &span_rows[r];
    w->calls++;
    w->sum += s->cycles;
    if (s->cycles > w->max)
        w->max = s->cycles;
    if (trace.present)
        trace_append(s);
}

/* The runtime reads the clock before and after a span. The module only reads the counter; it never writes a timer register. */
static u32 span_clock(void)
{
    return DEBUG_READ32(clk_addr);
}

static void span_reset(void)
{
    u32 i;
    u16 sr = span_lock();
    el_ticks = 0;
    if (profiling)
        el_last = DEBUG_READ32(clk_addr);
    for (i = 0; i < SPAN_ROWS; i++) {
        span_rows[i].calls = 0;
        span_rows[i].sum = 0;
        span_rows[i].max = 0;
    }
    if (trace.present)
        trace_clear();
    span_unlock(sr);
}

/* The DSP stats window (PROFILE_DSP): on while the DSP counts, and the clear generation last sent. Only the owner of the span channel may control it. */
static u8 dsp_on;
static u32
    dsp_gen; /* starts at the block count of the load: a generation the DSP cannot be holding from an earlier load of the module */

static void span_stop(void)
{
    u16 sr;
    if (dsp_on) {
        dk_dsp_stats_ctl(0, dsp_gen); /* before the channel goes: the control is the owner's */
        dsp_on = 0;
    }
    sr = span_lock();
    span_elapse_locked();
    profiling = 0;
    span_unlock(sr);
    dk_span_sink_set(0, 0);
    if (trace.present)
        trace_order();
}

/* Own the span channel and, when requested, open a generation before a producer can append to it. */
static int32_t span_start(u8 with_trace)
{
    int32_t r = 0;
    u16 sr;
    if (clk_addr == 0u || (with_trace && !trace_version))
        return E_PROTO;
    sr = span_lock();
    if (!profiling) {
        r = dk_span_sink_set((uint8_t *)span_sink, (uint8_t *)span_clock);
        if (r == 0) {
            profiling = 1;
            el_last = DEBUG_READ32(clk_addr);
            trace.present = 0;
        }
    }
    if (r == 0 && with_trace && !trace.present) {
        span_elapse_locked();
        trace_clear();
        trace.present = 1;
    }
    span_unlock(sr);
    return r;
}

static int32_t cmd_profile_state(const u8 *body, u32 len, u8 *out)
{
    const u32 trace_at = 15u + 5u * SPAN_POLLS;
    u32 i, generation;
    uint64_t elapsed;
    u16 sr;
    if (len != 1u)
        return E_MSG;
    if (body[0] == 1u || body[0] == 4u) {
        int32_t r = span_start(body[0] == 4u);
        if (r != 0)
            return r;
    } else if (body[0] == 2u) {
        if (profiling)
            span_stop();
    } else if (body[0] == 3u) {
        span_reset();
    } else if (body[0] > 3u) {
        return E_PROTO;
    }
    sr = span_lock();
    span_elapse_locked();
    elapsed = el_ticks;
    generation = trace.generation;
    out[0] = profiling;
    span_unlock(sr);
    dk_bytes_put32(out + 1, clk_hz);
    out[5] = (u8)(SPAN_ROWS >> 8);
    out[6] = (u8)SPAN_ROWS;
    dk_bytes_put32(out + 7, (u32)(elapsed >> 32));
    dk_bytes_put32(out + 11, (u32)elapsed);
    for (i = 0; i < SPAN_POLLS; i++) {
        u32 fn = 0;
        out[15u + 5u * i] = (u8)dk_debug_poll(i, &fn);
        dk_bytes_put32(out + 16u + 5u * i, fn);
    }
    out[trace_at] = trace_version;
    if (body[0] == 4u) {
        dk_bytes_put32(out + trace_at + 1u, generation);
        return (int32_t)(trace_at + 5u);
    }
    return (int32_t)(trace_at + 1u);
}

static int32_t cmd_trace_read(const u8 *body, u8 *out)
{
    u32 generation = dk_bytes_get32(body + 1u);
    u32 first = ((u32)body[5] << 8) | body[6];
    u32 n, i;
    if (!trace_version || body[0] != TRACE_VERSION)
        return E_PROTO;
    if (profiling || !trace.present || (generation && generation != trace.generation))
        return DK_E_STATE;
    if (first > trace.count)
        return E_PEEK;
    n = trace.count - first;
    if (n > TRACE_PER_REPLY)
        n = TRACE_PER_REPLY;
    out[0] = TRACE_VERSION;
    dk_bytes_put32(out + 1, trace.generation);
    out[5] = (u8)(trace.count >> 8);
    out[6] = (u8)trace.count;
    dk_bytes_put32(out + 7, trace.dropped);
    dk_bytes_put32(out + 11, clk_hz);
    out[15] = body[5];
    out[16] = body[6];
    out[17] = (u8)n;
    for (i = 0; i < n; i++) {
        const SpanEvent *e = &trace.events[first + i];
        u8 *o = out + TRACE_HEADER + TRACE_EVENT * i;
        dk_bytes_put32(o, e->owner);
        dk_bytes_put32(o + 4, (u32)(e->start >> 32));
        dk_bytes_put32(o + 8, (u32)e->start);
        dk_bytes_put32(o + 12, e->duration);
    }
    return (int32_t)(TRACE_HEADER + TRACE_EVENT * n);
}

static int32_t cmd_profile_read(const u8 *body, u32 len, u8 *out)
{
    u32 first, n, i;
    if (len == 7u)
        return cmd_trace_read(body, out);
    if (len != 2u)
        return E_MSG;
    first = ((u32)body[0] << 8) | body[1];
    if (first >= SPAN_ROWS)
        return E_PEEK;
    n = SPAN_ROWS - first;
    if (n > SPAN_PER_REPLY)
        n = SPAN_PER_REPLY;
    out[0] = body[0];
    out[1] = body[1];
    out[2] = (u8)n;
    for (i = 0; i < n; i++) {
        u8 *o = out + 3u + 16u * i;
        u16 sr = span_lock();
        SpanRow w = span_rows[first + i];
        span_unlock(sr);
        dk_bytes_put32(o, w.calls);
        dk_bytes_put32(o + 4, (u32)(w.sum >> 32));
        dk_bytes_put32(o + 8, (u32)w.sum);
        dk_bytes_put32(o + 12, w.max);
    }
    return (int32_t)(3u + 16u * n);
}

/* PROFILE_DSP: body u8 op (0 get, 1 start, 2 stop, 3 clear); the reply is the state after the op (layout: sdk/sdk.toml). Start takes the span channel when the
 * module does not own it yet, as PROFILE_STATE start does; stopping the spans stops the DSP counters. */
static u32 dsp_words[DSP_READ_WORDS];

static int32_t cmd_profile_dsp(u32 c, u32 seq, const void *vbody, u32 len, void *vout)
{
    const u8 *body = vbody;
    u8 *out = vout;
    u32 sweep, i;
    (void)seq;
    if (c != CMD_PROFILE_DSP)
        return E_PROTO;
    if (len != 1u)
        return E_MSG;
    if (body[0] > 3u)
        return E_PROTO;
    if (body[0] == 1u && !dsp_on) {
        int32_t r = span_start(0);
        if (r == 0)
            r = dk_dsp_stats_ctl(1, dsp_gen);
        if (r != 0)
            return r;
        dsp_on = 1;
    } else if (body[0] == 2u && dsp_on) {
        dk_dsp_stats_ctl(0, dsp_gen);
        dsp_on = 0;
    } else if (body[0] == 3u) {
        if (!profiling)
            return DK_E_STATE; /* the control is the span channel owner's: nothing could be sent */
        dsp_gen++; /* the DSP and the mirror drop what they counted when they see the new generation */
        dk_dsp_stats_ctl(dsp_on, dsp_gen);
    }
    sweep = dk_dsp_stats_read(dsp_words, DSP_READ_WORDS);
    out[0] = 1; /* version */
    out[1] = (u8)(dsp_on | (sweep ? 2u : 0u));
    out[2] = (u8)dsp_gen;
    out[3] = (u8)DSP_ROWS;
    dk_bytes_put32(out + 4, sweep);
    for (i = 0; i < 3u; i++)
        dk_bytes_put32(out + 8u + 4u * i, dsp_words[DSP_WINDOW_WORDS + i]);
    if (!sweep)
        return 20;
    for (i = 0; i < DSP_WINDOW_WORDS; i++)
        dk_bytes_put32(out + 20u + 4u * i, dsp_words[i]);
    return (int32_t)(20u + 4u * DSP_WINDOW_WORDS);
}

static int32_t cmd_profile(u32 c, u32 seq, const void *body, u32 len, void *out)
{
    (void)seq;
    if (c == CMD_PROFILE_STATE)
        return cmd_profile_state(body, len, out);
    if (c == CMD_PROFILE_READ)
        return cmd_profile_read(body, len, out);
    return E_PROTO;
}

static int32_t cmd(u32 c, u32 seq, const void *body, u32 len, void *out)
{
    (void)seq;
    if (c == CMD_LOG)
        return cmd_log(body, len, out);
    if (c == CMD_PEEK)
        return cmd_peek(body, len, out);
    if (c == CMD_LOGCFG)
        return cmd_logcfg(body, len, out);
    return E_PROTO;
}

/* ---- SETUP: a track's machine or filter type, set in the UI task ----------------------------------------------------------------- */

static struct {
    volatile u8
        pending; /* set last by the command, cleared by the tick after it logged SETUP_DONE */
    volatile u8
        ready; /* the track watch has its baseline: a project exists and was scanned (set by the tick, cleared by fini) */
    u8 op, track, type, seq;
} setup;

static int32_t cmd_setup(u32 c, u32 seq, const void *vbody, u32 len, void *vout)
{
    const u8 *body = vbody;
    u8 *out = vout;
    (void)seq;
    if (c != CMD_SETUP)
        return E_PROTO;
    if (len != 3u)
        return E_MSG;
    if (body[0] < 1u || body[0] > 2u || body[1] >= TRACKS)
        return E_PROTO;
    if (!setup.ready)
        return DK_E_STATE; /* the project is not loaded yet: the track watch has not taken its baseline (the first scan, 2 s after the first UI pass) */
    if (setup.pending)
        return DK_E_BUSY;
    setup.op = body[0];
    setup.track = body[1];
    setup.type = body[2];
    setup.seq = (u8)(setup.seq == 255u ? 1u : setup.seq + 1u);
    setup.pending = 1;
    out[0] = setup.seq;
    return 1;
}

/* SETUP_DONE for the pending request with `r`, then the request is gone. */
static void setup_done(int32_t r)
{
    u32 a[5];
    a[0] = setup.op;
    a[1] = setup.track;
    a[2] = setup.type;
    a[3] = setup.seq;
    a[4] = (u32)r;
    dk_log_event(K_SETUP_DONE, L_INFO << LEVEL_SHIFT, a);
    setup.pending = 0;
}

/* The request, in the UI task: the type goes through the firmware's own commit; the log says what came of it. The status is 0, DK_E_RANGE (a type no chooser
 * offers), or DK_E_STATE: the firmware kept the old type (the machine permit refused it on that track, or the project's settings mode makes the setter skip the
 * write), the read-back of the type differs from the request, or there is no project. */
static void setup_run(void)
{
    if (setup.pending)
        setup_done(dk_debug_setup(setup.op, setup.track, setup.type));
}

/* ---- the track watch (the sink's tick, once per pass of the UI task) ---------------------------------------------------------------- */

static struct {
    u32 started, next;
    u32 valid;
    int32_t types[TRACKS];
    /* the Sound words of the selected track at the last pass; `track` is -1 until a pass has read them */
    int32_t track;
    int16_t words[PARAMS];
} watch;

/* PARAM_CHANGE (debug level; the baseline is info level, so a host that raises the level right after a restart cannot miss it: its level set can land after the
 * first snapshot): each Sound word of the selected track that differs from the last pass: track, index, old, new. Only writes to that array show (the stock
 * setter: a knob turn); p-lock, sound lock, LFO and MIDI CC take other paths. A change of the selected track, or a track whose record is
 * missing (a kit load), takes the snapshot again and logs a baseline instead: index 0xffffffff, old and new 0. */
static void watch_params(void)
{
    int16_t now[PARAMS];
    u32 i, a[5];
    int32_t was = watch.track;
    int32_t t = dk_debug_selected_params(now, PARAMS);
    if (t < 0) {
        watch.track = -1;
        return;
    }
    if (t == watch.track) {
        for (i = 0; i < PARAMS; i++) {
            if (now[i] == watch.words[i])
                continue;
            a[0] = (u32)t;
            a[1] = i;
            a[2] = (u32)(int32_t)watch.words[i];
            a[3] = (u32)(int32_t)now[i];
            a[4] = 0u;
            dk_log_event(K_PARAM_CHANGE, L_DEBUG << LEVEL_SHIFT, a);
        }
    }
    watch.track = t;
    for (i = 0; i < PARAMS; i++)
        watch.words[i] = now[i];
    if (t != was) { /* a baseline: the host knows from here on that a turn on this track is seen */
        a[0] = (u32)t;
        a[1] = 0xffffffffu;
        a[2] = a[3] = a[4] = 0u;
        dk_log_event(K_PARAM_CHANGE, L_INFO << LEVEL_SHIFT, a);
    }
}

/* The tick of the sink: the runtime calls it once per pass of the UI task, so the module needs no slot (a slot has one owner). */
static void tick(void)
{
    u32 t, a[5];
    u32 b = now();
    setup_run();
    if (!watch.started) {
        watch.started = 1u;
        watch.next = b + TRACKS_FROM;
        return;
    }
    if (watch.valid)
        watch_params();
    if ((int32_t)(b - watch.next) < 0)
        return;
    watch.next = b + TRACKS_EVERY;
    if (dk_debug_track_type(0u) == -2)
        return; /* no project yet */
    for (t = 0; t < TRACKS; t++) {
        int32_t ty = dk_debug_track_type(t);
        if (watch.valid && ty != watch.types[t]) {
            a[0] = t;
            a[1] = (u32)watch.types[t];
            a[2] = (u32)ty;
            a[3] = a[4] = 0u;
            dk_log_event(K_MACHINE_CHANGE, L_INFO << LEVEL_SHIFT, a);
        }
        watch.types[t] = ty;
    }
    watch.valid = 1u;
    setup.ready = 1u;
}

static int32_t init(dk_module_ctx *ctx)
{
    u32 i;
    watch.track = -1;
    for (i = 0; i < MODULES; i++)
        level_module[i] = LEVEL_NONE;
    level_default = L_INFO;
    if (dk_host_register(CMD_FIRST, CMD_COUNT, cmd) != 0) {
        dk_log(DK_LOG_INFO, "debug: dk_host_register refused", 0);
        return DK_E_CMD_RANGE;
    }
    if (dk_host_register(CMD_PROFILE_FIRST, CMD_PROFILE_COUNT, cmd_profile) != 0) {
        dk_log(DK_LOG_INFO, "debug: dk_host_register refused (profile)", 0);
        return DK_E_CMD_RANGE;
    }
    if (dk_host_register(CMD_PROFILE_DSP, 1u, cmd_profile_dsp) != 0) {
        dk_log(DK_LOG_INFO, "debug: dk_host_register refused (profile dsp)", 0);
        return DK_E_CMD_RANGE;
    }
    if (dk_host_register(CMD_SETUP, 1u, cmd_setup) != 0) {
        dk_log(DK_LOG_INFO, "debug: dk_host_register refused (setup)", 0);
        return DK_E_CMD_RANGE;
    }
    clk_addr = dk_debug_span_clock(&clk_hz);
    trace_version =
        clk_addr && clk_hz && dk_module_ctx_runtime_version(ctx) >= DK_SPAN_START_TICK_VERSION
            ? TRACE_VERSION
            : 0;
    dsp_gen = dk_time_blocks();
    cl_init(ctx);
    /* Give a reloaded module a fresh starting generation for any outstanding host page requests. */
    if (trace_version)
        trace.generation = (cl.boot_seq * 2654435761u) ^ span_clock() ^ dsp_gen;
    dk_debug_log_sink_set((uint8_t *)sink, (uint8_t *)tick);
    return DK_OK;
}

static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_debug_log_sink_set(0, 0);
    setup.ready = 0;
    if (setup.pending)
        setup_done(DK_E_AGAIN); /* cancelled: the request never ran */
    if (profiling)
        span_stop();
    if (job_id > 0)
        dk_io_cancel(job_id); /* the runtime aborts a session the job leaves open */
    job_id = 0;
}

const dk_module_def dk_module = {.init = init, .fini = fini, .tick = mod_tick, .menu = menu};
