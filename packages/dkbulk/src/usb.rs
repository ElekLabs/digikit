//! The real link: the vendor interface of the device (class 0xFF) on USB bulk endpoints, through nusb.
//!
//! Two units of the same model have the same USB serial strings, so a device is named by where it is on the bus (`bus:address`, `Candidate::id`), and
//! the caller proves which candidate is the one on its MIDI port with the nonce handshake (docs/plans/usb-fast.md section 5).
use std::time::Duration;

use nusb::transfer::{Bulk, Direction, In, Out, TransferError};
use nusb::{DeviceInfo, Endpoint, Interface, MaybeFuture};

use crate::gen_bulk::{CLASS, PACKET, VID};
use crate::link::{FrameReader, InPipe, Link, LinkError, OutPipe, send_with_zlp};

/// Size of one IN transfer: replies are small; a multiple of every bulk packet size (64, 512).
const IN_SIZE: usize = 4096;

/// A device that offers the bulk interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// `bus:address`: valid while the device stays plugged in.
    pub id: String,
    /// The product string, for the log.
    pub name: String,
    pub interface: u8,
    /// The interface has one bulk OUT endpoint and no IN endpoint (USB AUDIO/MIDI): replies come over SysEx.
    pub out_only: bool,
}

fn id_of(d: &DeviceInfo) -> String {
    format!("{}:{}", d.bus_id(), d.device_address())
}

/// The bulk interface of a device and its shape.
struct Found {
    number: u8,
    out_only: bool,
    out_ep: Option<u8>,
}

/// The class interface of `d`, and whether it is a bulk pair or OUT only. A device that cannot be opened is taken for a pair.
fn classify(d: &DeviceInfo) -> Option<Found> {
    let number = offers(d)?;
    let pair = Found {
        number,
        out_only: false,
        out_ep: None,
    };
    let Ok(device) = d.open().wait() else {
        return Some(pair);
    };
    let (mut out_ep, mut has_in) = (None, false);
    for alt in device
        .configurations()
        .flat_map(|c| c.interface_alt_settings())
        .filter(|a| a.interface_number() == number && a.class() == CLASS)
    {
        for ep in alt.endpoints() {
            if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                continue;
            }
            match ep.direction() {
                Direction::Out => out_ep = out_ep.or(Some(ep.address())),
                Direction::In => has_in = true,
            }
        }
    }
    Some(match (out_ep, has_in) {
        (Some(_), false) => Found {
            number,
            out_only: true,
            out_ep,
        },
        _ => pair,
    })
}

fn offers(d: &DeviceInfo) -> Option<u8> {
    (d.vendor_id() == VID)
        .then(|| {
            d.interfaces()
                .find(|i| i.class() == CLASS)
                .map(|i| i.interface_number())
        })
        .flatten()
}

/// The devices with the vendor id and an interface of the bulk class.
pub fn list() -> Result<Vec<Candidate>, String> {
    let all = nusb::list_devices().wait().map_err(|e| e.to_string())?;
    Ok(all
        .filter_map(|d| {
            let f = classify(&d)?;
            Some(Candidate {
                id: id_of(&d),
                name: d.product_string().unwrap_or("?").to_string(),
                interface: f.number,
                out_only: f.out_only,
            })
        })
        .collect())
}

/// The OUT half.
pub struct UsbTx {
    out: Endpoint<Bulk, Out>,
}
/// The IN half; `recv` returns whole frames; the device sends 64-byte IN packets (each a short packet, no zero-length packets), so it reads until a frame is complete.
pub struct UsbRx {
    inp: Endpoint<Bulk, In>,
    reader: FrameReader,
}
pub struct UsbLink {
    pub tx: UsbTx,
    pub rx: UsbRx,
    interface: Interface,
}

