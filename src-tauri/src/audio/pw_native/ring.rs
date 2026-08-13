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
    /// Capacity is rounded up to a power of two. Monotonic cursors distinguish
    /// full from empty, so the complete allocation is usable.
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

    /// Push samples. The producer owns only `write`; on overflow the consumer
    /// notices that it has fallen behind and skips to the freshest window.
    /// Keeping a single writer per cursor prevents a stale producer update
    /// from moving `read` backward after the consumer has advanced it.
    pub fn push(&self, samples: &[f32]) {
        let mask = self.mask();
        let mut w = self.write.load(Ordering::Relaxed);
        for &s in samples {
            self.buf[w & mask].store(s.to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
    }

    /// Pop up to `out.len()` samples; unfilled tail is zeroed (underrun).
    /// Returns the number of real samples written.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let mask = self.mask();
        let w = self.write.load(Ordering::Acquire);
        let mut r = self.read.load(Ordering::Relaxed);
        // The producer may overwrite old samples, but never touches this
        // cursor. Catch up before reading so only its freshest full window is
        // visible after an overrun.
        if w.wrapping_sub(r) > self.buf.len() {
            r = w.wrapping_sub(self.buf.len());
        }
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

    #[test]
    fn producer_never_writes_the_consumer_cursor() {
        let ring = Ring::new(4);
        ring.write.store(8, Ordering::Relaxed);
        ring.read.store(5, Ordering::Relaxed);

        ring.push(&[8.0, 9.0, 10.0, 11.0]);

        assert_eq!(ring.read.load(Ordering::Relaxed), 5);
        let mut out = [0.0; 4];
        assert_eq!(ring.pop(&mut out), 4);
        assert_eq!(out, [8.0, 9.0, 10.0, 11.0]);
    }
}
