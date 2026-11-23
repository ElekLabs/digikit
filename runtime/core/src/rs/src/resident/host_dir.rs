//! `HostDirFns`: the host directory.
use crate::host_dir;
use dkrt_abi::resident::{Group, HostDirFns};

unsafe extern "C" fn publish(id: u32, addr: u32) -> bool {
    host_dir::publish_package(id, addr)
}

pub(super) static FNS: Group<HostDirFns> = Group::new(HostDirFns { publish });
