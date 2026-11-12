/* Public SDK only, no tick, firmware symbols, or backend assumptions. */
#include <dk/core/bytes.h>
#include <dk/core/module.h>
#include <dk/platform/files.h>
#include <dk/platform/host.h>
_Static_assert(__builtin_offsetof(dk_file_entry, hash) == 16, "hash bytes");
#define ROWS 32u
static dk_module_ctx *module_ctx;
static dk_files *files;
static volatile uint32_t client, ticket, old_ticket, count, callbacks, mode;
static volatile int32_t terminal, last_hint;
static volatile uint32_t terminal_ticket, hint_ticket;
static volatile uint32_t callback_seq;
static struct {
    uint32_t ticket;
    dk_file_entry row;
} rows[ROWS];
static uint32_t context_word = 0x46494c45;
static void event(uint32_t h, uint32_t t, int32_t hint, void *context)
{
    dk_file_entry row;
    ++callback_seq;
    ++callbacks;
    last_hint = hint;
    hint_ticket = t;
    if (context != (void *)&context_word || context_word != 0x46494c45) {
        terminal = -5;
        terminal_ticket = t;
    } else if (h == client && !(mode & 4u)) {
        /* Poll the supplied identity even if list has not returned to MIDI yet.
         * Append immutable ticket-tagged rows; never reset/overwrite them when a
         * new request supersedes an old callback. Host filters by exact ticket. */
        int32_t rc = dk_files_poll(files, t, &row, sizeof(row));
        if (rc == DK_FILES_ROW) {
            if (count == ROWS) {
                terminal = -13;
                terminal_ticket = t;
                dk_files_cancel(files, t);
            } else {
                rows[count].ticket = t;
                rows[count].row = row;
                __asm__ volatile("" ::: "memory");
                ++count;
                if (mode & 2u) {
                    dk_files_cancel(files, t);
                    terminal = -1;
                    terminal_ticket = t;
                }
            }
        } else if (rc != DK_FILES_PENDING) {
            terminal = rc;
            terminal_ticket = t;
        }
    }
    ++callback_seq;
}
/* ColdFire text must contain instructions, not dense switch-table data. */
__attribute__((optimize("no-jump-tables"))) static int32_t
command(uint32_t cmd, uint32_t seq, const void *request, uint32_t len, void *reply)
{
    const uint8_t *body = request;
    uint8_t *out = reply;
    (void)seq;
    if (cmd == 0x38) {
        if (len < 3 || body[len - 1] != 0)
            return -50;
        old_ticket = ticket;
        mode = body[1];
        int32_t t = dk_files_list(files, body[0], (const char *)body + 2, (mode & 1u) ? event : 0,
                                  &context_word);
        if (t < 0)
            return t;
        ticket = (uint32_t)t;
        dk_bytes_put32(out, ticket);
        return 4;
    }
    if (cmd == 0x39) {
        uint32_t seq = callback_seq;
        if (seq & 1u)
            return -12;
        dk_bytes_put32(out, client);
        dk_bytes_put32(out + 4, ticket);
        dk_bytes_put32(out + 8, count);
        dk_bytes_put32(out + 12, terminal_ticket == ticket ? (uint32_t)terminal : 0);
        dk_bytes_put32(out + 16, callbacks);
        dk_bytes_put32(out + 20, hint_ticket == ticket ? (uint32_t)last_hint : 0);
        dk_bytes_put32(out + 24, old_ticket);
        if (seq != callback_seq)
            return -12;
        return 28;
    }
    if (cmd == 0x3a) {
        if (len != 1 || body[0] >= count)
            return -1;
        dk_bytes_put32(out, rows[body[0]].ticket);
        const uint8_t *p = (const uint8_t *)&rows[body[0]].row;
        for (uint32_t i = 0; i < sizeof(dk_file_entry); ++i)
            out[4 + i] = p[i];
        return 4 + sizeof(dk_file_entry);
    }
    if (cmd == 0x3b) {
        if (len != 4)
            return -50;
        dk_bytes_put32(out, (uint32_t)dk_files_cancel(files, dk_bytes_get32(body)));
        return 4;
    }
    if (cmd == 0x3c) {
        if (len != 5)
            return -50;
        dk_file_entry row;
        uint8_t *p = (uint8_t *)&row;
        for (uint32_t i = 0; i < sizeof(row); ++i)
            p[i] = 0xa5;
        int32_t rc =
            dk_files_poll(files, dk_bytes_get32(body), p, body[4] ? sizeof(row) - 1 : sizeof(row));
        uint32_t untouched = 1;
        for (uint32_t i = 0; i < sizeof(row); ++i)
            if (p[i] != 0xa5)
                untouched = 0;
        dk_bytes_put32(out, (uint32_t)rc);
        dk_bytes_put32(out + 4, untouched);
        if (rc == 1) {
            for (uint32_t i = 0; i < sizeof(row); ++i)
                out[8 + i] = p[i];
            return 8 + sizeof(row);
        }
        return 8;
    }
    if (cmd == 0x3d) {
        dk_files *again = 0;
        dk_bytes_put32(out, dk_module_ctx_device(module_ctx));
        dk_bytes_put32(out + 4, dk_module_ctx_runtime_version(module_ctx));
        dk_bytes_put32(out + 8, sizeof(dk_file_entry));
        dk_bytes_put32(out + 12, (uint32_t)dk_files_open(module_ctx, &again));
        return 16; /* dk_files_open outside init must fail */
    }
    return -50;
}
static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_files_open(ctx, &files);
    if (r != DK_OK)
        return r;
    module_ctx = ctx;
    /* the handle is the client number the callback receives */
    client = (uint32_t)(uintptr_t)files;
    return dk_host_register(0x38, 6, command);
}

static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
    if (ticket)
        dk_files_cancel(files, ticket);
}

const dk_module_def dk_module = {.init = init, .fini = fini};
