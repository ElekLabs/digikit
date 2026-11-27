/* Host test of digikit/usb-fast: the real src/c/usb-fast.c (included, so its statics are reachable) on a fake SDK. The fakes stand for what the runtime gives the
 * module: the receive buffers of core/usb-bulk (scripted transfers), the replies it sends, the file calls of the runtime's write session (offset and length rules, the
 * 20 s idle expiry, E_BUSY), the allocators, the block clock, the I/O and helper tasks. Each case runs in its own process (fork): the module keeps its state in statics.
 *
 * What it proves, which the device test used to show: the stream parser (several frames in one transfer, a frame cut at every byte, corrupt streams, a bad request in
 * every state), the nonce of the INFO reply, the speed-switch mask, the staging buffer (sizes around its edges, the overlap halves, a small or no buffer, abort, bus reset)
 * with the bytes the file receives, and the session expiry. What it cannot show: the USB controller, the store and the firmware. Built and run by uf-host.test.ts. */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include "../src/c/usb-fast.c"

static int failed;
#define CHECK(c)                                                                                   \
    do {                                                                                           \
        if (!(c)) {                                                                                \
            printf("%s:%d: check failed: %s\n", __FILE__, __LINE__, #c);                           \
            failed = 1;                                                                            \
        }                                                                                          \
    } while (0)
#define CHECK_EQ(a, b)                                                                             \
    do {                                                                                           \
        long long a_ = (long long)(a), b_ = (long long)(b);                                        \
        if (a_ != b_) {                                                                            \
            printf("%s:%d: %s = %lld, want %lld\n", __FILE__, __LINE__, #a, a_, b_);               \
            failed = 1;                                                                            \
        }                                                                                          \
    } while (0)

/* ---- the fake world ------------------------------------------------------------------------------------------------------------------ */

static u32 now_blocks;
static int ctx_token, files_token, ub_token, sl_token;

/* allocators: the largest block each gives (the arena is small, the firmware's allocator large) */
static u32 arena_max, fw_max;
void *dk_alloc(uint32_t size)
{
    return size <= arena_max ? aligned_alloc(16, (size + 15u) & ~15u) : NULL;
}
void dk_free(void *p)
{
    free(p);
}
void *dk_fw_alloc(uint32_t size)
{
    return size <= fw_max ? aligned_alloc(16, (size + 15u) & ~15u) : NULL;
}
void dk_fw_free(void *p)
{
    free(p);
}
uint32_t dk_time_blocks(void)
{
    return now_blocks;
}
void dk_log(uint32_t level, const char *msg, uint32_t value)
{
    (void)level;
    (void)msg;
    (void)value;
}
void dk_log_event(uint32_t kind, uint32_t flags, const uint32_t *a)
{
    (void)kind;
    (void)flags;
    (void)a;
}

/* the I/O task: the test calls step() itself; the helper task runs its work at once */
static int wakes;
static dk_io_step_fn *job_step;
int32_t dk_io_post(dk_io_step_fn *step, dk_io_done_fn *done, void *arg)
{
    (void)done;
    (void)arg;
    job_step = step;
    return 7;
}
int32_t dk_io_cancel(int32_t id)
{
    (void)id;
    return 0;
}
void dk_io_wake(int32_t id)
{
    (void)id;
    wakes++;
}
static int helper_busy; /* dk_helper_run answers busy: the module copies itself */
int32_t dk_helper_run(dk_helper_fn *work, void *arg)
{
    if (helper_busy)
        return DK_E_BUSY;
    work(arg);
    return 0;
}
int32_t dk_helper_wait(void)
{
    return 0;
}

static dk_host_cmd_fn *host_cmd;
int32_t dk_host_register(uint32_t first, uint32_t count, dk_host_cmd_fn *fn)
{
    CHECK_EQ(first, 0x36);
    CHECK_EQ(count, 2);
    host_cmd = fn;
    return 0;
}

/* the speed levers */
static u32 debug_flags = 5u; /* hash once and the overlapped read-back hash, as at boot */
uint32_t dk_debug_speed_flags(uint32_t clear, uint32_t set)
{
    debug_flags = (debug_flags & ~clear) | set;
    return debug_flags;
}
static int sdhc_present = 1;
static u32 sdhc_switch = 1;
int32_t dk_sdhc_large_open(dk_module_ctx *ctx, dk_sdhc_large **out)
{
    (void)ctx;
    if (!sdhc_present)
        return DK_E_MISSING;
    *out = (dk_sdhc_large *)&sl_token;
    return 0;
}
uint32_t dk_sdhc_large_ctl(dk_sdhc_large *sl_, uint32_t key, uint32_t val)
{
    if (!sl_ || key != 0)
        return 0;
    uint32_t old = sdhc_switch;
    if (val <= 1)
        sdhc_switch = val;
    return old;
}

/* core/usb-bulk: the receive buffers the test queues, the replies the module sends */
typedef struct {
    u8 *data;
    u32 len;
    int released;
} rxbuf;
static rxbuf rx[600];
static int rx_n, rx_next;
static u32 bulk_state;
static int ring_present, send_rc, enables, disables;
static u32 ring_val;
static int double_release;

static void rx_push(const u8 *d, u32 n)
{
    CHECK(rx_n < 600);
    rx[rx_n].data = aligned_alloc(16, (n + 15u) & ~15u);
    memcpy(rx[rx_n].data, d, n);
    rx[rx_n].len = n;
    rx[rx_n].released = 0;
    rx_n++;
}
int32_t dk_usb_bulk_open(dk_module_ctx *ctx, dk_usb_bulk **out)
{
    (void)ctx;
    *out = (dk_usb_bulk *)&ub_token;
    return 0;
}
int32_t dk_usb_bulk_enable(dk_usb_bulk *u, int32_t io_id)
{
    (void)u;
    (void)io_id;
    if (bulk_state & DK_USB_BULK_WRONG_MODE)
        return DK_USB_BULK_E_STATE;
    enables++;
    bulk_state |= DK_USB_BULK_ENABLED | DK_USB_BULK_LIVE | DK_USB_BULK_CONNECTED;
    return 0;
}
int32_t dk_usb_bulk_disable(dk_usb_bulk *u, int32_t io_id)
{
    (void)u;
    (void)io_id;
    disables++;
    bulk_state &= ~(DK_USB_BULK_ENABLED | DK_USB_BULK_LIVE);
    return 0;
}
uint32_t dk_usb_bulk_state(dk_usb_bulk *u)
{
    (void)u;
    return bulk_state;
}
int32_t dk_usb_bulk_take(dk_usb_bulk *u, void **buf, uint32_t *len)
{
    (void)u;
    if (!(bulk_state & DK_USB_BULK_LIVE) || rx_next >= rx_n)
        return 0;
    *buf = rx[rx_next].data;
    *len = rx[rx_next].len;
    rx_next++;
    return 1;
}
int32_t dk_usb_bulk_release(dk_usb_bulk *u, void *buf)
{
    (void)u;
    for (int i = 0; i < rx_n; i++)
        if (rx[i].data == buf) {
            if (rx[i].released)
                double_release = 1;
            rx[i].released = 1;
            return 0;
        }
    double_release = 1; /* a buffer the adapter never gave */
    return -1;
}
int32_t dk_usb_bulk_ring(dk_usb_bulk *u, int32_t set)
{
    (void)u;
    if (!ring_present)
        return 0;
    if (set >= 0)
        ring_val = (ring_val & ~1u) | (u32)set;
    return (int32_t)ring_val;
}

typedef struct {
    u8 cmd, seq;
    i32 status;
    u32 len;
    u8 body[32];
} reply_t;
static reply_t replies[400];
static int reply_n;
static void note_reply(const u8 *r, u32 n)
{
    CHECK(reply_n < 400);
    CHECK(n >= HDR && memcmp(r, "DKB1", 4) == 0);
    reply_t *p = &replies[reply_n++];
    p->cmd = r[4];
    p->seq = r[5];
    p->status = (int16_t)((r[6] << 8) | r[7]);
    p->len = dk_bytes_get32(r + 8);
    CHECK_EQ(n, HDR + p->len);
    memcpy(p->body, r + HDR, p->len < 32 ? p->len : 32);
}
int32_t dk_usb_bulk_send(dk_usb_bulk *u, const void *buf, uint32_t len)
{
    (void)u;
    if (send_rc)
        return send_rc;
    note_reply(buf, len);
    return 0;
}

/* The runtime's write session: one open file, the offset and length rules, the 20 s idle expiry, the writes it received. */
#define FILE_MAX (3u << 20)
static int fh_open, fh_calls_busy;
static u32 f_size, f_written, f_last_act, f_flags;
static u8 f_hash[4];
static u8 *f_data;
static int expired;
typedef struct {
    u32 off, len;
} wr_t;
static wr_t writes[2000];
static int write_n, closes, aborts, commits, creates;
static u8 *done_data;
static u32 done_len;
static int busy_next; /* the next n calls to create/write/close answer E_BUSY */
static i32 write_rc;  /* a status for the next write (0: none) */
static int misaligned_write;

/* The runtime drops a write session that has had no create or write for 20 s: the call that finds it gone answers E_STATE, and the next create starts a new one. */
static int session_expired(void)
{
    if (fh_open && now_blocks - f_last_act >= SESS_IDLE_BLOCKS) {
        fh_open = 0;
        expired = 1;
        return 1;
    }
    return 0;
}
int32_t dk_files_open(dk_module_ctx *ctx, dk_files **out)
{
    (void)ctx;
    *out = (dk_files *)&files_token;
    return 0;
}
int32_t dk_files_create(dk_files *f, uint32_t root, const char *path, uint32_t size,
                        const void *hash4, uint32_t flags)
{
    (void)f;
    (void)path;
    creates++;
    if (busy_next > 0) {
        busy_next--;
        fh_calls_busy++;
        return DK_E_BUSY;
    }
    CHECK_EQ(root, 2);
    session_expired();
    if (fh_open)
        return DK_E_STATE;
    if (size > FILE_MAX)
        return DK_E_FULL;
    fh_open = 1;
    expired = 0;
    f_size = size;
    f_written = 0;
    f_flags = flags;
    f_last_act = now_blocks;
    memset(f_hash, 0, 4);
    if (hash4)
        memcpy(f_hash, hash4, 4);
    free(f_data);
    f_data = malloc(size ? size : 1);
    return 1;
}
int32_t dk_files_write(dk_files *f, int32_t h, uint32_t off, const void *buf, uint32_t len)
{
    (void)f;
    if (busy_next > 0) {
        busy_next--;
        fh_calls_busy++;
        return DK_E_BUSY;
    }
    if (session_expired() || !fh_open || h != 1)
        return DK_E_STATE;
    if (write_rc) {
        i32 r = write_rc;
        write_rc = 0;
        return r;
    }
    if (((uintptr_t)buf & 15u) != 0)
        misaligned_write = 1;
    if (off != f_written)
        return DK_E_OFFSET;
    if (off + len > f_size || ((len & 511u) != 0 && off + len != f_size))
        return DK_E_LENGTH;
    memcpy(f_data + off, buf, len);
    f_written += len;
    f_last_act = now_blocks;
    CHECK(write_n < 2000);
    writes[write_n].off = off;
    writes[write_n].len = len;
    write_n++;
    return 0;
}
static u32 fnv4(const u8 *d, u32 n)
{
    u32 h = 2166136261u;
    for (u32 i = 0; i < n; i++)
        h = (h ^ d[i]) * 16777619u;
    return h;
}
int32_t dk_files_close(dk_files *f, int32_t h, void *hash_out)
{
    (void)f;
    if (busy_next > 0) {
        busy_next--;
        fh_calls_busy++;
        return DK_E_BUSY;
    }
    if (session_expired() || !fh_open || h != 1)
        return DK_E_STATE;
    fh_open = 0;
    closes++;
    if (f_written != f_size)
        return DK_E_LENGTH;
    dk_bytes_put32(hash_out, fnv4(f_data, f_size));
    u8 want[4];
    dk_bytes_put32(want, fnv4(f_data, f_size));
    if ((f_hash[0] | f_hash[1] | f_hash[2] | f_hash[3]) && memcmp(f_hash, want, 4) != 0)
        return DK_E_HASH;
    free(done_data);
    done_data = f_data;
    done_len = f_size;
    f_data = NULL;
    return 0;
}
int32_t dk_files_abort(dk_files *f, int32_t h)
{
    (void)f;
    (void)h;
    aborts++;
    if (session_expired() || !fh_open)
        return DK_E_STATE;
    fh_open = 0;
    return 0;
}
int32_t dk_files_commit(dk_files *f)
{
    (void)f;
    commits++;
    return 0;
}
int32_t dk_files_pending(dk_files *f)
{
    (void)f;
    return 0;
}

/* ---- the driver ---------------------------------------------------------------------------------------------------------------------- */

static void fresh(void)
{
    now_blocks = 100000;
    arena_max = 256u * 1024u;
    fw_max = 8u << 20;
    debug_flags = 5u;
    sdhc_present = 1;
    sdhc_switch = 1;
    ring_present = 1;
    ring_val = 1u | (12u << 8) | (24u << 16);
    bulk_state = DK_USB_BULK_CONNECTED; /* not enabled yet: the first step enables */
    CHECK_EQ(dk_module.init((dk_module_ctx *)&ctx_token), 0);
    CHECK(job_step == step && host_cmd == command);
}

static unsigned long progress(void)
{
    return rx_next * 1000003ul + reply_n * 10007ul + write_n * 101ul + closes * 7ul + aborts * 3ul +
           (unsigned long)rep_count;
}

/* Run the job until it waits and nothing moves any more. */
static void run(void)
{
    unsigned long last = ~0ul;
    for (int i = 0; i < 100000; i++) {
        i32 r = step(0);
        CHECK(r >= 0);
        if (r == DK_IO_MORE)
            continue;
        if (progress() == last)
            return;
        last = progress();
    }
    CHECK(!"the job does not settle");
}

/* The host's frames. */
static u32 put_frame(u8 *out, u8 cmd, u8 seq, u16 flags, const u8 *body, u32 n)
{
    memcpy(out, "DKB1", 4);
    out[4] = cmd;
    out[5] = seq;
    out[6] = (u8)(flags >> 8);
    out[7] = (u8)flags;
    dk_bytes_put32(out + 8, n);
    if (n)
        memcpy(out + HDR, body, n);
    return HDR + n;
}
static u32 begin_frame(u8 *out, u8 seq, u16 flags, u32 size, const u8 *hash4)
{
    u8 b[64];
    u32 n = 0;
    dk_bytes_put32(b, 2);
    n = 4;
    memcpy(b + n, "f.bin", 6);
    n += 6;
    dk_bytes_put32(b + n, size);
    n += 4;
    if (hash4)
        memcpy(b + n, hash4, 4);
    else
        memset(b + n, 0, 4);
    n += 4;
    return put_frame(out, CMD_BEGIN, seq, flags, b, n);
}
static u8 big[FRAME_MAX];
static u32 data_frame(u8 *out, u8 seq, u32 off, const u8 *d, u32 n)
{
    dk_bytes_put32(out + HDR, off);
    memcpy(out + HDR + 4, d, n);
    return put_frame(out, CMD_DATA, seq, 0, out + HDR, 4 + n);
}

/* A transfer: buffers of at most 16 KiB; the last one is shorter (the end of the transfer). */
static void transfer(const u8 *bytes, u32 n)
{
    while (n >= DK_USB_BULK_RX_BYTES) {
        rx_push(bytes, DK_USB_BULK_RX_BYTES);
        bytes += DK_USB_BULK_RX_BYTES;
        n -= DK_USB_BULK_RX_BYTES;
    }
    if (n)
        rx_push(bytes, n);
}
static void send_frame(u8 cmd, u8 seq, const u8 *body, u32 n)
{
    static u8 f[HDR + 64];
    transfer(f, put_frame(f, cmd, seq, 0, body, n));
}

static void dump_replies(void)
{
    for (int i = 0; i < reply_n; i++)
        printf("  reply %d: cmd %02x seq %u status %d len %u\n", i, replies[i].cmd, replies[i].seq,
               replies[i].status, replies[i].len);
}
static const reply_t *reply_at(int i)
{
    static const reply_t none = {0, 0, 12345, 0, {0}};
    return i < reply_n ? &replies[i] : &none;
}
static void expect_reply(int i, u8 cmd, u8 seq, i32 status)
{
    const reply_t *r = reply_at(i);
    if (r->cmd != (u8)(cmd | REPLY_FLAG) || r->seq != seq || r->status != status) {
        printf(
            "%s: reply %d is cmd %02x seq %u status %d, want cmd %02x seq %u status %d (%d replies)\n",
            __FILE__, i, r->cmd, r->seq, r->status, cmd | REPLY_FLAG, seq, status, reply_n);
        failed = 1;
    }
}
static u8 *pat;
static void make_pattern(u32 n)
{
    free(pat);
    pat = malloc(n ? n : 1);
    for (u32 i = 0; i < n; i++)
        pat[i] = (u8)(i * 7 + (i >> 8));
}

/* One upload: BEGIN, DATA frames of `chunk` bytes, END, each frame one transfer, all queued before the job runs. */
static void upload(u32 size, u32 chunk, u16 flags)
{
    u32 n = begin_frame(big, 1, flags, size, NULL);
    transfer(big, n);
    u8 seq = 2;
    for (u32 off = 0; off < size; off += chunk, seq++) {
        u32 len = size - off < chunk ? size - off : chunk;
        transfer(big, data_frame(big, seq, off, pat + off, len));
    }
    send_frame(CMD_END, seq, NULL, 0);
}
static void expect_upload_ok(u32 size, const char *what)
{
    int frames = (int)((size + 65535u) / 65536u);
    if (reply_n != frames + 2) {
        printf("%s: %s: %d replies, want %d\n", __FILE__, what, reply_n, frames + 2);
        failed = 1;
    }
    for (int i = 0; i < reply_n; i++)
        CHECK_EQ(reply_at(i)->status, 0);
    CHECK_EQ(done_len, size);
    CHECK(size == 0 || (done_data && memcmp(done_data, pat, size) == 0));
    CHECK(!misaligned_write);
    CHECK(!fh_open && handle == 0);
    CHECK(!double_release);
    /* every receive buffer went back */
    for (int i = 0; i < rx_n; i++)
        CHECK(rx[i].released);
}

/* ---- the cases ----------------------------------------------------------------------------------------------------------------------- */

static void case_info_carries_the_nonce_of_the_last_set_nonce(void)
{
    fresh();
    run(); /* enables */
    const u8 n1[8] = {1, 2, 3, 4, 5, 6, 7, 8}, n2[8] = {9, 9, 9, 9, 0, 0, 0, 1};
    u8 out[900];
    CHECK_EQ(host_cmd(NONCE_CMD, 1, n1, 8, out), 0);
    send_frame(CMD_INFO, 5, NULL, 0);
    run();
    expect_reply(0, CMD_INFO, 5, 0);
    CHECK_EQ(reply_at(0)->len, 20);
    CHECK_EQ(reply_at(0)->body[0], BULK_VERSION);
    CHECK_EQ(reply_at(0)->body[1], 0); /* the pair: replies come on EP2 */
    CHECK(memcmp(reply_at(0)->body + 12, n1, 8) == 0);
    CHECK_EQ(host_cmd(NONCE_CMD, 2, n2, 8, out), 0);
    send_frame(CMD_INFO, 6, NULL, 0);
    run();
    expect_reply(1, CMD_INFO, 6, 0);
    CHECK(memcmp(reply_at(1)->body + 12, n2, 8) == 0);
    CHECK(memcmp(reply_at(1)->body + 12, n1, 8) != 0);
    CHECK_EQ(host_cmd(NONCE_CMD, 3, n1, 7, out), E_MSG);
    CHECK_EQ(host_cmd(NONCE_CMD, 3, n1, 0, out), E_MSG);
    CHECK_EQ(host_cmd(NONCE_CMD + 2, 3, n1, 8, out), E_PROTO);
    /* an odd generation (a SET_NONCE in progress) is never returned */
    nonce_gen |= 1u;
    send_frame(CMD_INFO, 7, NULL, 0);
    run();
    expect_reply(2, CMD_INFO, 7, DK_FILE_E_BUSY);
}

static i32 speed(u32 bit, u32 v, u32 *mask)
{
    u8 body[3] = {'S', (u8)bit, (u8)v}, out[900];
    i32 r = host_cmd(NONCE_CMD + 1, 0, body, 3, out);
    if (r == 4)
        *mask = dk_bytes_get32(out);
    return r;
}
static void case_the_speed_switches_make_the_mask(void)
{
    fresh();
    run();
    u8 out[900];
    const u8 read[1] = {'S'};
    CHECK_EQ(host_cmd(NONCE_CMD + 1, 0, read, 1, out), 4);
    CHECK_EQ(dk_bytes_get32(out), 15); /* all four on at boot */
    u32 m;
    /* each switch alone, off then on, leaves the other three as they were */
    for (u32 bit = 0; bit < 4; bit++) {
        CHECK_EQ(speed(bit, 0, &m), 4);
        CHECK_EQ(m, 15u & ~(1u << bit));
        CHECK_EQ(speed(bit, 1, &m), 4);
        CHECK_EQ(m, 15);
    }
    /* the middle two combinations: ring off with the large commands on, and the other way round */
    CHECK_EQ(speed(1, 0, &m), 4);
    CHECK_EQ(m, 13);
    CHECK_EQ(ring_val & 1u, 0);
    CHECK_EQ(speed(1, 1, &m), 4);
    CHECK_EQ(speed(3, 0, &m), 4);
    CHECK_EQ(m, 7);
    CHECK_EQ(sdhc_switch, 0);
    CHECK_EQ(speed(0, 0, &m), 4);
    CHECK_EQ(speed(2, 0, &m), 4);
    CHECK_EQ(m, 2);
    CHECK_EQ(debug_flags & 5u, 0);
    /* refusals */
    CHECK_EQ(speed(4, 1, &m), E_MSG);
    CHECK_EQ(speed(0, 2, &m), E_MSG);
    const u8 unknown[1] = {'Q'};
    CHECK_EQ(host_cmd(NONCE_CMD + 1, 0, unknown, 1, out), E_MSG);
    /* an adapter that is not in the image: its switch cannot be set and reads off */
    ring_present = 0;
    CHECK_EQ(speed(1, 1, &m), E_STATE);
    sdhc_present = 0;
    sl = 0;
    CHECK_EQ(speed(3, 1, &m), E_STATE);
    CHECK_EQ(speed(0, 1, &m), 4);
    CHECK_EQ(m & 10u, 0);
}

static void case_several_frames_in_one_transfer_are_answered_in_order(void)
{
    fresh();
    run();
    u8 *p = big;
    u32 n = begin_frame(p, 1, 0, 1024, NULL);
    p += n;
    make_pattern(1024);
    p += data_frame(p, 2, 0, pat, 512);
    p += data_frame(p, 3, 512, pat + 512, 512);
    p += put_frame(p, CMD_END, 4, 0, NULL, 0);
    p += put_frame(p, CMD_INFO, 5, 0, NULL, 0);
    transfer(big, (u32)(p - big));
    run();
    CHECK_EQ(reply_n, 5);
    for (int i = 0; i < 5; i++)
        expect_reply(i, (u8[]){CMD_BEGIN, CMD_DATA, CMD_DATA, CMD_END, CMD_INFO}[i], (u8)(i + 1),
                     0);
    CHECK_EQ(done_len, 1024);
    CHECK(memcmp(done_data, pat, 1024) == 0);
    CHECK_EQ(reply_at(3)->len, 8); /* END: hash4, pending, reserved */
    u8 want[4];
    dk_bytes_put32(want, fnv4(pat, 1024));
    CHECK(memcmp(reply_at(3)->body, want, 4) == 0);
}

static void case_every_split_point_of_a_frame_works(void)
{
    /* a frame cut at every byte: the two parts are two receive buffers of one transfer; the module keeps its state in statics, so each cut is a process of its own */
    u8 f1[HDR + 64];
    u32 n1 = begin_frame(f1, 1, 0, 512, NULL);
    int bad = 0;
    for (u32 k = 1; k < n1; k++) {
        pid_t pid = fork();
        if (pid == 0) {
            fresh();
            run();
            rx_push(f1, k);
            rx_push(f1 + k, n1 - k);
            send_frame(CMD_ABORT, 2, NULL, 0);
            run();
            _exit(reply_n == 2 && reply_at(0)->status == 0 && reply_at(1)->status == 0 &&
                          aborts == 1 && !failed
                      ? 0
                      : 1);
        }
        int st;
        waitpid(pid, &st, 0);
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            printf("%s: BEGIN cut at byte %u fails\n", __FILE__, k);
            bad = 1;
        }
    }
    u8 f2[HDR + 4 + 512];
    make_pattern(512);
    u32 n2 = data_frame(f2, 2, 0, pat, 512);
    for (u32 k = 1; k < n2; k++) {
        pid_t pid = fork();
        if (pid == 0) {
            fresh();
            run();
            transfer(f1, n1);
            rx_push(f2, k);
            rx_push(f2 + k, n2 - k);
            send_frame(CMD_END, 3, NULL, 0);
            run();
            _exit(reply_n == 3 && reply_at(1)->status == 0 && reply_at(2)->status == 0 &&
                          done_len == 512 && memcmp(done_data, pat, 512) == 0 && !failed
                      ? 0
                      : 1);
        }
        int st;
        waitpid(pid, &st, 0);
        if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
            printf("%s: DATA cut at byte %u fails\n", __FILE__, k);
            bad = 1;
        }
    }
    CHECK(!bad);
}

