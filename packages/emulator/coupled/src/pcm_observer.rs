//! Bounded observation of the latest source-rate stereo PCM window.

use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst},
};

const FRAMES: usize = 2048;

#[derive(serde::Serialize)]
pub struct PcmWindow {
    rate: u32,
    generation: u64,
    left: Vec<f32>,
    right: Vec<f32>,
}

#[derive(Default)]
pub struct PcmSource {
    requested: AtomicU32,
    generation: AtomicU64,
    ring: Mutex<Weak<Ring>>,
}

#[derive(Default)]
pub struct PcmCursor {
    ring: Weak<Ring>,
    revision: u64,
}

struct Ring {
    rate: u32,
    connected: AtomicBool,
    /// One atomic per stereo pair. The revision protects a read across pairs.
    pcm: Box<[AtomicU64]>,
    revision: AtomicU64,
    generation: AtomicU64,
    session: AtomicU32,
    end: AtomicU64,
}

pub struct PcmWriter {
    source: Arc<PcmSource>,
    ring: Arc<Ring>,
    session: u32,
    end: u64,
}

/// Owned by the audio stream. Its callbacks may still hold a writer after stop.
pub struct PcmConnection(Arc<Ring>);

impl Drop for PcmConnection {
    fn drop(&mut self) {
        self.0.connected.store(false, SeqCst);
    }
}

impl PcmSource {
    /// Select the subscriber for this source. Zero disables observation.
    pub fn subscribe(&self, session: u32) {
        self.requested.store(session, SeqCst);
    }

    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, SeqCst) + 1
    }

    pub fn connect(self: &Arc<Self>, rate: u32) -> (PcmWriter, PcmConnection) {
        let ring = Arc::new(Ring {
            rate,
            connected: AtomicBool::new(true),
            pcm: (0..FRAMES).map(|_| AtomicU64::new(0)).collect(),
            revision: AtomicU64::new(0),
            generation: AtomicU64::new(self.next_generation()),
            session: AtomicU32::new(0),
            end: AtomicU64::new(0),
        });
        *self.ring.lock().unwrap() = Arc::downgrade(&ring);
        let writer = PcmWriter {
            source: self.clone(),
            ring: ring.clone(),
            session: 0,
            end: 0,
        };
        (writer, PcmConnection(ring))
    }

    /// Copies the most recent complete window if it advanced. A concurrent write
    /// may invalidate this read, in which case the next poll can read a later one.
    pub fn read(&self, session: u32, cursor: &mut PcmCursor) -> Option<PcmWindow> {
        let current = self.ring.lock().unwrap();
        let ring = current.upgrade()?;
        let revision = ring.revision.load(SeqCst);
        if (revision & 1) != 0
            || (cursor.revision == revision && Weak::ptr_eq(&cursor.ring, &current))
            || ring.session.load(SeqCst) != session
            || self.requested.load(SeqCst) != session
            || !ring.connected.load(SeqCst)
        {
            return None;
        }
        let generation = ring.generation.load(SeqCst);
        let end = ring.end.load(SeqCst);
        let start = end.checked_sub(FRAMES as u64)?;
        let mut left = Vec::with_capacity(FRAMES);
        let mut right = Vec::with_capacity(FRAMES);
        for index in start..end {
            let pair = ring.pcm[index as usize % FRAMES].load(SeqCst);
            left.push(f32::from_bits(pair as u32));
            right.push(f32::from_bits((pair >> 32) as u32));
        }
        if ring.revision.load(SeqCst) != revision || !ring.connected.load(SeqCst) {
            return None;
        }
        cursor.ring = current.clone();
        cursor.revision = revision;
        Some(PcmWindow {
            rate: ring.rate,
            generation,
            left,
            right,
        })
    }
}

impl PcmWriter {
    /// Accept source-rate interleaved stereo PCM on the single producer thread.
    /// No allocation, locking or waiting here, including when the reader falls behind.
    pub fn push(&mut self, pcm: &[f32]) {
        let session = self.source.requested.load(SeqCst);
        let frames = pcm.len() / 2;
        if session == 0 || frames == 0 {
            return;
        }
        self.ring.revision.fetch_add(1, SeqCst);
        if self.session != session {
            self.session = session;
            self.end = 0;
            self.ring.session.store(session, SeqCst);
        }
        let skip = frames.saturating_sub(FRAMES);
        for (offset, pair) in pcm[skip * 2..].chunks_exact(2).enumerate() {
            let left = normalized(pair[0]).to_bits();
            let right = normalized(pair[1]).to_bits();
            let index = (self.end as usize + skip + offset) % FRAMES;
            self.ring.pcm[index].store(u64::from(left) | (u64::from(right) << 32), SeqCst);
        }
        self.end += frames as u64;
        self.ring.end.store(self.end, SeqCst);
        self.ring.revision.fetch_add(1, SeqCst);
    }

    /// Flush the partial and complete windows across an in-place source reset.
    pub(crate) fn reset(&mut self) {
        self.ring.revision.fetch_add(1, SeqCst);
        self.end = 0;
        self.ring.end.store(0, SeqCst);
        self.ring
            .generation
            .store(self.source.next_generation(), SeqCst);
        self.ring.revision.fetch_add(1, SeqCst);
    }
}

fn normalized(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

impl crate::CoupledRuntime {
    /// Attach an independent latest-window observer to the source PCM.
    pub fn observe_pcm_windows(&mut self, source: &Arc<PcmSource>) {
        if self.has_audio() {
            self.out.observe_windows(source);
        }
    }
}

#[cfg(test)]
mod tests;
