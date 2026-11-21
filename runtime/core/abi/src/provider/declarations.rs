use super::Status;
use dkrt_sdk::E_DECL_FULL;

/// Most resources one service declares.
pub const MAX_RESOURCES: usize = 8;
/// Most entries one package declares and installs: the one bound, used by the declarations and by the installers in `dkrt-lifecycle`.
pub const MAX_ENTRIES: usize = 16;

/// The limit of a declared resource: a number in the header, or "init", which the service decides in `start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    Fixed(u32),
    Init,
}

/// What `Provider::declare` reports: resources (name hash and limit) and entries (name hash). The core compares them with the packed header. Bounded;
/// a declaration past the bound is refused with `E_DECL_FULL`.
#[derive(Clone, Debug)]
pub struct Declarations {
    resources: [(u32, Limit); MAX_RESOURCES],
    nresources: usize,
    entries: [u32; MAX_ENTRIES],
    nentries: usize,
}

impl Declarations {
    pub const fn new() -> Declarations {
        Declarations {
            resources: [(0, Limit::Init); MAX_RESOURCES],
            nresources: 0,
            entries: [0; MAX_ENTRIES],
            nentries: 0,
        }
    }

    pub fn resource(&mut self, name: u32, limit: Limit) -> Result<(), Status> {
        let slot = self.resources.get_mut(self.nresources).ok_or(E_DECL_FULL)?;
        *slot = (name, limit);
        self.nresources += 1;
        Ok(())
    }

    pub fn entry(&mut self, name: u32) -> Result<(), Status> {
        let slot = self.entries.get_mut(self.nentries).ok_or(E_DECL_FULL)?;
        *slot = name;
        self.nentries += 1;
        Ok(())
    }

    pub fn resources(&self) -> &[(u32, Limit)] {
        &self.resources[..self.nresources]
    }

    pub fn entries(&self) -> &[u32] {
        &self.entries[..self.nentries]
    }
}

impl Default for Declarations {
    fn default() -> Self {
        Declarations::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declarations_keep_order_and_refuse_past_the_bound() {
        let mut d = Declarations::new();
        d.resource(1, Limit::Fixed(4)).unwrap();
        d.resource(2, Limit::Init).unwrap();
        assert_eq!(d.resources(), [(1, Limit::Fixed(4)), (2, Limit::Init)]);
        for i in 0..MAX_ENTRIES as u32 {
            d.entry(i).unwrap();
        }
        assert_eq!(d.entry(99), Err(E_DECL_FULL));
        assert_eq!(d.entries().len(), MAX_ENTRIES);
    }
}
