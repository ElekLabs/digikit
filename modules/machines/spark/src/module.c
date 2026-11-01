/* The machine declaration of digikit/spark: one stereo machine `spark` that plays the sample bound to the track it runs on, with four
   parameters on one page. patchkit pack reads these records; the device never sees them. The DSP symbols are in src/dsp/spark.s;
   the parameter words are read there in this order: START, END, REVERSE, PITCH.
   The sample comes from the runtime's per-track table (<dk/sample_track.h>): the module only plays; it never selects a file. */
#include <dk/api.h>
#include <dk/machine.h>
#include <dk/sample_picker.h>
#include <dk/sample_track.h>
#include <dk/ui.h>

DK_MACHINE(spark, .name = "SPARK", .short_name = "SPK", .render = "_spark_render",
           .note_on = "_spark_note_on", .desc = "_spark_desc", .stereo = 1);
/* START and END: 0 .. 127 spread over the whole sample (127 is its last frame boundary). REVERSE: 64 and above plays backwards.
   PITCH: 64 is the played note unchanged, each unit is 0.375 semitones, so 0 .. 127 is -24 .. +23.6 semitones. */
DK_PARAM(spark, 0, spark_start, "Start", "START", 0, 127, 0);
DK_PARAM(spark, 1, spark_end, "End", "END", 0, 127, 127);
DK_PARAM(spark, 2, spark_reverse, "Reverse", "REV", 0, 127, 0);
DK_PARAM(spark, 3, spark_pitch, "Pitch", "PITCH", 0, 127, 64);
DK_PAGE(spark, "SPK", "SPARK", DK_KNOB(0), DK_KNOB(1), DK_KNOB(2), DK_KNOB(3));
DK_SAMPLE_TRACKS(0, _spark_rec, _spark_ctl);

static uint32_t tracks; /* the runtime's handle of the 16-track table; the picker and the store select files with it */

int32_t dkm_init(const dk_api *api)
{
    int32_t handle;
    if (api->version != 1 || api->size < offsetof(dk_api, sample_track_info) + sizeof(api->sample_track_info)
        || !api->sample_tracks)
        return DK_SAMPLE_E_CONTRACT;
    handle = api->sample_tracks(0);
    if (handle <= 0)
        return handle < 0 ? handle : DK_SAMPLE_E_CONTRACT;
    tracks = (uint32_t)handle;
    dk_picker_init(api, tracks); /* the picker is optional: when it cannot start, the SAMPLES... row is absent */
    return 0;
}

void dkm_fini(const dk_api *api)
{
    (void)api;
    dk_picker_fini();
    tracks = 0;
}

/* SETTINGS... of this module in DIGIKIT > MODULES: the sample picker (sdk/src/sample_picker.c). */
static void spark_menu(dk_ui_menu *m, void *ctx)
{
    (void)ctx;
    dk_picker_open(m);
}
DK_MENU(spark_menu)
