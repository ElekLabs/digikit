/* Host test of digikit/remote's settings: the module's own C source, compiled here with fakes of core/input and of the runtime's
   imports (the new entry of <dk/core/module.h>). The fake <dk/platform/ui.h> (src/lib.rs) shows the page, and modules/core/adapters/ui/host/tests/remote.rs presses the keys. */
#include "../../../../../../tools/remote/src/c/remote.c"

static dk_host_cmd_fn *handlers[256];
static int key_state[128];
static int key_calls, next_result;

/* modules/tools/remote/tests/remote-pacing.c checks the INPUT_TAKEN records; the settings page test here ignores them. */
void dk_log_event(uint32_t kind, uint32_t flags, const uint32_t *a)
{
    (void)kind;
    (void)flags;
    (void)a;
}

static int input_token;

int32_t dk_input_open(dk_module_ctx *ctx, dk_input **out)
{
    (void)ctx;
    *out = (dk_input *)&input_token;
    return 0;
}

uint32_t dk_time_blocks(void)
{
    return 0;
}

int32_t dk_input_key(dk_input *in, uint32_t code, uint32_t down)
{
    (void)in;
    key_calls++;
    if (next_result) {
        int r = next_result;
        next_result = 0;
        return r;
    }
    key_state[code & 127] = (int)down;
    return 0;
}

int32_t dk_input_turn(dk_input *in, uint32_t encoder, int32_t delta)
{
    (void)in;
    (void)encoder;
    (void)delta;
    return 0;
}

int32_t dk_host_register(uint32_t first, uint32_t count, dk_host_cmd_fn *handler)
{
    for (uint32_t c = first; c < first + count && c < 256; c++)
        handlers[c] = handler;
    return 0;
}

void dk_log(uint32_t level, const char *message, uint32_t value)
{
    (void)level;
    (void)message;
    (void)value;
}

static void fill_page(dk_ui_menu *m, void *arg)
{
    (void)arg;
    dk_module.menu(0, m);
}

static const dk_ui_menu_desc page = {
    .name = "RemoteSettings", .title = "REMOTE CONTROL", .fill = fill_page};

int r_init(void)
{
    static int ctx_token;
    return (int)dk_module.init((dk_module_ctx *)&ctx_token);
}

int r_cmd(int cmd, const uint8_t *body, int len)
{
    uint8_t out[4];
    return handlers[cmd & 255]((uint32_t)cmd, 1, body, (uint32_t)len, out);
}

dk_ui_menu *r_page(void)
{
    return dk_ui_menu_new(0, &page);
}
int r_key_down(int code)
{
    return key_state[code & 127];
}
int r_key_calls(void)
{
    return key_calls;
}
void r_fail_next(int result)
{
    next_result = result;
}
