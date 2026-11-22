//! `TransportFns`: the reply a command handler builds.
use crate::transport::{self, RBODY};
use dkrt_abi::resident::TransportFns;

unsafe extern "C" fn reply(cmd: u32, seq: u32, status: i32, extra: usize) -> i32 {
    transport::reply(cmd as u8, seq, status, extra)
}
unsafe extern "C" fn reply_body() -> *mut u8 {
    core::ptr::addr_of_mut!(RBODY) as *mut u8
}

pub(super) static FNS: TransportFns = TransportFns { reply, reply_body };