static void case_a_corrupt_stream_is_dropped_until_the_transfer_ends(void)
{
    fresh();
    run();
    make_pattern(512);
    transfer(big, begin_frame(big, 1, 0, 512, NULL));
    run();
    expect_reply(0, CMD_BEGIN, 1, 0);
    CHECK(handle > 0);
    /* a bad length with a valid magic: E_MSG with the seq, the file is aborted; the buffer was full, so the transfer goes on in the next one */
    static u8 junk[DK_USB_BULK_RX_BYTES];
    u8 hdr[HDR];
    put_frame(hdr, CMD_DATA, 9, 0, NULL, 0);
    dk_bytes_put32(hdr + 8, DATA_MAX + 4 + SLACK + 1);
    memset(junk, 0xee, sizeof junk);
    memcpy(junk, hdr, HDR);
    rx_push(junk, sizeof junk); /* full: the transfer continues */
    memset(junk, 0xdd, 100);
    rx_push(junk, 100); /* a short buffer: the end of the transfer, skipped */
    /* the next transfer starts with a good frame */
    send_frame(CMD_INFO, 10, NULL, 0);
    run();
    expect_reply(1, CMD_DATA, 9, E_MSG);
    expect_reply(2, CMD_INFO, 10, 0);
    CHECK_EQ(reply_n, 3);
    CHECK_EQ(st_fatal, 1);
    CHECK(handle == 0 && aborts == 1);
    CHECK(!double_release);
    for (int i = 0; i < rx_n; i++)
        CHECK(rx[i].released);
    /* a bad magic: no reply (the seq cannot be trusted), the rest of the transfer is skipped, the stream parses again */
    reply_n = 0;
    u8 bad[HDR];
    put_frame(bad, CMD_INFO, 11, 0, NULL, 0);
    bad[0] = 'X';
    rx_push(bad, HDR);
    send_frame(CMD_INFO, 12, NULL, 0);
    run();
    CHECK_EQ(reply_n, 1);
    expect_reply(0, CMD_INFO, 12, 0);
    CHECK_EQ(st_fatal, 2);
}

