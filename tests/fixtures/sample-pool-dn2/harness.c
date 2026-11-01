/* Host harness for modules/core/adapters/sample-pool/src/cf/dn2 (pool.c and xfer.c, unchanged): the store, the DSP block clock, the stock
 * LP0 sender and the runtime's completion call are stand-ins, and a small model of the DSP receiver (spw_dsp_rule) writes the pages into
 * a DDR array and answers through the real reply hook. tests/patchkit/test_sample_pool_dn2.py runs the scenarios. It tests our logic:
 * the allocator, the page order, the resend and error rules, cancel and the data that reaches DDR; not the firmware or the DSP code. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cf/dn2/xfer.h"

#define DDR_BASE 0x80531000u
#define DDR_BYTES 0x100000u   /* 256 pages: small enough to fill */
const uint32_t sp_dev_ddr_base = DDR_BASE;
const uint32_t sp_dev_ddr_bytes = DDR_BYTES;

static uint8_t ddr[DDR_BYTES];

/* ---- store ---- */
typedef struct { char path[32]; uint32_t size, id; uint8_t *data; } File;
static File files[8];
static int nfiles;
static int read_fail_at = -1, reads;
static uint32_t val(uint32_t id, uint32_t frame, uint32_t ch) { return (id * 7919u + frame * 31u + ch * 1009u + 17u) & 0xffffu; }

static void make_file(const char *path, uint32_t id, uint32_t frames, uint32_t channels, int corrupt)
{
    File *f = &files[nfiles++];
    uint32_t pcm = frames * 2u * channels, i, c;
    f->size = 64u + pcm + 16u;
    f->id = id;
    f->data = calloc(1, f->size + 4096);
    strcpy(f->path, path);
    f->data[1] = (uint8_t)(channels - 1u);
    f->data[4] = pcm >> 24; f->data[5] = pcm >> 16; f->data[6] = pcm >> 8; f->data[7] = pcm;
    f->data[8] = 0; f->data[9] = 0; f->data[10] = 0xbb; f->data[11] = 0x80;   /* 48000 */
    f->data[0x14] = 0x7f;
    if (corrupt) f->data[30] = 1;
    for (i = 0; i < frames; i++)
        for (c = 0; c < channels; c++) {
            uint32_t v = val(id, i, c), at = 64u + (i * channels + c) * 2u;
            f->data[at] = v >> 8; f->data[at + 1] = v;
        }
}
int32_t dkrt_store_stat(const uint8_t *path, DkrtStoreFile *o)
{
    int i;
    for (i = 0; i < nfiles; i++)
        if (!strcmp((const char *)path, files[i].path)) {
            o->first = (uint32_t)i * 0x10000u;
            o->size = files[i].size;
            memset(o->sha, files[i].id, 32);
            return 0;
        }
    return DKRT_E_NOT_FOUND;
}
int32_t dkrt_store_read(const DkrtStoreFile *f, uint32_t sector, uint32_t count, uint8_t *dst)
{
    File *file = &files[f->first / 0x10000u];
    uint32_t sectors = (file->size + 511u) / 512u;
    if (((uintptr_t)dst & 15u) || count < 1 || count > 128 || sector + count > sectors) { printf("badread %u %u\n", sector, count); exit(3); }
    if (reads++ == read_fail_at) return DKRT_E_READ;
    memcpy(dst, file->data + sector * 512u, count * 512u);
    return 0;
}

/* ---- clock, completions ---- */
static uint32_t clock_now = 1000;
uint32_t dkrt_blocks(void) { return clock_now; }
static int done_n; static uint32_t done_req[64]; static int32_t done_res[64];
void dkrt_pool_done(uint32_t req, int32_t result) { done_req[done_n] = req; done_res[done_n++] = result; }
void dkrt_pool_lost(void) { puts("LOST"); exit(4); }

