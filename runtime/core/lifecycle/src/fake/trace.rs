use crate::{PackageId, Phase, Step, Trace};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::vec::Vec;

/// One event a [`RecordingTrace`] saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub phase: Phase,
    pub step: Step,
    pub package: PackageId,
    pub status: i32,
    /// The name the trace resolved: a fresh number for each resolve, so a step whose end resolved again shows.
    pub name: u32,
}

#[derive(Default)]
struct Inner {
    off: Cell<bool>,
    resolved: Cell<u32>,
    events: RefCell<Vec<Event>>,
}

/// A [`Trace`] that keeps its events. Clones share them. It is on until [`RecordingTrace::switch_off`].
#[derive(Clone, Default)]
pub struct RecordingTrace(Rc<Inner>);

impl RecordingTrace {
    pub fn events(&self) -> Vec<Event> {
        self.0.events.borrow().clone()
    }

    pub fn clear(&self) {
        self.0.events.borrow_mut().clear();
    }

    pub fn switch_off(&self) {
        self.0.off.set(true);
    }
}

impl Trace for RecordingTrace {
    fn on(&self) -> bool {
        !self.0.off.get()
    }

    type Name = u32;

    fn name_of(&self, _package: PackageId) -> u32 {
        self.0.resolved.set(self.0.resolved.get() + 1);
        self.0.resolved.get()
    }

    fn event(&self, phase: Phase, step: Step, package: PackageId, name: u32, status: i32) {
        self.0.events.borrow_mut().push(Event {
            phase,
            step,
            package,
            status,
            name,
        });
    }
}