static void case_a_bad_request_is_answered_in_every_state(void)
{
    fresh();
    run();
    make_pattern(2048);
    u8 f[HDR + 4 + 2048];
    /* no file open */
    transfer(f, data_frame(f, 1, 0, pat, 512));
    send_frame(CMD_END, 2, NULL, 0);
    send_frame(CMD_ABORT, 3, NULL, 0);
    send_frame(CMD_COMMIT, 4, NULL, 0);
    send_frame(CMD_VERIFY, 5, NULL, 0);
    send_frame(9, 6, NULL, 0);
    run();
    expect_reply(0, CMD_DATA, 1, E_STATE);
    expect_reply(1, CMD_END, 2, E_STATE);
    expect_reply(2, CMD_ABORT, 3, 0);
    expect_reply(3, CMD_COMMIT, 4, 0);
    CHECK_EQ(reply_at(3)->len, 4);
    expect_reply(4, CMD_VERIFY, 5, E_PROTO);
    expect_reply(5, 9, 6, E_PROTO);
    CHECK_EQ(commits, 1);
    reply_n = 0;
    /* BEGIN refused: a short body, a path without its NUL, a store that is full */
    send_frame(CMD_BEGIN, 7, (const u8 *)"\0\0\0\2f", 5);
    u8 nonul[4 + 3 + 8] = {0, 0, 0, 2, 'a', 'b', 'c', 0, 0, 0, 0, 0, 0, 0, 0};
    send_frame(CMD_BEGIN, 8, nonul, 4 + 3 + 8 - 8 + 0); /* ends inside the path: no NUL */
    transfer(big, begin_frame(big, 9, 0, FILE_MAX + 1, NULL));
    run();
    expect_reply(0, CMD_BEGIN, 7, E_MSG);
    expect_reply(1, CMD_BEGIN, 8, E_MSG);
    expect_reply(2, CMD_BEGIN, 9, DK_E_FULL);
    CHECK(handle == 0);
    reply_n = 0;
    /* a file open: DATA at a wrong offset, one that is not a whole sector in the middle, one past the end, a short body; END with less than the size */
    transfer(big, begin_frame(big, 10, 0, 1024, NULL));
    transfer(f, data_frame(f, 11, 512, pat, 512));
    transfer(f, data_frame(f, 12, 0, pat, 100));
    transfer(f, data_frame(f, 13, 0, pat, 1024 + 512));
    u8 tiny[3] = {0, 0, 0};
    send_frame(CMD_DATA, 14, tiny, 3);
    run();
    expect_reply(0, CMD_BEGIN, 10, 0);
    expect_reply(1, CMD_DATA, 11, DK_E_OFFSET);
    expect_reply(2, CMD_DATA, 12, DK_E_LENGTH);
    expect_reply(3, CMD_DATA, 13, DK_E_LENGTH);
    expect_reply(4, CMD_DATA, 14, E_MSG);
    CHECK_EQ(reply_n, 5);
    CHECK(handle > 0);
    transfer(f, data_frame(f, 15, 0, pat, 512));
    send_frame(CMD_END, 16, NULL, 0);
    run();
    expect_reply(5, CMD_DATA, 15, 0);
    expect_reply(6, CMD_END, 16, DK_E_LENGTH); /* 512 of 1024: the close refuses it */
    CHECK(handle == 0 && !fh_open);
    CHECK(done_len == 0);
    reply_n = 0;
    /* a wrong hash at END is E_HASH with the hash in the body; the session is closed either way */
    u8 wrong[4] = {1, 2, 3, 4};
    transfer(big, begin_frame(big, 17, 0, 512, wrong));
    transfer(f, data_frame(f, 18, 0, pat, 512));
    send_frame(CMD_END, 19, NULL, 0);
    run();
    expect_reply(2, CMD_END, 19, DK_E_HASH);
    CHECK_EQ(reply_at(2)->len, 8);
    CHECK(handle == 0);
}

