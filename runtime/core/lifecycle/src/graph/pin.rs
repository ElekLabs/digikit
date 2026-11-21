use super::PackageGraph;
use crate::PackageId;

impl PackageGraph {
    /// Pins or unpins `id`: a pinned service stays up when its last consumer goes down. False when `id` is not in the graph.
    pub fn set_pinned(&mut self, id: PackageId, pinned: bool) -> bool {
        match self.nodes[..self.len].iter_mut().find(|n| n.id() == id) {
            Some(n) => {
                n.set_pinned(pinned);
                true
            }
            None => false,
        }
    }
}
