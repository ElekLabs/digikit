//! The constants of the runtime's log that the resident core and the system package both use: the sink channel of the log, the flags of an info event and the
//! phases of module code (the crash record keeps the phase).

/// Sink channel number of the event log (`dk_sink_set`, <dk/core/sink.h>).
pub const SINK_LOG: u32 = 0;

/// The flags of a non-error event at the info level (an error event is `logk::F_ERROR`: level 0).
pub const INFO: u16 = crate::core_api::LOG_INFO;

/// No module code is running.
pub const PHASE_NONE: u8 = 0;
/// The module's `init`.
pub const PHASE_INIT: u8 = 1;
/// The module's `tick`.
pub const PHASE_TICK: u8 = 2;
/// The module's `fini`.
pub const PHASE_FINI: u8 = 3;
/// A callback of the module: a host command handler, an I/O or job step, an intro frame, a menu action.
pub const PHASE_CALLBACK: u8 = 4;
