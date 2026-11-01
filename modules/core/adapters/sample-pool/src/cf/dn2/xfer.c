/* core/sample-pool on the Digitone II: the page transport (docs/design/dn2-sample-pool.md section 2).
 *
 * One load runs at a time. Its pages go to the DSP over the stock LP0 sender (`sp_stock_send`, format 1) one at a time: a page is sent only
 * when the receiver reported itself armed, and it counts as written when the receiver's acknowledgement names it (stop and wait, bounded
 * by wall time in DSP blocks, resent up to three times). The acknowledgement rides in a 16-byte extension of the DSP's reply record; the
 * hook `sp_copy_reply` on the stock reply copy decodes it in the interrupt into a snapshot that dkpool_tick reads (pool_wire.h has the
 * format). Everything else runs in the UI-loop task from sp_xfer_tick: one send and one card read plus conversion per pass, so the read of
 * the next page overlaps the round trip of the one in flight. The result goes to the runtime only from there (dkrt_pool_done). */

#include <stdint.h>
#include "cf/dn2/xfer.h"

typedef uint32_t u32;
typedef int32_t i32;
typedef uint8_t u8;
typedef uint16_t u16;

extern void sp_stock_send(u32 tag, const volatile u32 *source, u32 format);
extern void *sp_stock_copy(void *destination, const void *source, u32 length);

#define SP_SLOTS 4u            /* 4 KiB output pages: a stereo chunk takes two, the next chunk is converted while the first one goes out */
#define SP_STOCK_REPLY 0xabcu  /* the stock reply copy is this long; the extension follows it (the wire is 0xacc bytes long) */

static u8 rbuf[SPW_RBUF_SECTORS * SPW_SECTOR] __attribute__((aligned(16)));   /* one chunk as it is in the file */
static u8 pages[SP_SLOTS][SPA_PAGE_BYTES] __attribute__((aligned(16)));       /* converted pages, FIFO */

static spw_snap snap;           /* the last good acknowledgement; written by sp_copy_reply in the interrupt */

static struct {
    SpReq *r;
    spw_x x;
    u32 channels, plane_pages, first_page, frames;
    u32 chunks_total, chunk_next;   /* chunks read and converted so far */
    u32 pages_total, pages_done;    /* pages acknowledged */
    u32 head, n;                    /* FIFO of the converted pages: oldest slot and count */
    i32 fail;                       /* a card read failed: end the load when no page is in flight */
    u32 page_index[SP_SLOTS];       /* destination page of each slot */
} job;

/* ---- The two hooks on the stock link ---------------------------------------------------------------------------------------------------- */

/* Boot-only replacement for the wire length getter: the DSP builds all its SPI2 descriptors with this length too (patched in the same
 * op list, `dsp.link.site.boot_wire_size`). */
u32 sp_wire_size(void) { return SP_STOCK_REPLY + 16u; }

/* The stock reply copy (0xabc bytes) with the extension read after it. Interrupt context; the only writer of `snap`. */
void *sp_copy_reply(void *destination, const void *source, u32 length)
{
    void *result = sp_stock_copy(destination, source, length);
    spw_ack a;
    int ok = 0;
    if (length == SP_STOCK_REPLY && source && destination)
        ok = spw_ack_decode((const volatile u16 *)((const u8 *)source + SP_STOCK_REPLY), &a);
    if (!ok) {
        if (snap.valid)   /* fail closed, once */
            spw_snap_store(&snap, 0, &a);
        return result;
    }
    if (snap.valid && snap.status == a.status && snap.error == a.error && snap.errcount == a.errcount && snap.count == a.count &&
        snap.last == a.last)
        return result;   /* the same observation again keeps the sequence */
    spw_snap_store(&snap, 1, &a);
    return result;
}

/* ---- One load --------------------------------------------------------------------------------------------------------------------------- */

static void finish(i32 result)
{
    SpReq *r = job.r;
    u32 req = r->req;
    if (result == 0) {
        spa_ready(&sp_pool, r->blk);
        spa_fill(&sp_pool, r->blk, r->out);
    } else {
        spa_abort(&sp_pool, r->blk);
        if (r->cancelled)
            result = DKPOOL_E_LOST;
    }
    r->state = RQ_FREE;
    job.r = 0;
    dkrt_pool_done(req, result);
}

