/* DN2 1.12 emulator-only, boot-scoped reply extension. One immutable staged
 * request is retained until a valid CF-observed DSP completion. The stock LP0
 * sender remains unbounded; the external runner must bound the owned emulator.
 * No public generation contract, cache guarantee or hardware ownership claim. */
typedef unsigned int u32;
typedef unsigned short u16;
#define LP_START 1u
#define LP_BUSY 2u
#define LP_DONE 3u
#define LP_ERROR 4u
#define LP_PAGES 235u
#define LP_BYTES 960000u
#define LP_STOCK_BYTES 0xabcu

struct LpMailbox { volatile u32 state, error, tag, sent; };
struct LpAck { volatile u32 sequence, valid, status, error, pages, bytes; };
struct LpMailbox lp_mail __attribute__((aligned(16)));
struct LpAck lp_ack __attribute__((aligned(16)));
volatile u32 lp_staging[1024] __attribute__((aligned(16)));
static volatile u32 lp_failed, lp_bad_reply;
static u32 lp_next, lp_sent;
extern void lp_stock_send(u32 tag, const volatile u32 *source, u32 format);
extern void *lp_stock_copy(void *destination, const void *source, u32 length);

/* Retarget only the boot getter CALL. The original getter and stock copy
 * argument remain 0xabc. Both CPUs must be initialized with this wire size. */
u32 lp_wire_size(void) { return 0xaccu; }

static u32 progress_bytes(u32 pages)
{
    return pages == LP_PAGES ? LP_BYTES : pages * 4096u;
}

/* Called at the original memcpy call site, BEFORE the frame driver rearms DMA.
 * Preserve the complete stock copy and return ABI. The extra 16 bytes never land
 * at stock destination+0xabc, which is the stock TX buffer's first byte. */
void *lp_copy_reply(void *destination, const void *source, u32 length)
{
    void *result = lp_stock_copy(destination, source, length);
    u32 valid = 2, status = 0, error = 0, pages = 0, bytes = 0;
    if (length == LP_STOCK_BYTES && source && destination) {
        const volatile u16 *extra = (const volatile u16 *)source + LP_STOCK_BYTES / 2;
        u32 magic = extra[0], version = extra[1];
        status = extra[2]; error = extra[3];
        pages = (u32)extra[4] | ((u32)extra[5] << 16);
        bytes = (u32)extra[6] | ((u32)extra[7] << 16);
        if (magic != 0x4c50u) valid = 0; /* missing/not-yet-published reply */
        else if (magic == 0x4c50u && version == 1 && pages <= LP_PAGES &&
                 bytes == progress_bytes(pages) && status <= 4 &&
                 ((status < 2 && !pages && !error) ||
                  (status == 2 && pages < LP_PAGES && !error) ||
                  (status == 3 && pages == LP_PAGES && !error) ||
                  (status == 4 && error))) valid = 1;
    }
    if (valid == 2 || (valid == 1 && (error || status == 4))) lp_bad_reply = 1;
    /* Do not republish an identical snapshot every audio frame. This keeps the
     * local sequence stable while progress is unchanged (not a wire epoch). */
    if (lp_ack.valid == valid && lp_ack.status == status && lp_ack.error == error &&
        lp_ack.pages == pages && lp_ack.bytes == bytes) return result;
    /* One ISR writer; UI accepts only matching even sequence observations. */
    ++lp_ack.sequence;
    lp_ack.valid = valid;
    lp_ack.status = status; lp_ack.error = error;
    lp_ack.pages = pages; lp_ack.bytes = bytes;
    ++lp_ack.sequence;
    return result;
}

static void fail(u32 error)
{
    lp_failed = 1;
    lp_mail.error = error;
    lp_mail.state = LP_ERROR;
}

void lp_tick(void)
{
    u32 sequence, valid, status, error, pages, bytes;
    if (lp_failed) { lp_mail.state = LP_ERROR; return; }
    if (lp_bad_reply) { fail(2); return; }
    if (!lp_sent && lp_mail.state != LP_START) return;
    if (lp_next >= LP_PAGES || lp_mail.tag != lp_next) { fail(1); return; }
    /* Single bounded seqlock attempt: a concurrent ISR leaves this pending. */
    sequence = lp_ack.sequence;
    if (sequence & 1u) return;
    valid = lp_ack.valid; status = lp_ack.status; error = lp_ack.error;
    pages = lp_ack.pages; bytes = lp_ack.bytes;
    if (sequence != lp_ack.sequence) return;
    if (!valid) return;
    if (valid != 1 || error || status == 4) { fail(2); return; }
    if (!lp_sent) {
        if (pages < lp_next || status < 2) return;
        if (pages != lp_next || bytes != progress_bytes(lp_next) || status != 2) { fail(3); return; }
        lp_mail.state = LP_BUSY;
        lp_sent = 1; /* Set before the unbounded call: polling can never resend. */
        lp_stock_send(lp_next, lp_staging, 1);
        lp_mail.sent = lp_next + 1;
        return; /* Sender return alone is not DSP completion. */
    }
    if (pages < lp_next + 1) return; /* old but valid reply, no resend */
    if (pages != lp_next + 1 || bytes != progress_bytes(lp_next + 1) ||
        status != (pages == LP_PAGES ? 3u : 2u)) { fail(3); return; }
    ++lp_next;
    lp_sent = 0;
    lp_mail.state = LP_DONE;
}
