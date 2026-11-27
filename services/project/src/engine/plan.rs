//! What a scanned slot needs. The planners are pure: they read the scan's result and a copy of the bound project's facts, push file operations into a [`TaskRun`] and say what the
//! bound project must do ([`Effect`]); they reach neither the store nor the log. UI task only.
use super::bound::{BoundView, Effect, SourceFile};
use super::scan::{ScanFor, ScanResult, SlotFile, SLOT_FILES_MAX};
use super::task::{SaveEnd, Task, TaskRun};
use super::{bytes_equal, SlotRecord};
use crate::dkp::Ignore;
use crate::name::{self, Path};

/// A plan: the tasks are in the run; `effect` is what the bound project does before they start (`None`: nothing).
pub struct Plan {
    pub effect: Option<Effect>,
}

/// The plan for what `result` found, by what the scan was for. `None`: there is nothing to run (the firmware has no record of the slot). `run` must be `begin`ned.
/// (If-let chain, not a `match`: see `Task::prepare`.)
pub fn plan(result: &ScanResult, bound: &BoundView, run: &mut TaskRun) -> Option<Plan> {
    let scan_for = result.scan_for();
    if scan_for == ScanFor::Load {
        plan_load(result, run)
    } else if scan_for == ScanFor::Clear {
        plan_clear(result, bound, run)
    } else if scan_for == ScanFor::Save {
        plan_save(result, bound, run)
    } else {
        plan_paste(result, run)
    }
}

/// The best usable file of the slot: passes the checks and has the slot's stamp, the highest write counter wins.
pub(super) fn pick_winner(files: &[SlotFile], record: &Option<SlotRecord>) -> Option<usize> {
    let mut best: Option<usize> = None;
    let mut at = 0;
    while at < files.len() {
        if let Ok(head) = files[at].head {
            let usable = match record {
                Some(record) => head.stamp == record.stamp,
                None => false,
            };
            if usable
                && best
                    .map(|b| {
                        files[b]
                            .head
                            .map(|best_head| head.counter > best_head.counter)
                            .unwrap_or(true)
                    })
                    .unwrap_or(true)
            {
                best = Some(at);
            }
        }
        at += 1;
    }
    best
}

/// A record for a slot the firmware does not list.
const NO_RECORD: SlotRecord = SlotRecord {
    name: [0; 16],
    stamp: 0,
};

/// A slot became current: the winner's file becomes the image, the other usable files are superseded copies. Without a usable file the slot is bound empty (and a refused file is
/// reported).
fn plan_load(result: &ScanResult, run: &mut TaskRun) -> Option<Plan> {
    let slot = result.slot();
    let files = result.files();
    let record = result.record();
    if let (Some(win), Some(record)) = (pick_winner(files, &record), record) {
        let Ok(head) = files[win].head else {
            return None;
        };
        // the other usable files of the slot are superseded copies; a stock rename moves the file to the new name
        let mut at = 0;
        while at < files.len() {
            if at != win
                && files[at]
                    .head
                    .map(|other| other.stamp == record.stamp)
                    .unwrap_or(false)
            {
                run.push(Task::Delete {
                    path: files[at].path,
                });
            }
            at += 1;
        }
        run.set_data_after(slot, true);
        let to = name::file_path(slot, &record.name);
        if !bytes_equal(to.as_bytes(), files[win].path.as_bytes()) {
            let mut superseded = [Path::EMPTY; 3];
            superseded[0] = files[win].path;
            run.push(Task::WriteLive {
                to,
                slot: slot as u8,
                record,
                counter: head.counter.wrapping_add(1),
                superseded,
                superseded_count: 1,
            });
        }
        return Some(Plan {
            effect: Some(Effect::BindFile {
                slot,
                record,
                file: SourceFile {
                    head,
                    path: files[win].path,
                },
            }),
        });
    }
    // no usable file: stock project without digikit data, or a file that is not this project's (it stays in the store, but the loader does not use it)
    run.set_data_after(slot, false);
    let ignored = if files.is_empty() {
        None
    } else {
        Some(match files[0].head {
            Err(why) => why,
            Ok(_) => Ignore::Stamp,
        })
    };
    Some(Plan {
        effect: Some(Effect::BindEmpty {
            slot,
            record: record.unwrap_or(NO_RECORD),
            ignored,
        }),
    })
}

