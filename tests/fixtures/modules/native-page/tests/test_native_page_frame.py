"""The neutral callback reports H and level independently and rejects short frames."""

import shutil
import subprocess

import pytest
from support.paths import REPO_ROOT


@pytest.mark.skipif(shutil.which("cc") is None, reason="no host C compiler")
def test_native_frame_level_and_lifetime_bounds(tmp_path):
    source = tmp_path / "frame.c"
    source.write_text(
        r"""
#include <assert.h>
#include <string.h>
#include "tests/fixtures/modules/native-page/src/module.c"
/* init is not run here: the builder imports only have to link */
dk_machine *dk_module_ctx_add_machine(dk_module_ctx *c, uint32_t i, const char *n, const char *s) { (void)c; (void)i; (void)n; (void)s; return 0; }
dk_page *dk_machine_add_page(dk_machine *m, const char *n, const char *s) { (void)m; (void)n; (void)s; return 0; }
dk_param *dk_page_add_param(dk_page *p, const char *n, const char *s, uint32_t d) { (void)p; (void)n; (void)s; (void)d; return 0; }
void dk_page_canvas(dk_page *p, uint32_t f, dk_page_draw_fn *d, dk_page_event_fn *e, void *a) { (void)p; (void)f; (void)d; (void)e; (void)a; }
void dk_page_bind(dk_page *p, dk_param *q, uint32_t s) { (void)p; (void)q; (void)s; }
void dk_page_bind_local(dk_page *p, uint32_t l, uint32_t s) { (void)p; (void)l; (void)s; }
int32_t dk_host_register(uint32_t a, uint32_t b, dk_host_cmd_fn *f) { (void)a; (void)b; (void)f; return 0; }
static void text(int32_t x, int32_t y, const char *s) { (void)x; (void)y; (void)s; }
static void rect(int32_t a, int32_t b, int32_t c, int32_t d, int32_t color) {
    (void)a; (void)b; (void)c; (void)d; (void)color;
}
int main(void) {
    dk_page_frame f = {.version=1, .size=sizeof(f), .track=0,
        .params={[7]=112<<8}, .canvas={99,53,text,rect}, .level=12345};
    native_draw(NULL, &f);
    assert(native_draws == 1 && native_end_seen == 112<<8 && native_level_seen == 12345);
    f.params[7] = 122<<8;
    native_draw(NULL, &f);
    assert(native_end_seen == 122<<8 && native_level_seen == 12345 && f.level == 12345);
    f.level = 6789;
    native_draw(NULL, &f);
    assert(native_end_seen == 122<<8 && native_level_seen == 6789);
    uint8_t out[28];
    assert(status(0,0,0,0,out) == sizeof(out));
    assert(out[24] == 0 && out[25] == 0 && out[26] == (6789>>8) && out[27] == (6789&255));
    f.size = offsetof(dk_page_frame, level);
    native_draw(NULL, &f);
    assert(native_draws == 3);
    f.size = sizeof(f); f.track = 16;
    native_draw(NULL, &f);
    assert(native_draws == 3);
    return 0;
}
"""
    )
    binary = tmp_path / "frame"
    subprocess.run(
        [
            "cc",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            str(REPO_ROOT),
            "-I",
            str(REPO_ROOT / "sdk/include"),
            "-I",
            str(REPO_ROOT / "tests/fixtures/modules/native-page/build/gen"),
            str(source),
            "-o",
            str(binary),
        ],
        check=True,
    )
    subprocess.run([str(binary)], check=True)