static void case_a_new_begin_aborts_the_open_file(void)
{
    fresh();
    run();
    make_pattern(1024);
    transfer(big, begin_frame(big, 1, 0, 1024, NULL));
    u8 f[HDR + 4 + 512];
    transfer(f, data_frame(f, 2, 0, pat, 512));
    transfer(big, begin_frame(big, 3, 0, 512, NULL));
    run();
    CHECK_EQ(aborts, 1);
    expect_reply(2, CMD_BEGIN, 3, 0);
    CHECK(handle > 0 && f_size == 512);
    CHECK_EQ(write_n,
             0); /* nothing of the first file was written: its bytes were in the staging buffer */
}

static void case_the_staging_buffer_gives_the_file_its_bytes_at_every_size(void)
{
    static const u32 sizes[] = {1,
                                4095,
                                4096,
                                4097,
                                65536,
                                65537,
                                (256u << 10) - 1,
                                256u << 10,
                                (256u << 10) + 1,
                                (512u << 10),
                                (512u << 10) + 1,
                                1u << 20,
                                (1u << 20) + 513,
                                2u << 20,
                                (2u << 20) + 7};
    for (unsigned s = 0; s < sizeof sizes / sizeof sizes[0]; s++) {
        for (int tier = 0; tier < 4; tier++) {
            pid_t pid = fork();
            if (pid == 0) {
                fresh();
                u32 size = sizes[s];
                /* tier 0: the reservation of 1 MiB (the firmware allocator); 1: only 256 KiB can be had; 2: only the arena's small blocks; 3: no buffer at all */
                if (tier == 1)
                    fw_max = 256u << 10;
                if (tier == 2) {
                    fw_max = 0;
                    arena_max = 64u << 10;
                }
                if (tier == 3)
                    fw_max = arena_max = 0;
                helper_busy =
                    (int)(s &
                          1u); /* the overlap's copy goes to the helper task, or is made by the module */
                make_pattern(size);
                run();
                upload(size, 65536, 0);
                run();
                expect_upload_ok(size, "upload");
                /* the writes make the file in order: from 0, each at the end of the one before, whole sectors but the last */
                u32 at = 0;
                for (int i = 0; i < write_n; i++) {
                    CHECK_EQ(writes[i].off, at);
                    at += writes[i].len;
                }
                CHECK_EQ(at, size);
                if (tier == 3) { /* no buffer: one write for each DATA frame */
                    CHECK_EQ(write_n, (size + 65535u) / 65536u);
                    CHECK_EQ(acc_cap, 0);
                }
                if (tier == 0 && size > 1u << 20)
                    CHECK(write_n >= 2);
                _exit(failed);
            }
            int st;
            waitpid(pid, &st, 0);
            if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
                printf("%s: size %u, tier %d fails\n", __FILE__, sizes[s], tier);
                failed = 1;
            }
        }
    }
}

