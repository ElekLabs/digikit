/* shot.h: the internal header of the SHOT module. */
#ifndef SHOT_H
#define SHOT_H
#include <dk/core/module.h>
#include <dk/gen/machine.h>
#include <dk/platform/param.h>
#include <dk/sample/pool.h>
#include <dk/sample/view.h>
#include <dk/page/widgets.h>
#include <dk/platform/ui.h>
#include "shot_params.h"
#include "sample_view.h"

extern sample_view shot_samples; /* the pool view of both pages (module.c) */

void shot_controls_draw(const dk_page_frame *f);
void shot_range_draw(const dk_page_frame *f);
void shot_range_event(const dk_page_event *event);

#endif
