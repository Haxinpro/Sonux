//! Minimal lock-free SPSC ring buffer for f32 samples, connecting the mic
//! capture callback (producer) to the virtual-source playback callback
//! (consumer). Both run on PipeWire data threads; no locks, no allocation.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// Next write position (producer-owned).
    write: AtomicUsize,
    /// Next read position (consumer-owned).
    read: AtomicUsize,
}

impl Ring {
    /// Capacity is rounded up to a power of two; effective capacity is
    /// `capacity - 1` (one slot distinguishes full from empty).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(2);
        let buf = (0..cap).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Self {
            buf: buf.into_boxed_slice(),
            write: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    fn mask(&self) -> usize {
        self.buf.len() - 1
    }

    /// Push samples; drops the oldest unread data on overflow (live audio -
    /// stale samples are worthless).
    pub fn push(&self, samples: &[f32]) {
        let mask = self.mask();
        let mut w = self.write.load(Ordering::Relaxed);
        for &s in samples {
            self.buf[w & mask].store(s.to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
        // If the producer lapped the consumer, advance the read cursor so
        // the consumer only ever sees the freshest window.
        let r = self.read.load(Ordering::Acquire);
        let len = self.buf.len();
        if w.wrapping_sub(r) > len {
            self.read.store(w.wrapping_sub(len), Ordering::Release);
        }
    }

    /// Pop up to `out.len()` samples; unfilled tail is zeroed (underrun).
    /// Returns the number of real samples written.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let mask = self.mask();
        let w = self.write.load(Ordering::Acquire);
        let mut r = self.read.load(Ordering::Relaxed);
        let avail = w.wrapping_sub(r).min(out.len());
        for slot in out.iter_mut().take(avail) {
            *slot = f32::from_bits(self.buf[r & mask].load(Ordering::Relaxed));
            r = r.wrapping_add(1);
        }
        self.read.store(r, Ordering::Release);
        for slot in out.iter_mut().skip(avail) {
            *slot = 0.0;
        }
        avail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_underrun() {
        let ring = Ring::new(8);
        ring.push(&[1.0, 2.0, 3.0]);
        let mut out = [0.0f32; 5];
        let n = ring.pop(&mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[1.0, 2.0, 3.0]);
        assert_eq!(&out[3..], &[0.0, 0.0]); // underrun zero-fill
    }

    #[test]
    fn overflow_keeps_freshest_window(// producer overruns consumer
    ) {
        let ring = Ring::new(4); // effective window of 4
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        ring.push(&data);
        let mut out = [0.0f32; 4];
        let n = ring.pop(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out, [6.0, 7.0, 8.0, 9.0]); // freshest 4 survive
    }
}