static void case_abort_and_a_bus_reset_drop_the_file_and_free_the_buffer(void)
{
    fresh();
    make_pattern(1u << 20);
    run();
    /* ABORT after some data: the staged bytes are gone, nothing was written, the next upload is whole */
    transfer(big, begin_frame(big, 1, 0, 1u << 20, NULL));
    u8 *f = malloc(HDR + 4 + 65536);
    for (u32 i = 0; i < 3; i++)
        transfer(f, data_frame(f, (u8)(2 + i), i * 65536u, pat + i * 65536u, 65536));
    send_frame(CMD_ABORT, 9, NULL, 0);
    run();
    CHECK(handle == 0 && acc_cap == 0 && aborts == 1 && write_n == 0 && !fh_open);
    expect_reply(4, CMD_ABORT, 9, 0);
    reply_n = 0;
    /* a bus reset across a flush: enough DATA to fill the buffer, then the reset; the file is aborted, the replies and the half frame are void */
    transfer(big, begin_frame(big, 20, 0, 1u << 20, NULL));
    for (u32 i = 0; i < 9; i++)
        transfer(f, data_frame(f, (u8)(21 + i), i * 65536u, pat + i * 65536u, 65536));
    run();
    CHECK(write_n >= 1 && fh_open);
    rx_n = rx_next;         /* the adapter dropped every buffer of the old connection */
    bulk_state += 1u << 24; /* the reset count changes */
    run();
    CHECK(handle == 0 && !fh_open && acc_cap == 0);
    CHECK_EQ(st_resets, 1);
    CHECK_EQ(rep_count, 0);
    /* the next upload works */
    reply_n = 0;
    write_n = 0;
    upload(1u << 20, 65536, 0);
    run();
    CHECK_EQ(done_len, 1u << 20);
    CHECK(memcmp(done_data, pat, 1u << 20) == 0);
    free(f);
}