/* ---- DSP model ---- */
static uint32_t d_last = 0xd0000000u, d_count, d_status = 1, d_err, d_errcount;
static int sends, drop_sends, freeze_frames, force_err_tag;
static uint32_t sent_tags[2000];
void *sp_stock_copy(void *d, const void *s, uint32_t n) { memcpy(d, s, n); return d; }
void sp_stock_send(uint32_t tag, const volatile uint32_t *src, uint32_t format)
{
    uint32_t r;
    if (format != 1 || ((uintptr_t)src & 15u)) { puts("badsend"); exit(5); }
    sent_tags[sends++] = tag;
    if (drop_sends > 0) { drop_sends--; return; }
    r = force_err_tag ? SPW_ERR_TAG : spw_dsp_rule(d_last, tag, DDR_BYTES / 4096u);
    force_err_tag = 0;
    if (r >= 4) { d_status = 2; d_err = r; d_errcount++; return; }
    memcpy(ddr + spw_tag_page(tag) * 4096u, (const void *)src, 4096);
    if (r == 0) { d_count++; d_last = tag; }
    d_status = 1; d_err = 0;
}
static int garble;
static void deliver(void)
{
    static uint8_t reply[0xabc + 16 + 16], dest[0xabc + 16];
    uint16_t *h = (uint16_t *)(reply + 0xabc);
    h[0] = 0x5350; h[1] = 2; h[2] = d_status; h[3] = d_err | (d_errcount << 8);
    h[4] = d_count; h[5] = d_count >> 16; h[6] = (spw_tag_seq(d_last) << 8) | (d_last & 0xff);
    h[7] = (spw_ack_sum(h) ^ 0x5a5a) & 0xffff;
    if (garble) h[7] ^= 1;
    sp_copy_reply(dest, reply, 0xabc);
}
static void step(void)
{
    clock_now++;
    if (freeze_frames > 0) freeze_frames--; else deliver();
    dkpool_tick();
}
static void run(int n, int stop_after_done) { int d0 = done_n; while (n-- > 0) { step(); if (stop_after_done && done_n > d0) break; } }

/* ---- helpers ---- */
static dkpool_handle H[8];
static dk_file_id fid(const char *path)
{
    dk_file_id id; int i;
    memset(&id, 0, sizeof id);
    for (i = 0; i < nfiles; i++)
        if (!strcmp(path, files[i].path)) { id.size = files[i].size; id.flags = 1; id.hash_kind = 1; memset(id.hash, files[i].id, 32); }
    return id;
}
static int32_t load(uint32_t req, const char *path, dkpool_handle *h) { dk_file_id id = fid(path); return dkpool_load(req, &id, (const uint8_t *)path, h); }
static uint32_t off_of(uint32_t nw) { return 0x80000000u + (nw - 0x10000000u) * 4u - DDR_BASE; }
/* the planes in DDR against the pattern, the zero padding of each plane's last page included: bit 0 L ok, bit 1 R ok */
static int planes_ok(const dkpool_handle *h, uint32_t id)
{
    int ok = 3; uint32_t ch, i, pp = (h->frames + 2047) / 2048;
    for (ch = 0; ch < h->channels; ch++) {
        uint32_t off = off_of(ch ? h->base_r : h->base_l);
        for (i = 0; i < pp * 2048; i++) {
            uint32_t want = i < h->frames ? val(id, i, ch) : 0, got = ((uint32_t)ddr[off + 2 * i] << 8) | ddr[off + 2 * i + 1];
            if (got != want) ok &= ~(1 << ch);
        }
    }
    if (h->channels == 1) ok &= 1;
    return ok;
}
static void show(const char *k, const dkpool_handle *h)
{
    printf("%s id=%u frames=%u ch=%u rate=%u l=%x r=%x\n", k, h->id, h->frames, h->channels, h->rate, h->base_l, h->base_r);
}

