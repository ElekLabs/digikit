/* The machine of digikit/shot, declared by builder calls in init: one stereo machine `shot` that plays a sample of its pool, with five
   parameters shown through a knob page and two canvas pages.
   The DSP symbols are in src/dsp/shot.s; the parameter words (src/shot_params.h) are shared with it.
   src/pages.c draws the two canvas pages; src/sample_view.c reads the pool slot they show.
   The pool (16 entries) is the runtime's per-module table (<dk/sample/pool.h>); SMP picks the entry of a note. The module only plays;
   files are loaded into pool slots by the picker (the SMP click list) through the runtime. */
#include <dk/sample/picker.h>
#include "shot.h"
#include "module_ids.h"

sample_view shot_samples;
static dk_ui *g_ui;
static uint32_t
    click_track; /* the track of the page the SMP click came from: where the pick sets SMP */

static void shot_smp_click(void *arg, uint32_t track);

/* The page callbacks of src/pages.c take no state: these adapt them to dk_page_draw_fn and dk_page_event_fn. */
static void controls_draw(void *arg, const dk_page_frame *f)
{
    (void)arg;
    shot_controls_draw(f);
}

static void range_draw(void *arg, const dk_page_frame *f)
{
    (void)arg;
    shot_range_draw(f);
}

static void range_event(void *arg, const dk_page_event *event)
{
    (void)arg;
    shot_range_event(event);
}

static int32_t init(dk_module_ctx *ctx)
{
    dk_sample_pool *pool;
    dk_files *files;
    int32_t r;

    dk_machine *m = dk_module_ctx_add_machine(ctx, DK_MACHINE_ID_SHOT, "SHOT", "SHT");
    /* SMP: the pool entry that plays, 0 .. 15 (turn: steps through the pool; p-lockable; an empty entry is silent).
       START and END: 0 .. 127 spread over the whole sample (127 is its last frame boundary). PLAY: 0 REV, 1 REV.L, 2 FWD.L, 3 FWD (default); .L loops
       START .. END (the glyph is the stock PLAY knob on the Digitakt II).
       PITCH: 64 is the played note unchanged, each unit is 0.375 semitones, so 0 .. 127 is -24 .. +23.6 semitones.
       The words are in the order of SHOT_P_*: SMP, START, END, PLAY, PITCH. */
    dk_page *knobs = dk_machine_add_page(m, "SHOT", "SHT");
    dk_param *smp = dk_page_add_param(knobs, "Sample", "SMP", 0);
    dk_param_range(smp, 0, 15);
    dk_param_latched(smp);
    dk_param_on_click(smp, shot_smp_click, 0);
    dk_param *start = dk_page_add_param(knobs, "Start", "START", 0);
    dk_param *end = dk_page_add_param(knobs, "End", "END", 127);
    dk_param *play = dk_page_add_param(knobs, "Play", "PLAY", 3);
    dk_param_range(play, 0, 3);
    dk_param_glyph(play, DK_GLYPH_PLAY);
    dk_param *pitch = dk_page_add_param(knobs, "Pitch", "TUNE", 64);

    dk_page *controls = dk_machine_add_page(m, "SHOT", "SHT");
    dk_page_canvas(controls, DK_PAGE_STOCK_OVERLAY, controls_draw, range_event, 0);
    dk_page_bind(controls, pitch, 0);
    dk_page_bind(controls, play, 1);
    dk_page_bind(controls, smp, 3);
    dk_page_bind(controls, start, 4);
    dk_page_bind(controls, end, 7);

    dk_page *wave = dk_machine_add_page(m, "SHOT WAVE", "SHT");
    dk_page_canvas(wave, DK_PAGE_WIDE_CANVAS, range_draw, range_event, 0);
    dk_page_bind(wave, start, 4);
    dk_page_bind_local(wave, 0, 6);
    dk_page_bind(wave, end, 7);

    r = dk_sample_pool_open(ctx, &pool);
    if (r != DK_OK)
        return r;
    sample_view_init(&shot_samples, pool);
    /* the picker is optional: when it cannot start, a click on SMP does nothing */
    if (dk_files_open(ctx, &files) == DK_OK && dk_ui_open(ctx, &g_ui) == DK_OK)
        dk_sample_picker_init(pool, files, g_ui);
    return DK_OK;
}

static void fini(dk_module_ctx *ctx)
{
    (void)ctx;
    dk_sample_picker_fini();
    g_ui = 0;
    sample_view_init(&shot_samples, 0);
}

const dk_module_def dk_module = {.init = init, .fini = fini};

/* The pick: SMP of the clicked track becomes the pool index (raw = unit * 256). The lists have closed, the page is on top. A refusal shows on
   the page, not in a dialog. */
static void smp_pick(void *arg, int32_t index)
{
    int32_t rc;
    (void)arg;
    rc = dk_param_set(click_track, SHOT_P_SMP, index << 8);
    if (rc == DK_PARAM_E_HELD)
        dk_ui_toast(g_ui, "RELEASE TRIG TO SET SMP");
    else if (rc < 0)
        dk_ui_toast(g_ui, "SMP NOT SET");
}

/* Click on SMP at D (release without a turn on the custom SHOT page): the runtime's page hook calls this in the UI task with the track of the page.
   It opens the slot list over the page (sdk/src/sample/picker.c) with the current SMP value marked and the cursor on it. YES on a slot opens
   the file browser for it; YES on a file loads it into that slot, sets SMP to the slot and returns to the page in one step
   (docs/design/ui-rules.md); NO goes back one level. */
static void shot_smp_click(void *arg, uint32_t track)
{
    int32_t raw;
    (void)arg;
    click_track = track;
    raw = dk_param_get(track, SHOT_P_SMP);
    dk_sample_picker_open_pool(DK_PARAM_IS_ERROR(raw) || raw < 0 ? -1 : (int32_t)(raw >> 8),
                               smp_pick, 0);
}