static void case_a_busy_store_is_retried_and_then_reported(void)
{
    fresh();
    make_pattern(512);
    run();
    busy_next = 2; /* BEGIN is tried again (up to BEGIN_TRIES times) */
    transfer(big, begin_frame(big, 1, 0, 512, NULL));
    run();
    expect_reply(0, CMD_BEGIN, 1, 0);
    CHECK_EQ(fh_calls_busy, 2);
    u8 f[HDR + 4 + 512];
    transfer(f, data_frame(f, 2, 0, pat, 512));
    run();
    expect_reply(1, CMD_DATA, 2, 0);
    busy_next = 2; /* END: two tries are retried inside the job, the third answers */
    send_frame(CMD_END, 3, NULL, 0);
    run();
    expect_reply(2, CMD_END, 3, 0);
    CHECK_EQ(done_len, 512);
    /* a store that stays busy: BUSY_TRIES calls, then the host gets E_BUSY and asks again */
    reply_n = 0;
    transfer(big, begin_frame(big, 4, 0, 512, NULL));
    run();
    transfer(f, data_frame(f, 5, 0, pat, 512));
    run();
    busy_next = 1000;
    send_frame(CMD_END, 6, NULL, 0);
    run();
    expect_reply(2, CMD_END, 6, DK_FILE_E_BUSY);
    CHECK(handle > 0);
    busy_next = 0;
    send_frame(CMD_END, 7, NULL, 0);
    run();
    expect_reply(3, CMD_END, 7, 0);
}