/// A cleared slot: every usable file of it goes to the archive (the clear keeps the stamp, so the old data would come back otherwise); a cleared current project is empty.
fn plan_clear(result: &ScanResult, bound: &BoundView, run: &mut TaskRun) -> Option<Plan> {
    let slot = result.slot();
    archive_usable(result.files(), run);
    run.set_data_after(slot, false);
    let effect = if bound.slot.as_i32() == slot as i32 {
        Some(Effect::EmptyCurrent {
            slot,
            record: result.record().unwrap_or(NO_RECORD),
        })
    } else {
        None
    };
    Some(Plan { effect })
}

/// A slot pasted over: its old files with another stamp go to the archive, then the source's file is copied to it (paste copies the stamp and the name of the project, so the
/// source is the usable file of another slot with the same name part and stamp; the scan chose it).
fn plan_paste(result: &ScanResult, run: &mut TaskRun) -> Option<Plan> {
    let record = result.record()?;
    archive_usable(result.files(), run);
    run.set_data_after(result.slot(), result.paste_source().is_some());
    if let Some(from) = result.paste_source() {
        run.push(Task::Copy {
            from,
            to: name::file_path(result.slot(), &record.name),
            slot: result.slot() as u8,
            record,
        });
    }
    Some(Plan { effect: None })
}

/// The files of the slot against the image: archive what no longer belongs to the slot, write the image when it or its header changed, remove superseded copies.
fn plan_save(result: &ScanResult, bound: &BoundView, run: &mut TaskRun) -> Option<Plan> {
    let slot = result.slot();
    let files = result.files();
    let record = result.record()?;
    let bound = bound.adopted_for_save(slot);
    let target = name::file_path(slot, &record.name);
    let mut max_counter = bound.file.map(|file| file.head.counter).unwrap_or(0);
    let mut extra = [Path::EMPTY; SLOT_FILES_MAX];
    let mut extra_count = 0;
    let mut at_target = false;
    let mut at = 0;
    while at < files.len() {
        if let Ok(head) = files[at].head {
            // the file the bound project came from or was last written to is a superseded copy of the same project, whatever its stamp (a stock rename of the current project
            // gives it a new stamp); every other file with another stamp is a slot reused
            let own = bound
                .file
                .map(|file| bytes_equal(file.path.as_bytes(), files[at].path.as_bytes()))
                .unwrap_or(false);
            if bound.has_data && (head.stamp == record.stamp || own) {
                if head.counter > max_counter {
                    max_counter = head.counter;
                }
                if bytes_equal(files[at].path.as_bytes(), target.as_bytes()) {
                    at_target = true;
                } else {
                    extra[extra_count] = files[at].path;
                    extra_count += 1;
                }
            } else {
                // another stamp (the slot was cleared, deleted, formatted or overwritten and a new project saved), or no data left: the old file leaves the slot
                run.push(Task::Archive {
                    from: files[at].path,
                    to: None,
                });
            }
        }
        at += 1;
    }
    let mut bytes = 0;
    let mut counter = 0;
    if bound.has_data {
        bytes = bound.used;
        let header_same = bound.record_matches_file(slot, &record);
        if bound.dirty || !at_target || !header_same {
            counter = max_counter.wrapping_add(1);
            let mut superseded = [Path::EMPTY; 3];
            let mut superseded_count = 0;
            while superseded_count < extra_count.min(3) {
                superseded[superseded_count] = extra[superseded_count];
                superseded_count += 1;
            }
            run.push(Task::WriteLive {
                to: target,
                slot: slot as u8,
                record,
                counter,
                superseded,
                superseded_count: superseded_count as u8,
            });
        } else {
            counter = max_counter;
            let mut at_extra = 0;
            while at_extra < extra_count {
                run.push(Task::Delete {
                    path: extra[at_extra],
                });
                at_extra += 1;
            }
        }
    }
    run.set_data_after(slot, bound.has_data);
    run.set_save_end(SaveEnd {
        slot,
        bytes,
        counter,
    });
    Some(Plan {
        effect: Some(Effect::AdoptForSave { slot }),
    })
}

/// Every usable file of the slot goes to the archive.
fn archive_usable(files: &[SlotFile], run: &mut TaskRun) {
    let mut at = 0;
    while at < files.len() {
        if files[at].head.is_ok() {
            run.push(Task::Archive {
                from: files[at].path,
                to: None,
            });
        }
        at += 1;
    }
}
