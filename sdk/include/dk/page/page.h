/* <dk/page/page.h>: what a canvas page callback receives (`dk_page_canvas`, `dk_page_bind`) and the page flags. Hand-written.
   A page is built in `init` with the builder calls of <dk/gen/imports.h> (`dk_machine_add_page`, `dk_fx_add_page`, `dk_page_add_param`,
   `dk_param_glyph`, `dk_page_canvas`). The glyph and route constants are in <dk/gen/page_consts.h>. */
#ifndef DK_PAGE_H
#define DK_PAGE_H
#include <stdint.h>
#include <dk/gen/page_consts.h>

/* Canvas page (machine contract 1.4). PARAM bindings edit existing words through the firmware; LOCAL bindings never edit a stored word.
   Canvas coordinates are local to its clipped content rectangle. Functions are valid only during draw.
   Callbacks run in the UI task. Do not retain the frame/event or call canvas functions outside draw. */
typedef struct dk_page_canvas_v1 {
    uint32_t width, height;
    void (*text)(int32_t x, int32_t y, const char *text);
    void (*fill_rect)(int32_t x0, int32_t y0, int32_t x1, int32_t y1, int32_t color);
} dk_page_canvas_v1;

typedef struct dk_page_frame dk_page_frame; /* also declared by <dk/gen/imports.h> */
struct dk_page_frame {
    uint32_t version, size, track;
    int32_t params[8]; /* current native PARAM values, indexed by encoder position */
    dk_page_canvas_v1 canvas;
    /* Snapshot of this machine's declared parameters, in declaration order.
       word_count is 0..8; registration rejects a larger snapshot. Independent
       of encoder bindings; read-only and valid for this draw only. */
    uint32_t word_count;
    int32_t word_values[8];
    int32_t level; /* read-only native track level; independent of A-H bindings */
};

typedef struct dk_page_event dk_page_event; /* also declared by <dk/gen/imports.h> */
struct dk_page_event {
    uint32_t version, size, track, control;
    int32_t delta; /* signed detents, independent of PARAM coarse/p-lock editing */
    uint32_t coarse;
};

/* Page-zero stock overlay: native widgets draw first, then the callback. The
   frame ABI is unchanged. Canvas stays clipped to 99x53 local pixels. Only
   rectangles explicitly filled by the callback are composited; clear each
   intended overlay area before drawing text/glyphs there. Other stock pixels,
   including LEV and page dots outside the canvas, remain intact. Renderer
   internals are adapter-owned and may change without changing this contract. */
#define DK_PAGE_STOCK_OVERLAY 1u
/* Replacement page one may expand left to 107x53 pixels (global x17..123).
   Mutually exclusive with STOCK_OVERLAY; ordinary pages remain 99x53. */
#define DK_PAGE_WIDE_CANVAS 2u

#endif