static void case_an_idle_session_ends_with_state_and_no_stray_write(void)
{
    fresh();
    make_pattern(1u << 20);
    run();
    /* the pair: the host goes away with 3 frames staged; the runtime drops the write session after 20 s; the flushing DATA is answered E_STATE */
    transfer(big, begin_frame(big, 1, 0, 1u << 20, NULL));
    u8 *f = malloc(HDR + 4 + 65536);
    for (u32 i = 0; i < 3; i++)
        transfer(f, data_frame(f, (u8)(2 + i), i * 65536u, pat + i * 65536u, 65536));
    run();
    CHECK(handle > 0 && write_n == 0);
    now_blocks += SESS_IDLE_BLOCKS;
    for (u32 i = 3; i < 16; i++)
        transfer(f, data_frame(f, (u8)(2 + i), i * 65536u, pat + i * 65536u, 65536));
    send_frame(
        CMD_END, 40, NULL,
        0); /* the write of the full half was refused when it went out: the next request shows it */
    run();
    CHECK_EQ(write_n, 0);
    int e_state = -1;
    for (int i = 0; i < reply_n; i++)
        if (reply_at(i)->status == DK_E_STATE) {
            e_state = i;
            break;
        }
    if (e_state <= 0)
        dump_replies();
    CHECK(e_state > 0);
    CHECK(handle == 0 && acc_cap == 0);
    CHECK(done_len == 0);
    /* the next upload on the same link works (the runtime opened a new session) */
    reply_n = 0;
    make_pattern(2048);
    upload(2048, 2048, 0);
    run();
    expect_upload_ok(2048, "after the expiry");
    free(f);
}

