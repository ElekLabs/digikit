use dkrt_abi::link::{LinkResult, Owner, Stream, Ticket};

/// The link queue of the core as the host sees it (`CoreApi::link_submit` and `link_result`): the one seam under `Transfer`. The package's queue is the real one, a fake serves the tests.
pub trait LinkQueue {
    /// Queue `stream` for `owner`; `None` when the queue does not take it now (ask again later).
    fn submit(&mut self, owner: Owner, stream: &Stream) -> Option<Ticket>;
    /// The result of a request of its owner; a terminal result is given once.
    fn result(&mut self, ticket: Ticket) -> LinkResult;
}