impl UsbLink {
    /// Open the device `id` (`bus:address` of `list`), or the first candidate when `id` is None, claim its bulk interface and take its endpoints.
    pub fn open(id: Option<&str>) -> Result<UsbLink, String> {
        let all = nusb::list_devices().wait().map_err(|e| e.to_string())?;
        let info = all
            .filter(|d| id.is_none_or(|w| id_of(d) == w))
            .find_map(|d| classify(&d).filter(|f| !f.out_only).map(|f| (d, f.number)))
            .ok_or_else(|| match id {
                Some(w) => format!("no USB device {w} with an interface of class {CLASS:#04x}"),
                None => format!("no USB device {VID:#06x} with an interface of class {CLASS:#04x} (is usb-fast loaded?)"),
            })?;
        let (info, number) = info;
        let device = info.open().wait().map_err(|e| format!("open: {e}"))?;
        let interface = device
            .claim_interface(number)
            .wait()
            .map_err(|e| format!("claim interface {number}: {e}"))?;
        let (mut out_addr, mut in_addr) = (None, None);
        for ep in interface
            .descriptor()
            .into_iter()
            .flat_map(|d| d.endpoints())
        {
            if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                continue;
            }
            match ep.direction() {
                Direction::Out => out_addr = out_addr.or(Some(ep.address())),
                Direction::In => in_addr = in_addr.or(Some(ep.address())),
            }
        }
        let (Some(o), Some(i)) = (out_addr, in_addr) else {
            return Err(format!(
                "interface {number} has no bulk OUT and bulk IN endpoint pair"
            ));
        };
        let out = interface
            .endpoint::<Bulk, Out>(o)
            .map_err(|e| format!("endpoint {o:#04x}: {e}"))?;
        let inp = interface
            .endpoint::<Bulk, In>(i)
            .map_err(|e| format!("endpoint {i:#04x}: {e}"))?;
        Ok(UsbLink {
            tx: UsbTx { out },
            rx: UsbRx {
                inp,
                reader: FrameReader::default(),
            },
            interface,
        })
    }

    /// The two halves, for a bridge that sends and receives on different threads; the interface stays claimed until the third value drops.
    pub fn split(self) -> (UsbTx, UsbRx, Box<dyn std::any::Any + Send>) {
        (self.tx, self.rx, Box::new(self.interface))
    }
}

/// A claimed interface with one bulk OUT endpoint and no IN endpoint (the USB AUDIO/MIDI probe: interface 5, EP 0x02). Only that interface is claimed:
/// the class drivers keep the others, and no kernel driver is detached.
pub struct OutOnly {
    out: Endpoint<Bulk, Out>,
    _interface: Interface,
}

impl OutOnly {
    /// Open the device `id` (`bus:address`), or the first device with the vendor id that has an interface `number`; claim it and take the OUT endpoint `ep`.
    pub fn open(id: Option<&str>, number: u8, ep: u8) -> Result<OutOnly, String> {
        let all = nusb::list_devices().wait().map_err(|e| e.to_string())?;
        let info = all
            .filter(|d| d.vendor_id() == VID && d.interfaces().any(|i| i.interface_number() == number))
            .find(|d| id.is_none_or(|w| id_of(d) == w))
            .ok_or_else(|| match id {
                Some(w) => format!("no USB device {w} with an interface {number}"),
                None => format!("no USB device {VID:#06x} with an interface {number} (is the probe image loaded?)"),
            })?;
        Self::claim(&info, number, ep)
    }

    /// Open the OUT-only interface of the device `id` (or the first one that has it): the one `list` marks `out-only`.
    pub fn open_auto(id: Option<&str>) -> Result<OutOnly, String> {
        let all = nusb::list_devices().wait().map_err(|e| e.to_string())?;
        let (info, number, ep) = all
            .filter(|d| id.is_none_or(|w| id_of(d) == w))
            .find_map(|d| {
                let f = classify(&d).filter(|f| f.out_only)?;
                Some((d, f.number, f.out_ep?))
            })
            .ok_or_else(|| match id {
                Some(w) => format!("no USB device {w} with an OUT-only bulk interface"),
                None => format!("no USB device {VID:#06x} with an OUT-only bulk interface (is core/usb-bulk-audio loaded?)"),
            })?;
        Self::claim(&info, number, ep)
    }

    fn claim(info: &DeviceInfo, number: u8, ep: u8) -> Result<OutOnly, String> {
        let device = info.open().wait().map_err(|e| format!("open: {e}"))?;
        let interface = device
            .claim_interface(number)
            .wait()
            .map_err(|e| format!("claim interface {number}: {e}"))?;
        let out = interface
            .endpoint::<Bulk, Out>(ep)
            .map_err(|e| format!("endpoint {ep:#04x}: {e}"))?;
        Ok(OutOnly {
            out,
            _interface: interface,
        })
    }

