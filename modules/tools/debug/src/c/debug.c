/* digikit/debug: the runtime's event log, its level filter, and the commands that read it (docs/findings/46-digikit-debug.md).
 *
 * The runtime calls the sink (`dkrt_log_sink_set`) with every event it logs, with interrupts masked, in whatever task logs. The sink applies the level
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
 * The tick (once per pass of the UI task, set with the sink) reads the machine type of each of the 16 tracks every 20 ms from the firmware (`dkrt_track_type`) and logs a change (MACHINE_CHANGE). Each pass it also reads the Sound words of the selected track (`dkrt_selected_params`) and logs a changed word (PARAM_CHANGE, level 3). */

#include <dk/core/api.h>
#include <dk/core/bytes.h>
#include <dk/core/status.h>
#include <dk/core/text.h>
#include <dk/platform/file.h>
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

#define E_MSG (-50)
#define E_PROTO (-60)
#define E_PEEK (-63)

#define CMD_FIRST 0x20u
#define CMD_COUNT 3u
#define CMD_LOG 0x20u
#define CMD_PEEK 0x21u
#define CMD_LOGCFG 0x22u

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

/* The ring is module bss, not an `Api.alloc` block: the arena need is then what `dkneeds` reads from the file (docs/design/digikit-dkm.md, "Needs are inferred from the contents"). */
static Ev ring[RING];
static volatile u32 count; /* events appended: the newest sequence number */
static volatile u32
    base; /* sequence numbers up to here were lost before this module loaded (the runtime's small ring overflowed) */
static volatile u32 level_default = L_INFO;
static volatile u8 level_module[MODULES];

