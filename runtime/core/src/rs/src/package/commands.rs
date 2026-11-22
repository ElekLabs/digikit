//! The system package's upload commands: the module list, the capacity, the boot loads and the live load, unload and reload.

use crate::package::module_admin::{h_capacity, h_load, h_reload, h_reload_state, h_unload};
use crate::module_cmds::{h_loads, h_modules};
use crate::transport::{Cmd, NOLOG};
use dkrt_sdk::proto::*;

/// The package's commands, looked up one after the other like the resident ones.
pub(crate) static CMDS: [Cmd; 7] = [
    Cmd { cmd: CMD_MODULES, flags: NOLOG, h: h_modules },
    Cmd { cmd: CMD_CAPACITY, flags: NOLOG, h: h_capacity },
    Cmd { cmd: CMD_LOADS, flags: NOLOG, h: h_loads },
    Cmd { cmd: CMD_RELOAD, flags: 0, h: h_reload },
    Cmd { cmd: CMD_RELOAD_STATE, flags: NOLOG, h: h_reload_state },
    Cmd { cmd: CMD_LOAD, flags: 0, h: h_load },
    Cmd { cmd: CMD_UNLOAD, flags: 0, h: h_unload },
];

pub(crate) fn find(cmd: u8) -> Option<&'static Cmd> {
    let mut i = 0;
    while i < CMDS.len() {
        if CMDS[i].cmd == cmd {
            return Some(&CMDS[i]);
        }
        i += 1;
    }
    None
}
