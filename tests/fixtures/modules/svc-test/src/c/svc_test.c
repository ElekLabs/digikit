/* test/svc-test: a service provider of the test-only service svc-test (docs/plans/runtime-and-services.md section 13.2).
 *
 * It exports dk_svctest_ping, which returns 42, and dk_svctest_hits, how often its hook ran. The hook is the firmware's `cf.menu.hook.settings_list.tail`
 * (the last step of the SETTINGS list rebuild) as the test-only hook `cf.menu.hook.settings_list.test`: the runtime installs a jump to the routine `svc_test_hook`
 * (src/cf/hook.s) after `init`; it calls `svc_test_settings_tail` and runs the tail. `init` repeats the hook and the exports with dk_service_declare_*: the runtime compares them with the HOOKS and EXPORTS of the header
 * table at the commit. The test-only imports have no public header, so the prototypes are declared here.
 */

#include <dk/core/module.h>
#include <dk/gen/imports.h>

int dk_svctest_ping(void);
uint32_t dk_svctest_hits(void);
void svc_test_settings_tail(uint8_t *menu);
void svc_test_hook(
    void); /* src/cf/hook.s: saves registers, calls svc_test_settings_tail(a2), runs the tail */

static uint32_t hits;

int dk_svctest_ping(void)
{
    return 42;
}

uint32_t dk_svctest_hits(void)
{
    return hits;
}

/* The hook handler: `menu` is the settings menu (a2 at the site). It counts the call and changes nothing. */
void svc_test_settings_tail(uint8_t *menu)
{
    (void)menu;
    hits++;
}

/* FNV-1a 32 of a name: the identity of a hook site and of an export in the header table. */
static uint32_t name_hash(const char *s)
{
    uint32_t h = 0x811c9dc5u;
    while (*s) {
        h ^= (uint8_t)*s++;
        h *= 0x01000193u;
    }
    return h;
}

static int32_t init(dk_module_ctx *ctx)
{
    int32_t r = dk_service_declare_hook(ctx, name_hash("cf.menu.hook.settings_list.test"),
                                        (const void *)svc_test_hook);
    if (r != 0)
        return r;
    r = dk_service_declare_export(ctx, name_hash("dk_svctest_ping"), (const void *)dk_svctest_ping);
    if (r != 0)
        return r;
    return dk_service_declare_export(ctx, name_hash("dk_svctest_hits"),
                                     (const void *)dk_svctest_hits);
}

const dk_module_def dk_module = {.init = init};
