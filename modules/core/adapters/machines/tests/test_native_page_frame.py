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


@pytest.mark.skipif(shutil.which("cc") is None, reason="no host C compiler")
def test_native_page_dot_order_in_bottom_origin_bitmap(tmp_path):
    source = tmp_path / "dots.c"
    source.write_text(
        r"""
#include <assert.h>
#include <string.h>
#include "modules/core/adapters/machines/src/cf/native-page-dots.h"
static uint8_t pixels[64][128];
static void rect(uint8_t *canvas, int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color) {
    (void)canvas;
    for (int y=y0;y<=y1;y++) for(int x=x0;x<=x1;x++) pixels[63-y][x]=color!=0;
}
static unsigned count(int first,int last) {
    unsigned n=0;
    for(int y=first;y<=last;y++) for(int x=124;x<128;x++) n+=pixels[y][x];
    return n;
}
int main(void) {
    native_page_dots(0,0,rect);
    assert(count(28,33)>count(35,41));
    memset(pixels,0,sizeof(pixels)); native_page_dots(0,1,rect);
    assert(count(28,33)<count(35,41));
    memset(pixels,0,sizeof(pixels)); native_page_dots(0,2,rect);
    assert(count(0,63)==0);
    return 0;
}
"""
    )
    binary = tmp_path / "dots"
    subprocess.run(
        [
            "cc",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            str(REPO_ROOT),
            str(source),
            "-o",
            str(binary),
        ],
        check=True,
    )
    subprocess.run([str(binary)], check=True)


@pytest.mark.skipif(shutil.which("cc") is None, reason="no host C compiler")
def test_native_overlay_mask_preserves_stock_and_clips(tmp_path):
    source = tmp_path / "mask.c"
    source.write_text(
        r"""
#include <assert.h>
#include "modules/core/adapters/machines/src/cf/native-page-mask.h"
static uint32_t mask[99*2];
static int visible(int x,int y) {
    int row=52-y;
    return (mask[x*2+row/32]>>(31-row%32))&1;
}
int main(void) {
    native_page_mask_rect(mask,99,53,0,0,22,19);
    native_page_mask_rect(mask,99,53,24,0,47,19);
    native_page_mask_rect(mask,99,53,76,0,98,19);
    for(int x=0;x<99;x++) for(int y=0;y<53;y++) {
        int expected=y<=19 && (x<=22 || (x>=24&&x<=47) || x>=76);
        assert(visible(x,y)==expected);
        int stock=1, overlay=0;
        assert((visible(x,y)?overlay:stock)==!expected);
    }
    native_page_mask_rect(mask,99,53,-10,-10,200,100);
    for(int x=0;x<99;x++) { assert(mask[x*2]==UINT32_MAX); assert(mask[x*2+1]==(UINT32_MAX<<11)); }
    return 0;
}
"""
    )
    binary = tmp_path / "mask"
    subprocess.run(
        [
            "cc",
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-I",
            str(REPO_ROOT),
            str(source),
            "-o",
            str(binary),
        ],
        check=True,
    )
    subprocess.run([str(binary)], check=True)
