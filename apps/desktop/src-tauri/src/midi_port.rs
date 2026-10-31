//! A real MIDI port for the digikit SysEx protocol (the desktop app's second target next to the emulator). The page speaks the protocol itself
//! in TypeScript (`packages/dk-session/src/dkupload-core.ts`); this file only moves whole SysEx messages: it lists the ports, opens one by name
//! (input and output of the same name), sends a message and waits for the next one. No process is started. Nothing here knows a device: the
//! port is whatever the user picks.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
    mpsc::{Receiver, RecvTimeoutError, Sender, channel},
};
use std::time::{Duration, Instant};

use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use tauri::State;

/// A device's SysEx can reach the callback in pieces (CoreMIDI splits a long message); this puts the pieces together. Bytes outside a SysEx
/// message (clock, active sensing, notes) are dropped. A message that starts inside another one drops the unfinished one.
#[derive(Default)]
pub struct Sysex {
    open: Vec<u8>,
}

impl Sysex {
    /// Feed the bytes of one callback; returns the messages (`F0 ... F7`) it completed.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut done = Vec::new();
        for &b in bytes {
            if b == 0xF0 {
                self.open.clear();
                self.open.push(b);
            } else if self.open.is_empty() {
                continue;
            } else if b == 0xF7 {
                self.open.push(b);
                done.push(std::mem::take(&mut self.open));
            } else if b < 0x80 {
                self.open.push(b);
            } else if b < 0xF8 {
                // a status byte inside a SysEx ends it without F7: drop it
                self.open.clear();
            }
            // real-time bytes (0xF8 and up) may sit anywhere: ignored
        }
        done
    }
}

/// The names that are both an input and an output port, in the input's order.
pub fn common_ports(inputs: &[String], outputs: &[String]) -> Vec<String> {
    inputs
        .iter()
        .filter(|n| outputs.contains(n))
        .cloned()
        .collect()
}

struct Open {
    out: MidiOutputConnection,
    /// Keeps the input connection alive; dropping it closes the port.
    _input: MidiInputConnection<()>,
    rx: Arc<Mutex<Receiver<Vec<u8>>>>,
}

/// The open port, and the cancel counter of the pending `midi_recv`: `midi_cancel` raises it, a waiting recv that sees it change ends.
/// Both are behind an `Arc` so a command can hand them to a worker thread (no command does MIDI I/O on the thread that called it).
#[derive(Default)]
pub struct MidiPort {
    open: Arc<Mutex<Option<Open>>>,
    cancel: Arc<AtomicU64>,
}