/* The card log's state (below). The sink only sets `urgent`. */
static struct {
    int on;            /* the setting; an int for the menu toggle */
    volatile u8 fault; /* 0, CL_FULL or CL_ERROR: no more writes until the log is turned on again */
    volatile u8 urgent; /* a warn or error event arrived since the last wake of the job */
    volatile u8 save;   /* the setting is to be saved by the tick (set by a host op) */
    u8 avail, file, last_file,
        saved; /* `file`: this boot's file (0 a, 1 b); `saved`: last_file = `file` was saved before the first write */
    u8 busy, has_try, has_write, was_urgent;
    u32 writes, written, last_try, last_write, boot_seq, saved_at;
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
    return dkrt_blocks();
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
    if (((flags >> LEVEL_SHIFT) & LEVEL_MASK) <= L_WARN || (flags & F_ERROR))
        cl.urgent = 1;
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
    n = dkrt_peek_range(0u, r);
    if (n > MAX_RANGES)
        n = MAX_RANGES;
    for (i = 0; i < n; i++) {
        dkrt_peek_range(i, r);
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
 * number). Before the first write of a boot the tick saves the setting with this boot's file and waits 3 s, so the card owns the choice. The tick decides when
 * to write (UI task); the I/O job writes. Header (big endian): u32 magic 'DKCL', u16 version 1, u16 header bytes 64, u16 event bytes 32, u16 events,
 * u32 write number (from 1 in this boot), u8 file, 3 bytes 0, u32 oldest seq, u32 newest seq, u32 block clock, u32 ring size, u32 boot number, then zeros. */
#define CL_HEADER 64u
#define CL_SIZE (CL_HEADER + RING * 32u)
#define CL_MAGIC 0x444b434cu
#define CL_KEY "cardlog"
#define CL_BLOCKS_S 1500u
#define CL_GAP (2u * CL_BLOCKS_S)     /* between two writes */
#define CL_QUIET CL_BLOCKS_S          /* since the newest event */
#define CL_MAX_AGE (5u * CL_BLOCKS_S) /* of the oldest event not written */
#define CL_SETTLE                                                                                  \
    (3u *                                                                                          \
     CL_BLOCKS_S) /* from the setting put to the first write: the settings reach the card 2 s after the last change */
#define CL_FULL 2u
#define CL_ERROR 3u
#define CL_LETTER 27u /* the index of the file letter in cl_path */

static u8 cl_raw[CL_SIZE + 15u];
static u8 *cl_buf;
static char cl_path[] = "data/digikit/debug/cardlog-a.bin";
static const dk_api *sdk;
static int32_t job_id;
static int32_t settings_h;

static int32_t cl_save(void)
{
    u8 v[8];
    v[0] = 2u;
    v[1] = cl.on != 0;
    v[2] = cl.last_file;
    v[3] = 0;
    dk_bytes_put32(v + 4, cl.boot_seq);
    if (!cl.avail)
        return DK_E_STATE;
    return dk_settings_put(sdk, settings_h, CL_KEY, v, 8u);
}

/* Save the setting with this boot's file as the last file. */
static int32_t cl_save_boot(void)
{
    cl.last_file = cl.file;
    return cl_save();
}

/* Turning the log on clears full and error. */
static void cl_set_on(int on)
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
static int32_t cl_step(u8 *ctx)
{
    int32_t r;
    (void)ctx;
    if (!cl.io_req)
        return DK_IO_WAIT;
    if (cl.h == 0) {
        cl_fill();
        r = sdk->file_create(DK_FILES_STORE, cl_path, CL_SIZE, 0, DK_FILE_REPLACE);
        if (r <= 0)
            return cl_finish(r < 0 ? r : DK_E_STATE);
        cl.h = r;
        r = sdk->file_write(cl.h, 0u, cl_buf, CL_SIZE);
        if (r == 0) {
            r = sdk->file_close(cl.h, 0);
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
    if (sdk->file_abort(cl.h) == DK_E_BUSY)
        return DK_IO_MORE;
    cl.h = 0;
    return cl_finish(cl.err);
}

static void cl_done(u8 *ctx, int32_t status)
{
    (void)ctx;
    (void)status;
}

static int cl_retry(int32_t r)
{
    return r == DK_E_BUSY || r == DK_E_FILE_IN_USE;
}

/* In the UI task, from the debug tick: take the result of the job, save the setting, and wake the job when a write is due. */
static void cl_tick(u32 b)
{
    u32 newest, first, newest_at, first_at;
    const volatile Ev *e;
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
            cl.written = cl.newest;
            cl.last_write = b;
            cl.has_write = 1;
        } else if (r == DK_E_FULL) {
            cl.fault = CL_FULL;
            sdk->log((const u8 *)"card log: store full", 0);
        } else if (!cl_retry(r)) {
            cl.fault = CL_ERROR;
        } else if (cl.was_urgent) {
            cl.urgent = 1; /* kept until a write succeeds */
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
    if (!cl.on || cl.fault || cl.busy || newest <= cl.written || !cl.saved ||
        b - cl.saved_at < CL_SETTLE)
        return;
    if (cl.has_try && b - cl.last_try < CL_GAP)
        return;
    e = &ring[(newest - 1u) % RING];
    newest_at = e->seq == newest ? e->time : b;
    first = cl.written + 1u;
    e = &ring[(first - 1u) % RING];
    first_at = e->seq == first ? e->time : b - CL_MAX_AGE;
    if (!cl.urgent && b - newest_at < CL_QUIET && b - first_at < CL_MAX_AGE)
        return;
    cl.was_urgent = cl.urgent;
    cl.urgent = 0;
    cl.busy = 1;
    cl.has_try = 1;
    cl.last_try = b;
    cl.io_req = 1;
    sdk->io_wake(job_id);
}

static void cl_init(const dk_api *api)
{
    u8 v[8];
    if (api->size < sizeof(dk_api))
        return;
    sdk = api;
    cl_buf = (u8 *)(((u32)cl_raw + 15u) & ~15u);
    cl.last_file = 1u;
    settings_h = dk_settings_client(api);
    if (settings_h > 0 && dk_settings_get(api, settings_h, CL_KEY, v, 8u) == 8 && v[0] == 2u) {
        cl.on = v[1] != 0u;
        cl.last_file = v[2] & 1u;
        cl.boot_seq = dk_bytes_get32(v + 4);
    }
    cl.boot_seq++;
    cl.file = cl.last_file ^ 1u;
    cl_path[CL_LETTER] = (char)('a' + cl.file);
    job_id = api->io_post(cl_step, cl_done, 0);
    cl.avail = job_id > 0;
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
static void cl_toggled(void *ctx, int arg)
{
    (void)ctx;
    cl_set_on(arg);
    cl_save();
}

static int cl_put_str(char *s, int n, const char *t)
{
    while (*t)
        s[n++] = *t++;
    return n;
}

static void cl_fill_menu(dk_ui_menu *m, void *ctx)
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
        dk_ui_toggle(m, "CARD LOG", &cl.on, cl_toggled, 0);
    dk_ui_text(m, t);
}

DK_MENU(cl_fill_menu)

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

static int32_t cmd(u32 c, u32 seq, const u8 *body, u32 len, u8 *out)
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
    int32_t t = dkrt_selected_params(now, PARAMS);
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
            dkrt_log_ev(K_PARAM_CHANGE, L_DEBUG << LEVEL_SHIFT, a);
        }
    }
    watch.track = t;
    for (i = 0; i < PARAMS; i++)
        watch.words[i] = now[i];
    if (t != was) { /* a baseline: the host knows from here on that a turn on this track is seen */
        a[0] = (u32)t;
        a[1] = 0xffffffffu;
        a[2] = a[3] = a[4] = 0u;
        dkrt_log_ev(K_PARAM_CHANGE, L_INFO << LEVEL_SHIFT, a);
    }
}

/* The tick of the sink: the runtime calls it once per pass of the UI task, so the module needs no slot (a slot has one owner). */
static void tick(void)
{
    u32 t, a[5];
    u32 b = now();
    cl_tick(b);
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
    if (dkrt_track_type(0u) == -2)
        return; /* no project yet */
    for (t = 0; t < TRACKS; t++) {
        int32_t ty = dkrt_track_type(t);
        if (watch.valid && ty != watch.types[t]) {
            a[0] = t;
            a[1] = (u32)watch.types[t];
            a[2] = (u32)ty;
            a[3] = a[4] = 0u;
            dkrt_log_ev(K_MACHINE_CHANGE, L_INFO << LEVEL_SHIFT, a);
        }
        watch.types[t] = ty;
    }
    watch.valid = 1u;
}

int32_t dkm_init(const dk_api *api)
{
    u32 i;
    watch.track = -1;
    for (i = 0; i < MODULES; i++)
        level_module[i] = LEVEL_NONE;
    level_default = L_INFO;
    if (api->register_cmds(CMD_FIRST, CMD_COUNT, cmd) != 0) {
        api->log((const u8 *)"debug: register_cmds refused", 0);
        return -1;
    }
    cl_init(api);
    dkrt_log_sink_set((uint8_t *)sink, (uint8_t *)tick);
    return 0;
}

void dkm_fini(const dk_api *api)
{
    (void)api;
    dkrt_log_sink_set(0, 0);
    if (job_id > 0)
        sdk->io_cancel(job_id); /* the runtime aborts a session the job leaves open */
    job_id = 0;
}
