use super::page::NativePage;
use super::table::{target, PAGES, TYPES};
use super::view::View;

/// The most live pages a table can hold: the replacement pair of each reserved machine type.
const MAX: usize = TYPES * PAGES as usize;

/// One live page with its place.
#[derive(Clone)]
struct Candidate {
    ty: u32,
    index: u32,
    page: NativePage,
}

/// The live pages of a [`PageTable`](super::PageTable), cloned: what the first phase of one owner entry (`Owner::step`) takes out, so that the second phase reads the firmware's
/// view with the owner idle and picks the page it shows, with no second entry. A hook runs on every draw and turn of the machine page, and which page it is depends on firmware
/// reads that must not run inside the owner.
pub struct Candidates {
    len: usize,
    items: [Candidate; MAX],
}

impl Candidates {
    pub(super) fn empty() -> Self {
        const E: Candidate = Candidate {
            ty: 0,
            index: 0,
            page: NativePage::empty(),
        };
        Candidates {
            len: 0,
            items: [E; MAX],
        }
    }

    pub(super) fn push(&mut self, ty: u32, index: u32, page: &NativePage) {
        self.items[self.len] = Candidate {
            ty,
            index,
            page: page.clone(),
        };
        self.len += 1;
    }

    /// The page the view shows, if it is a native one. With no live page the view is not read.
    pub fn selected(&self, view: &impl View) -> Option<&NativePage> {
        let (index, ty) = target(self.len != 0, view)?;
        let ty = u32::try_from(ty).ok()?;
        self.items[..self.len]
            .iter()
            .find(|c| c.ty == ty && c.index == index)
            .map(|c| &c.page)
    }

    /// The parameter code of encoder `encoder` on the page the view shows: -1 resumes the stock mapper.
    pub fn code(&self, view: &impl View, encoder: u32) -> i32 {
        self.selected(view)
            .map_or(-1, |page| page.encoder_code(encoder))
    }
}