/// One MIDI client that lives as long as the app and is created on the main thread (`Builder::setup`).
///
/// CoreMIDI tells a process about ports that came or went on the run loop of the thread that created its first client (midir's
/// `MidiInput::new` calls `MIDIClientCreate`; `ports()` only reads CoreMIDI's per-process list). The commands below create their
/// clients on worker threads that never run a run loop, so a first client made there froze the list at its start state: a device plugged in
/// later was not listed until the app restarted. The main thread runs the window's run loop, so a client made there keeps the list current.
pub struct MidiWatch(#[allow(dead_code)] Mutex<Option<MidiInput>>);

impl MidiWatch {
    /// Call on the main thread, before any command can create a client. A failure leaves no client (listing still works as before).
    pub fn start() -> Self {
        Self(Mutex::new(MidiInput::new("digikit watch").ok()))
    }
}

/// How a wait for the next message ended.
#[derive(Debug, PartialEq)]
pub enum Recv {
    Message(Vec<u8>),
    Timeout,
    Cancelled,
    Closed,
}

/// How often a waiting recv looks at the cancel counter.
const CANCEL_SLICE: Duration = Duration::from_millis(40);

/// Wait up to `timeout` for the next message, in short slices so a change of `cancel` (from the value `seen`) ends the wait at once.
/// Blocks the calling thread: run it on a worker.
pub fn recv_message(rx: &Receiver<Vec<u8>>, timeout: Duration, cancel: &AtomicU64, seen: u64) -> Recv {
    let end = Instant::now() + timeout;
    loop {
        if cancel.load(Ordering::Acquire) != seen {
            return Recv::Cancelled;
        }
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left.min(CANCEL_SLICE)) {
            Ok(m) => return Recv::Message(m),
            Err(RecvTimeoutError::Disconnected) => return Recv::Closed,
            Err(RecvTimeoutError::Timeout) if left <= CANCEL_SLICE => return Recv::Timeout,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn names_in(input: &MidiInput) -> Vec<String> {
    input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect()
}

fn names_out(output: &MidiOutput) -> Vec<String> {
    output
        .ports()
        .iter()
        .filter_map(|p| output.port_name(p).ok())
        .collect()
}

/// The MIDI ports a device could be on: names that exist as input and as output.
#[tauri::command]
pub async fn midi_ports() -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let input = MidiInput::new("digikit").map_err(|e| e.to_string())?;
        let output = MidiOutput::new("digikit").map_err(|e| e.to_string())?;
        Ok(common_ports(&names_in(&input), &names_out(&output)))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Open the port called `name` (closing the open one first).
#[tauri::command]
pub async fn midi_open(state: State<'_, MidiPort>, name: String) -> Result<(), String> {
    let slot = state.open.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let mut input = MidiInput::new("digikit").map_err(|e| e.to_string())?;
        input.ignore(Ignore::None); // SysEx is what this is for
        let output = MidiOutput::new("digikit").map_err(|e| e.to_string())?;
        let in_port = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).is_ok_and(|n| n == name))
            .ok_or_else(|| format!("no MIDI input port {name:?}"))?;
        let out_port = output
            .ports()
            .into_iter()
            .find(|p| output.port_name(p).is_ok_and(|n| n == name))
            .ok_or_else(|| format!("no MIDI output port {name:?}"))?;
        let (tx, rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = channel();
        let mut assembler = Sysex::default();
        let conn = input
            .connect(
                &in_port,
                "digikit in",
                move |_, bytes, _| {
                    for m in assembler.feed(bytes) {
                        let _ = tx.send(m);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        let out = output
            .connect(&out_port, "digikit out")
            .map_err(|e| e.to_string())?;
        let opened = Open {
            out,
            _input: conn,
            rx: Arc::new(Mutex::new(rx)),
        };
        // the old port closes here, on the worker (closing a CoreMIDI connection can take a moment)
        *slot.lock().map_err(|e| e.to_string())? = Some(opened);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Close the open port (on a worker: dropping a connection can block).
#[tauri::command]
pub async fn midi_close(state: State<'_, MidiPort>) -> Result<(), String> {
    state.cancel.fetch_add(1, Ordering::AcqRel);
    let slot = state.open.clone();
    tauri::async_runtime::spawn_blocking(move || {
        *slot.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// End the pending `midi_recv` now (it answers as if the time had run out). Nothing pending: no effect.
#[tauri::command]
pub async fn midi_cancel(state: State<'_, MidiPort>) -> Result<(), String> {
    state.cancel.fetch_add(1, Ordering::AcqRel);
    Ok(())
}

/// Send one whole message (the raw body of the request).
#[tauri::command]
pub async fn midi_send(
    state: State<'_, MidiPort>,
    request: tauri::ipc::Request<'_>,
) -> Result<(), String> {
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.clone(),
        _ => return Err("a MIDI message must use a raw byte body".into()),
    };
    let slot = state.open.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = slot.lock().map_err(|e| e.to_string())?;
        let open = guard.as_mut().ok_or("no MIDI port is open")?;
        open.out.send(&bytes).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The next complete SysEx message, or an empty reply after `timeout_ms` or when `midi_cancel` was called.
#[tauri::command]
pub async fn midi_recv(
    state: State<'_, MidiPort>,
    timeout_ms: u64,
) -> Result<tauri::ipc::Response, String> {
    let rx = {
        let guard = state.open.lock().map_err(|e| e.to_string())?;
        guard.as_ref().ok_or("no MIDI port is open")?.rx.clone()
    };
    let cancel = state.cancel.clone();
    let seen = cancel.load(Ordering::Acquire);
    tauri::async_runtime::spawn_blocking(move || {
        let rx = rx.lock().map_err(|e| e.to_string())?;
        match recv_message(&rx, Duration::from_millis(timeout_ms), &cancel, seen) {
            Recv::Message(m) => Ok(tauri::ipc::Response::new(m)),
            Recv::Timeout | Recv::Cancelled => Ok(tauri::ipc::Response::new(Vec::new())),
            Recv::Closed => Err("the MIDI port closed".to_string()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_in_pieces_is_put_together_and_realtime_bytes_are_ignored() {
        let mut s = Sysex::default();
        assert!(s.feed(&[0xFE, 0xF8]).is_empty());
        assert!(s.feed(&[0xF0, 0x7D, 0x64]).is_empty());
        assert!(s.feed(&[0xF8, 0x6B, 0x01]).is_empty());
        assert_eq!(
            s.feed(&[0x02, 0xF7, 0xFE]),
            vec![vec![0xF0, 0x7D, 0x64, 0x6B, 0x01, 0x02, 0xF7]]
        );
        // two messages in one callback
        assert_eq!(
            s.feed(&[0xF0, 1, 0xF7, 0xF0, 2, 0xF7]),
            vec![vec![0xF0, 1, 0xF7], vec![0xF0, 2, 0xF7]]
        );
    }

    #[test]
    fn an_unfinished_message_is_dropped_when_another_starts_or_a_status_byte_comes() {
        let mut s = Sysex::default();
        assert_eq!(
            s.feed(&[0xF0, 1, 2, 0xF0, 3, 0xF7]),
            vec![vec![0xF0, 3, 0xF7]]
        );
        assert!(s.feed(&[0xF0, 1, 0x90, 2, 0xF7]).is_empty());
        assert_eq!(s.feed(&[0xF0, 9, 0xF7]), vec![vec![0xF0, 9, 0xF7]]);
    }

    /// A recv with no device behind it ends at its timeout, on a worker thread, and the thread that waits for it is free meanwhile.
    #[test]
    fn a_recv_with_no_device_returns_at_the_timeout_on_a_worker() {
        let (_tx, rx) = channel::<Vec<u8>>(); // the sender stays alive and silent: nothing answers
        let cancel = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let worker = {
            let cancel = cancel.clone();
            std::thread::spawn(move || recv_message(&rx, Duration::from_millis(300), &cancel, 0))
        };
        // the caller is not blocked: it can do other work while the worker waits
        assert!(!worker.is_finished());
        assert_eq!(worker.join().unwrap(), Recv::Timeout);
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(290) && took < Duration::from_millis(1500), "{took:?}");
    }

    #[test]
    fn a_cancel_ends_a_long_wait_at_once_and_a_message_still_arrives() {
        let (tx, rx) = channel::<Vec<u8>>();
        let cancel = Arc::new(AtomicU64::new(0));
        let worker = {
            let cancel = cancel.clone();
            std::thread::spawn(move || {
                let started = Instant::now();
                (recv_message(&rx, Duration::from_secs(30), &cancel, 0), started.elapsed(), rx)
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        cancel.fetch_add(1, Ordering::AcqRel);
        let (got, took, rx) = worker.join().unwrap();
        assert_eq!(got, Recv::Cancelled);
        assert!(took < Duration::from_secs(2), "{took:?}");
        // a later wait (it sees the new counter) is not cancelled by the old cancel
        tx.send(vec![0xF0, 1, 0xF7]).unwrap();
        assert_eq!(recv_message(&rx, Duration::from_secs(1), &cancel, 1), Recv::Message(vec![0xF0, 1, 0xF7]));
        drop(tx);
        assert_eq!(recv_message(&rx, Duration::from_secs(1), &cancel, 1), Recv::Closed);
    }

    #[test]
    fn only_names_that_are_both_input_and_output_are_offered() {
        let i = ["A".to_string(), "B".to_string(), "C".to_string()];
        let o = ["C".to_string(), "A".to_string()];
        assert_eq!(common_ports(&i, &o), vec!["A".to_string(), "C".to_string()]);
    }
}
