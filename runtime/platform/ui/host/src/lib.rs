//! Host fake of `<dk/platform/ui.h>` and the ui calls of `<dk/gen/imports.h>`. The `dk_ui_*` functions below keep a model of menus, the view stack,
//! the open dialog and the toasts. `driver` acts like the user on that model.
//! The model is per thread; `session()` serialises tests, because the C code under test has statics.
pub mod driver;

mod caps;
mod dialog;
mod limits;
mod menu;
mod menu_desc;
mod model;
mod tick;
mod toast;
mod ui_token;

pub use caps::*;
pub use dialog::*;
pub use limits::*;
pub use menu::*;
pub use menu_desc::*;
pub use model::*;
pub use tick::*;
pub use toast::*;
pub use ui_token::*;
