/// The answer of a call that may take more than one UI pass: done, or call again on a later pass (the callee keeps where it stands). `Payloads::install`, `Payloads::retire`,
/// `ProviderHost::attach` and `ProviderHost::detach` answer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Done,
    Pending,
}

impl Step {
    /// The C shape of an answer that is a status or a step: `Done` is 0 (`OK`), `Pending` is [`Step::PENDING`]; a refusal is a negative load code.
    pub const PENDING: i32 = 1;

    /// The C shape of a step.
    pub const fn code(self) -> i32 {
        match self {
            Step::Done => dkrt_sdk::OK,
            Step::Pending => Step::PENDING,
        }
    }

    /// The step or the refusal in a C answer: 0 is `Done`, [`Step::PENDING`] is `Pending`, anything else is the status of a refusal.
    pub const fn of_code(code: i32) -> Result<Step, i32> {
        if code == dkrt_sdk::OK {
            Ok(Step::Done)
        } else if code == Step::PENDING {
            Ok(Step::Pending)
        } else {
            Err(if code > 0 { dkrt_sdk::E_FORMAT } else { code })
        }
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Step::Pending)
    }
}

/// The C shape of the answer of a call that gives a [`Step`] or refuses.
pub(crate) fn step_code(r: Result<Step, i32>) -> i32 {
    match r {
        Ok(step) => step.code(),
        Err(status) => status,
    }
}
