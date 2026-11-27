//! `Refusal`: why a package does not accept a resident table.

/// The kinds of refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// No table.
    NoTable = 0,
    /// The first word is not `RESIDENT_MAGIC`.
    Magic = 1,
    /// Another major version.
    Major = 2,
    /// A minor version older than the one the package was built against.
    Minor = 3,
    /// The table is shorter than the package's `ResidentTable`: a group pointer is missing.
    TableSize = 4,
    /// A group pointer is null.
    GroupMissing = 5,
    /// A group is smaller than the layout the package uses.
    GroupSize = 6,
}

/// The log text of each `Reason`, by its number (a table, not a `match`: a jump table would put data in .text). At most 23 bytes and a NUL.
const MESSAGES: [&[u8]; 7] = [
    b"resident: no table\0",
    b"resident: magic\0",
    b"resident: major\0",
    b"resident: minor\0",
    b"resident: table size\0",
    b"resident: no group\0",
    b"resident: group size\0",
];

/// What `check` found wrong: the kind, the value found and the value the package needs (0 when it has none), and the group for the two group kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub reason: Reason,
    pub found: u32,
    pub need: u32,
    /// The name of the group (`""` for the table itself).
    pub group: &'static str,
}

impl Refusal {
    pub(super) const fn new(reason: Reason, found: u32, need: u32) -> Refusal {
        Refusal {
            reason,
            found,
            need,
            group: "",
        }
    }

    pub(super) const fn of_group(
        reason: Reason,
        group: &'static str,
        found: u32,
        need: u32,
    ) -> Refusal {
        Refusal {
            reason,
            found,
            need,
            group,
        }
    }

    /// The text of the boot log event.
    pub const fn message(&self) -> &'static [u8] {
        MESSAGES[self.reason as usize]
    }
}