int main(int argc, char **argv)
{
    const char *s = argc > 1 ? argv[1] : "";
    int32_t rc;
    printf("wire %u\n", sp_wire_size());
    if (!strcmp(s, "stereo")) {
        make_file("a", 1, 3000, 2, 0);
        rc = load(1, "a", &H[0]);
        run(2000, 1);
        printf("rc %d done %d %d sends %d\n", rc, done_n, done_res[0], sends);
        show("h", &H[0]);
        printf("planes %d check %d free %u\n", planes_ok(&H[0], 1), dkpool_check(&H[0]), dkpool_free_frames());
        printf("tags %x %x %x %x\n", sent_tags[0], sent_tags[1], sent_tags[2], sent_tags[3]);
    } else if (!strcmp(s, "mono")) {
        make_file("a", 2, 5000, 1, 0);
        rc = load(7, "a", &H[0]);
        run(2000, 1);
        printf("rc %d done %d %d req %u sends %d\n", rc, done_n, done_res[0], done_req[0], sends);
        show("h", &H[0]);
        printf("planes %d\n", planes_ok(&H[0], 2));
        printf("tags %x %x %x\n", sent_tags[0], sent_tags[1], sent_tags[2]);
    } else if (!strcmp(s, "exact")) {   /* frames a multiple of 2048, and one frame */
        make_file("a", 3, 4096, 2, 0); make_file("b", 4, 1, 1, 0);
        load(1, "a", &H[0]); run(3000, 1);
        load(2, "b", &H[1]); run(3000, 1);
        printf("done %d %d %d sends %d planes %d %d\n", done_n, done_res[0], done_res[1], sends, planes_ok(&H[0], 3), planes_ok(&H[1], 4));
    } else if (!strcmp(s, "dedup")) {
        make_file("a", 5, 3000, 2, 0);
        load(1, "a", &H[0]); run(2000, 1);
        int before = sends;
        rc = load(2, "a", &H[1]);
        printf("second rc %d sends %d same %d token %d %d\n", rc, sends - before, H[0].base_l == H[1].base_l && H[0].base_r == H[1].base_r, H[0].token == H[1].token, dkpool_check(&H[0]));
        dkpool_release(&H[0]);
        printf("after one release check %d %d\n", dkpool_check(&H[0]), dkpool_check(&H[1]));
        dkpool_release(&H[1]);
        printf("after two release check %d free %u\n", dkpool_check(&H[1]), dkpool_free_frames());
    } else if (!strcmp(s, "cache")) {
        make_file("a", 6, 3000, 2, 0);
        load(1, "a", &H[0]); run(2000, 1);
        dkpool_release(&H[0]);
        int before = sends;
        rc = load(2, "a", &H[1]);
        printf("rc %d sends %d same %d tokens differ %d old %d new %d planes %d\n", rc, sends - before, H[0].base_l == H[1].base_l, H[0].token != H[1].token,
               dkpool_check(&H[0]), dkpool_check(&H[1]), planes_ok(&H[1], 6));
        dkpool_release(&H[0]);   /* a stale handle changes nothing */
        printf("still ready %d\n", dkpool_check(&H[1]));
    } else if (!strcmp(s, "evict")) {
        make_file("a", 7, 140000, 2, 0); make_file("b", 8, 140000, 2, 0);   /* 69 pages a plane, 138 pages each */
        load(1, "a", &H[0]); run(20000, 1);
        printf("a %d free %u\n", done_res[0], dkpool_free_frames());
        rc = load(2, "b", &H[1]);
        printf("b while a is ready: %d\n", rc);
        dkpool_release(&H[0]);
        printf("free with a cached %u\n", dkpool_free_frames());
        rc = load(3, "b", &H[1]); run(20000, 1);
        printf("b rc %d done %d planes %d a stale %d\n", rc, done_res[1], planes_ok(&H[1], 8), dkpool_check(&H[0]));
        int before = sends;
        dkpool_release(&H[1]);
        rc = load(4, "a", &H[0]);
        printf("a again rc %d\n", rc);
        run(20000, 1);
        printf("a sends %d planes %d\n", sends - before, planes_ok(&H[0], 7));
    } else if (!strcmp(s, "resend")) {
        make_file("a", 9, 3000, 2, 0);
        drop_sends = 1;
        load(1, "a", &H[0]); run(3000, 1);
        printf("done %d %d sends %d planes %d tag0 %x tag1 %x\n", done_n, done_res[0], sends, planes_ok(&H[0], 9), sent_tags[0], sent_tags[1]);
    } else if (!strcmp(s, "lostack")) {   /* the page is accepted but the acknowledgement is late: the resend is a repeat */
        make_file("a", 10, 3000, 2, 0);
        load(1, "a", &H[0]);
        run(3, 0);
        freeze_frames = 120;
        run(3000, 1);
        printf("done %d %d sends %d count %u planes %d\n", done_n, done_res[0], sends, d_count, planes_ok(&H[0], 10));
    } else if (!strcmp(s, "dsperr")) {
        make_file("a", 11, 3000, 2, 0); make_file("b", 12, 3000, 2, 0);
        force_err_tag = 1;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d free %u\n", done_res[0], dkpool_free_frames());
        load(2, "b", &H[1]); run(3000, 1);
        printf("b %d planes %d\n", done_res[1], planes_ok(&H[1], 12));
    } else if (!strcmp(s, "dead")) {
        make_file("a", 13, 3000, 2, 0);
        d_status = 3; d_err = 9;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d sends %d\n", done_res[0], sends);
        d_status = 1;
        rc = load(2, "a", &H[0]);
        printf("next %d\n", rc);
    } else if (!strcmp(s, "timeout")) {
        make_file("a", 14, 3000, 2, 0);
        drop_sends = 100;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d sends %d clock %u free %u\n", done_res[0], sends, clock_now - 1000, dkpool_free_frames());
    } else if (!strcmp(s, "unarmed")) {
        make_file("a", 15, 3000, 2, 0);
        d_status = 0;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d sends %d clock %u\n", done_res[0], sends, clock_now - 1000);
    } else if (!strcmp(s, "cancel")) {
        make_file("a", 16, 3000, 2, 0); make_file("b", 17, 3000, 2, 0); make_file("c", 18, 30000, 2, 0);
        load(1, "a", &H[0]); load(2, "b", &H[1]);
        dkpool_cancel(2);   /* b has not started */
        run(3000, 0);
        printf("done %d: %u %d, %u %d\n", done_n, done_req[0], done_res[0], done_req[1], done_res[1]);
        printf("a planes %d b blocks %u\n", planes_ok(&H[0], 16), sp_pool.blk[1].state);
        int before = sends;
        load(3, "c", &H[2]);
        run(3, 0);
        dkpool_cancel(3);   /* c is running */
        run(3000, 0);
        printf("c done %d: %u %d sends %d\n", done_n, done_req[2], done_res[2], sends - before);
        printf("free %u\n", dkpool_free_frames());
    } else if (!strcmp(s, "readfail")) {
        make_file("a", 19, 9000, 2, 0);
        read_fail_at = 2;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d free %u\n", done_res[0], dkpool_free_frames());
    } else if (!strcmp(s, "badfile")) {
        make_file("bad", 20, 3000, 2, 1);
        printf("missing %d bad %d\n", load(1, "nope", &H[0]), load(2, "bad", &H[0]));
        printf("free %u\n", dkpool_free_frames());
    } else if (!strcmp(s, "tokens")) {
        make_file("a", 21, 3000, 2, 0);
        dkpool_free_frames();
        sp_pool.tokens = 0xffffffffu;
        printf("rc %d\n", load(1, "a", &H[0]));
    } else if (!strcmp(s, "many")) {   /* the sequence number wraps (more than 256 pages), pages of all loads land right */
        char name[8]; int i, bad = 0;
        for (i = 0; i < 5; i++) { name[0] = 'f'; name[1] = '0' + i; name[2] = 0; make_file(name, 30 + i, 80 * 2048, 1, 0); }
        for (i = 0; i < 5; i++) {
            name[0] = 'f'; name[1] = '0' + i; name[2] = 0;
            load(1 + i, name, &H[0]); run(20000, 1);
            if (done_res[i] != 0 || planes_ok(&H[0], 30 + i) != 1) bad++;
            dkpool_release(&H[0]);
        }
        printf("sends %d bad %d last seq %u\n", sends, bad, spw_tag_seq(sent_tags[sends - 1]));
    } else if (!strcmp(s, "garble")) {
        make_file("a", 22, 3000, 2, 0);
        garble = 1;
        load(1, "a", &H[0]); run(3000, 1);
        printf("a %d sends %d\n", done_res[0], sends);
    } else {
        puts("unknown scenario");
        return 2;
    }
    return 0;
}