    /// Write `total` bytes of a pattern in `chunk`-byte transfers, up to `depth` in flight. Returns the bytes the device accepted.
    pub fn write(
        &mut self,
        total: usize,
        chunk: usize,
        depth: usize,
        timeout: Duration,
    ) -> Result<usize, String> {
        let (mut submitted, mut done) = (0usize, 0usize);
        while done < total {
            while submitted < total && self.out.pending() < depth {
                let n = chunk.min(total - submitted);
                let data: Vec<u8> = (0..n).map(|k| (submitted + k) as u8).collect();
                self.out.submit(data.into());
                submitted += n;
            }
            match self.out.wait_next_complete(timeout) {
                Some(c) => {
                    done += c.actual_len;
                    c.status.map_err(|e| {
                        if e == TransferError::Stall {
                            let _ = self.out.clear_halt().wait();
                        }
                        format!("after {done} of {total} bytes: {e}")
                    })?;
                }
                None => {
                    self.out.cancel_all();
                    let _ = self.out.wait_next_complete(Duration::from_millis(500));
                    return Err(format!("timeout after {done} of {total} bytes"));
                }
            }
        }
        // a transfer of a multiple of the packet size ends with a zero-length packet
        if total > 0 && total.is_multiple_of(PACKET) {
            OutPipe::out(self, &[], timeout).map_err(|e| format!("zero-length packet: {e:?}"))?;
        }
        Ok(done)
    }

    /// One whole frame, then a zero-length transfer when its length is a multiple of the packet size.
    pub fn send(&mut self, frame: &[u8], timeout: Duration) -> Result<(), LinkError> {
        send_with_zlp(self, frame, timeout)
    }
}

impl OutPipe for OutOnly {
    fn out(&mut self, data: &[u8], timeout: Duration) -> Result<(), LinkError> {
        self.out.submit(data.to_vec().into());
        match self.out.wait_next_complete(timeout) {
            Some(c) => c.status.map_err(|e| {
                if e == TransferError::Stall {
                    let _ = self.out.clear_halt().wait();
                }
                map(e)
            }),
            None => {
                self.out.cancel_all();
                let _ = self.out.wait_next_complete(Duration::from_millis(500));
                Err(LinkError::Timeout)
            }
        }
    }
}

impl Drop for OutOnly {
    fn drop(&mut self) {
        self.out.cancel_all();
    }
}

fn map(e: TransferError) -> LinkError {
    match e {
        TransferError::Disconnected => LinkError::Disconnected,
        TransferError::Cancelled => LinkError::Timeout,
        other => LinkError::Io(other.to_string()),
    }
}

impl OutPipe for UsbTx {
    fn out(&mut self, data: &[u8], timeout: Duration) -> Result<(), LinkError> {
        self.out.submit(data.to_vec().into());
        match self.out.wait_next_complete(timeout) {
            Some(c) => c.status.map_err(|e| {
                if e == TransferError::Stall {
                    let _ = self.out.clear_halt().wait();
                }
                map(e)
            }),
            None => {
                self.out.cancel_all();
                let _ = self.out.wait_next_complete(Duration::from_millis(500));
                Err(LinkError::Timeout)
            }
        }
    }
}

impl UsbTx {
    /// One frame, then a zero-length transfer when its length is a multiple of the packet size.
    pub fn send(&mut self, frame: &[u8], timeout: Duration) -> Result<(), LinkError> {
        send_with_zlp(self, frame, timeout)
    }
}

impl InPipe for UsbRx {
    fn read(&mut self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        if self.inp.pending() == 0 {
            let buf = self.inp.allocate(IN_SIZE);
            self.inp.submit(buf);
        }
        // a timeout leaves the transfer pending: the next read waits on it
        let c = self
            .inp
            .wait_next_complete(timeout)
            .ok_or(LinkError::Timeout)?;
        if let Err(e) = c.status {
            if e == TransferError::Stall {
                let _ = self.inp.clear_halt().wait();
            }
            return Err(map(e));
        }
        Ok(c.buffer[..c.actual_len].to_vec())
    }
}

impl UsbRx {
    pub fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        let mut reader = std::mem::take(&mut self.reader);
        let r = reader.next(self, timeout);
        self.reader = reader;
        r
    }
}

impl Link for UsbLink {
    fn send(&mut self, frame: &[u8], timeout: Duration) -> Result<(), LinkError> {
        self.tx.send(frame, timeout)
    }
    fn recv(&mut self, timeout: Duration) -> Result<Vec<u8>, LinkError> {
        self.rx.recv(timeout)
    }
    fn close(&mut self) {
        self.tx.out.cancel_all();
        self.rx.inp.cancel_all();
    }
}

impl Drop for UsbTx {
    fn drop(&mut self) {
        self.out.cancel_all();
    }
}
impl Drop for UsbRx {
    fn drop(&mut self) {
        self.inp.cancel_all();
    }
}
