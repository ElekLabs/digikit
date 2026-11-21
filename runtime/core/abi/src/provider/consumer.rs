/// Which consumer a call is about. Opaque to the service: it only compares ids and gives them back (`detach`, `release`, `describe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConsumerId(u8);

impl ConsumerId {
    pub const fn new(id: u8) -> ConsumerId {
        ConsumerId(id)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The records of one consumer for one service: `(kind local to the service, body)` in file order. The packed format comes in RC3; this is the
/// borrowed view the service sees.
pub type Records<'a> = &'a [(u16, &'a [u8])];

/// A consumer that came up: its id and its records for the service being attached.
pub struct Consumer<'a> {
    id: ConsumerId,
    records: Records<'a>,
}

impl<'a> Consumer<'a> {
    pub fn new(id: ConsumerId, records: Records<'a>) -> Consumer<'a> {
        Consumer { id, records }
    }

    pub fn id(&self) -> ConsumerId {
        self.id
    }

    /// The bodies of the records of `kind`, in file order.
    pub fn records(&self, kind: u16) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.records
            .iter()
            .filter(move |r| r.0 == kind)
            .map(|r| r.1)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    #[test]
    fn records_of_a_kind_in_file_order() {
        let recs: &[(u16, &[u8])] = &[(1, b"a"), (2, b"x"), (1, b"b")];
        let c = Consumer::new(ConsumerId::new(3), recs);
        assert_eq!(c.id().get(), 3);
        let got: Vec<&[u8]> = c.records(1).collect();
        assert_eq!(got, [&b"a"[..], &b"b"[..]]);
        assert_eq!(c.records(9).count(), 0);
    }
}