/* Read the next chunk from the card and convert it into one page (mono) or two (stereo). */
static i32 prepare(void)
{
    spw_chunk c;
    u32 c_idx = job.chunk_next, slot, l;
    spw_chunk_of(job.channels, job.frames, c_idx, &c);
    if (dkrt_store_read(&job.r->file, c.sector, c.sectors, rbuf) != 0)
        return DKPOOL_E_READ;
    slot = (job.head + job.n) % SP_SLOTS;
    l = job.first_page + c_idx;
    if (job.channels == 1) {
        spw_page_mono(rbuf + c.skip, c.frames, pages[slot]);
        job.page_index[slot] = l;
        job.n++;
    } else {
        u32 slot_r = (slot + 1u) % SP_SLOTS;
        spw_page_stereo(rbuf + c.skip, c.frames, pages[slot], pages[slot_r]);
        job.page_index[slot] = l;
        job.page_index[slot_r] = job.first_page + job.plane_pages + c_idx;
        job.n += 2;
    }
    job.chunk_next++;
    return 0;
}

static void start_job(SpReq *r)
{
    const spa_block *b = &sp_pool.blk[r->blk];
    spw_begin(&job.x, dkrt_blocks());
    job.r = r;
    job.channels = b->channels;
    job.plane_pages = b->plane_pages;
    job.first_page = b->first_page;
    job.frames = b->frames;
    job.chunks_total = b->plane_pages;
    job.chunk_next = 0;
    job.pages_total = b->plane_pages * b->channels;
    job.pages_done = 0;
    job.head = job.n = 0;
    job.fail = 0;
    r->state = RQ_ACTIVE;
}

static i32 map_error(i32 poll)
{
    if (poll == SPW_E_TAG)
        return DKPOOL_E_FORMAT;
    return DKPOOL_E_READ;
}

/* The oldest queued request, or 0. A queued request nobody waits for is finished here without touching the link. */
static SpReq *next_queued(void)
{
    SpReq *best = 0;
    u32 i;
    for (i = 0; i < SP_REQS; i++) {
        SpReq *r = &sp_reqs[i];
        if (r->state != RQ_QUEUED)
            continue;
        if (r->cancelled) {
            spa_abort(&sp_pool, r->blk);
            r->state = RQ_FREE;
            dkrt_pool_done(r->req, DKPOOL_E_LOST);
            continue;
        }
        if (!best || (int32_t)(r->age - best->age) < 0)
            best = r;
    }
    return best;
}

void sp_xfer_tick(void)
{
    spw_ack a;
    int have;
    int32_t p;
    u32 now, armed = 0;
    if (!job.r) {
        SpReq *r = next_queued();
        if (!r)
            return;
        start_job(r);
    }
    now = dkrt_blocks();
    have = spw_snap_read(&snap, &a);
    /* A cancelled load that has no page in flight stops here; one with a page in flight waits for that page's outcome, so the link
     * and the receiver's counters are settled when the next load starts. */
    if ((job.r->cancelled || job.fail) && !job.x.inflight) {
        finish(job.r->cancelled ? DKPOOL_E_LOST : job.fail);
        return;
    }
    p = spw_poll(&job.x, now, have ? &a : 0);
    if (p < 0) {
        if (p == SPW_E_DEAD)
            sp_link_dead = 1;
        finish(map_error(p));
        return;
    }
    if (p == SPW_ACKED) {
        job.head = (job.head + 1u) % SP_SLOTS;
        job.n--;
        job.pages_done++;
        if (job.pages_done == job.pages_total) {
            finish(0);
            return;
        }
        armed = 1;   /* the acknowledging snapshot is armed and idle */
    } else if (p == SPW_READY) {
        armed = 1;
    } else if (p == SPW_RESEND) {
        sp_stock_send(job.x.tag, (const volatile u32 *)pages[job.head], 1);
        spw_resent(&job.x, now);
    }
    /* 2. one send, only to an armed receiver */
    if (armed && !job.r->cancelled && !job.fail && !job.x.inflight && job.n > 0) {
        u32 tag = spw_next_tag(&job.x, job.page_index[job.head]);
        sp_stock_send(tag, (const volatile u32 *)pages[job.head], 1);
        spw_sent(&job.x, tag, now);
    }
    /* 3. read and convert the next chunk while the sent page waits for its acknowledgement */
    if (!job.r->cancelled && !job.fail && job.chunk_next < job.chunks_total && job.n + job.channels <= SP_SLOTS) {
        i32 rc = prepare();
        if (rc != 0)
            job.fail = rc;   /* ends at the next pass, when no page is in flight */
    }
}

/* dkpool_cancel found the request: nothing to do here beyond the flag, which the tick reads. */
void sp_xfer_cancel(SpReq *r) { r->cancelled = 1; }
