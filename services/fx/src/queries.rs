//! The `Query` part of the owner `Fx`: the resident query facet that the FX ROUTES menu asks for the routes it may offer (operation 1, `fxsend::ROUTE_CHOICES_V1`).
use crate::fx::Fx;
use crate::fxsend::{self, RouteChoicesV1};
use core::mem::size_of;
use dkrt_abi::provider::{Query, Status};
use dkrt_sdk::E_CONTRACT;

impl Query for Fx {
    /// The reply is the allowed routes (`DkrtFx.allowed` bits) in one word, written only on success. An unknown operation is `E_NOT_FOUND`, a request or a reply of another
    /// size `E_CONTRACT`. Success answers 0, as the facet's callers (`dk_fx_route_choices` in `sdk/include/dk/fx/route-query.h`) check for it.
    fn query(&self, op: u32, req: &[u8], reply: &mut [u8]) -> Result<usize, Status> {
        if op != fxsend::ROUTE_CHOICES_V1 {
            return Err(dkrt_sdk::E_NOT_FOUND);
        }
        if req.len() != size_of::<RouteChoicesV1>() || reply.len() != size_of::<u32>() {
            return Err(E_CONTRACT);
        }
        // SAFETY: `req` has the size of the request; it is read as bytes, with no alignment
        let request = unsafe { core::ptr::read_unaligned(req.as_ptr().cast::<RouteChoicesV1>()) };
        let allowed = fxsend::route_choices(request)?;
        reply.copy_from_slice(&allowed.to_ne_bytes());
        Ok(0)
    }
}