static void case_an_expired_session_with_a_half_pending_and_a_half_received_frame(void)
{
    fresh();
    make_pattern(2u << 20);
    run();
    /* the overlap: 7 frames of 64 KiB leave the first half of 512 KiB nearly full and nothing written; the session expires; the frame that fills the half is
     * followed by the first bytes of the next one */
    transfer(big, begin_frame(big, 1, 0, 2u << 20, NULL));
    u8 *f = malloc(HDR + 4 + 65536);
    for (u32 i = 0; i < 7; i++)
        transfer(f, data_frame(f, (u8)(2 + i), i * 65536u, pat + i * 65536u, 65536));
    run();
    CHECK(ov == 1 && write_n == 0);
    now_blocks += SESS_IDLE_BLOCKS;
    u32 n = data_frame(f, 9, 7 * 65536u, pat + 7 * 65536u, 65536);
    transfer(f, n);
    u32 n2 = data_frame(f, 10, 8 * 65536u, pat + 8 * 65536u, 65536);
    rx_push(f, 100); /* the half of the next frame: the header and a part of the offset... */
    (void)n2;
    run();
    CHECK_EQ(write_n, 0);
    CHECK(handle == 0 && !fh_open);
    CHECK_EQ(done_len, 0);
    int e_state = 0;
    for (int i = 0; i < reply_n; i++)
        e_state += reply_at(i)->status == DK_E_STATE;
    CHECK(e_state >= 1);
    CHECK(!double_release);
    /* the next upload works */
    reply_n = 0;
    now_blocks += 10;
    make_pattern(4096);
    run();
    upload(4096, 4096, 0);
    run();
    CHECK_EQ(done_len, 4096);
    free(f);
}

static void case_out_only_replies_wait_for_the_poll_and_an_idle_session_frees_the_buffers(void)
{
    fresh();
    bulk_state |= DK_USB_BULK_OUT_ONLY;
    make_pattern(512);
    run();
    CHECK_EQ(enables, 0); /* OUT-only: nothing is allocated until a host asks */
    u8 out[900];
    const u8 nonce8[8] = {1, 1, 1, 1, 1, 1, 1, 1};
    CHECK_EQ(host_cmd(NONCE_CMD, 1, nonce8, 8, out), 0);
    run();
    CHECK_EQ(enables, 1);
    CHECK(res_raw != NULL);
    send_frame(CMD_INFO, 1, NULL, 0);
    transfer(big, begin_frame(big, 2, 0, 512, NULL));
    run();
    CHECK_EQ(reply_n, 0); /* the replies are not sent: the host polls */
    CHECK_EQ(rep_count, 2);
    const u8 poll[1] = {'R'};
    i32 n = host_cmd(NONCE_CMD + 1, 0, poll, 1, out);
    CHECK_EQ(n, (HDR + 20) + HDR); /* the two replies, whole and in order */
    CHECK(memcmp(out, "DKB1", 4) == 0);
    CHECK_EQ(out[4], CMD_INFO | REPLY_FLAG);
    CHECK_EQ(out[HDR + 20 + 4], CMD_BEGIN | REPLY_FLAG);
    CHECK_EQ(rep_count, 0);
    /* 21 s without a request: the job ends the session, the staging reservation and the ring go back */
    now_blocks += SESS_IDLE_BLOCKS;
    run();
    CHECK_EQ(st_expired, 1);
    CHECK_EQ(disables, 1);
    CHECK(res_raw == NULL && !enabled && handle == 0);
    CHECK_EQ(abort_h, 1); /* the abort of the open file goes out in the next session's first step */
    /* no new session until the next nonce */
    run();
    CHECK_EQ(enables, 1);
    CHECK_EQ(host_cmd(NONCE_CMD, 2, nonce8, 8, out), 0);
    run();
    CHECK_EQ(enables, 2);
    CHECK(abort_h == 0 && aborts == 1 && !fh_open);
}

static void case_a_port_without_bulk_ends_the_session_after_two_seconds(void)
{
    fresh();
    run();
    CHECK(enabled);
    bulk_state |= DK_USB_BULK_WRONG_MODE;
    run();
    CHECK(enabled); /* the first 2 s wait for the mode to come back */
    now_blocks += WRONG_MODE_BLOCKS;
    run();
    CHECK(!enabled);
    CHECK_EQ(disables, 1);
    bulk_state &= ~DK_USB_BULK_WRONG_MODE; /* USB MIDI again: the next wake enables */
    run();
    CHECK(enabled);
}

/* ---- the runner ---------------------------------------------------------------------------------------------------------------------- */

static int ran, bad;
static const char *only; /* argv[1]: run the cases whose name contains it */
static void run_case(const char *name, void (*fn)(void))
{
    if (only && !strstr(name, only))
        return;
    pid_t pid = fork();
    ran++;
    if (pid == 0) {
        fn();
        fflush(stdout);
        _exit(failed ? 1 : 0);
    }
    int st;
    waitpid(pid, &st, 0);
    if (!WIFEXITED(st) || WEXITSTATUS(st) != 0) {
        printf("FAIL %s%s\n", name, WIFSIGNALED(st) ? " (a signal)" : "");
        bad++;
    }
}
#define RUN(f) run_case(#f, f)

int main(int argc, char **argv)
{
    only = argc > 1 ? argv[1] : NULL;
    setvbuf(stdout, NULL, _IONBF, 0);
    RUN(case_info_carries_the_nonce_of_the_last_set_nonce);
    RUN(case_the_speed_switches_make_the_mask);
    RUN(case_several_frames_in_one_transfer_are_answered_in_order);
    RUN(case_every_split_point_of_a_frame_works);
    RUN(case_a_corrupt_stream_is_dropped_until_the_transfer_ends);
    RUN(case_a_bad_request_is_answered_in_every_state);
    RUN(case_a_new_begin_aborts_the_open_file);
    RUN(case_the_staging_buffer_gives_the_file_its_bytes_at_every_size);
    RUN(case_abort_and_a_bus_reset_drop_the_file_and_free_the_buffer);
    RUN(case_a_busy_store_is_retried_and_then_reported);
    RUN(case_an_idle_session_ends_with_state_and_no_stray_write);
    RUN(case_an_expired_session_with_a_half_pending_and_a_half_received_frame);
    RUN(case_out_only_replies_wait_for_the_poll_and_an_idle_session_frees_the_buffers);
    RUN(case_a_port_without_bulk_ends_the_session_after_two_seconds);
    printf("uf-host: %d cases, %d failed\n", ran, bad);
    return bad ? 1 : 0;
}
