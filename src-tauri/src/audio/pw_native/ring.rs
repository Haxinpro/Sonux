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

    /// Push as many samples as fit and return the count accepted. New samples
    /// are dropped on overflow rather than overwriting unread slots: writing
    /// over a slot before publishing the new cursor lets a concurrent reader
    /// observe a torn mixture of old and future audio.
    pub fn push(&self, samples: &[f32]) -> usize {
        let mask = self.mask();
        let mut w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        let used = w.wrapping_sub(r).min(self.buf.len());
        let accepted = samples.len().min(self.buf.len() - used);
        for &s in &samples[..accepted] {
            self.buf[w & mask].store(s.to_bits(), Ordering::Relaxed);
            w = w.wrapping_add(1);
        }
        self.write.store(w, Ordering::Release);
        accepted
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

    /// Snapshot the producer's next write position. The producer uses this to
    /// publish an exact lifecycle boundary before it writes resumed audio.
    pub fn write_position(&self) -> usize {
        self.write.load(Ordering::Relaxed)
    }

    /// Consumer-side lifecycle boundary: discard samples only through a
    /// producer cursor captured before fresh audio was published. If the
    /// consumer has already passed the boundary, leave its cursor unchanged.
    pub fn discard_through(&self, boundary: usize) -> usize {
        let r = self.read.load(Ordering::Relaxed);
        let discarded = boundary.wrapping_sub(r);
        if discarded > self.buf.len() {
            return 0;
        }
        self.read.store(boundary, Ordering::Release);
        discarded
    }

    /// Consumer-side resume boundary: discard everything published before the
    /// current write cursor. Samples published concurrently after this load
    /// remain available on the next pop.
    #[cfg(test)]
    pub fn discard_pending(&self) -> usize {
        let w = self.write.load(Ordering::Acquire);
        let r = self.read.load(Ordering::Relaxed);
        let discarded = w.wrapping_sub(r).min(self.buf.len());
        self.read.store(w, Ordering::Release);
        discarded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_underrun() {
        let ring = Ring::new(8);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0]), 3);
        let mut out = [0.0f32; 5];
        let n = ring.pop(&mut out);
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[1.0, 2.0, 3.0]);
        assert_eq!(&out[3..], &[0.0, 0.0]); // underrun zero-fill
    }

    #[test]
    fn overflow_drops_new_samples_without_overwriting_unread_audio() {
        let ring = Ring::new(4); // effective window of 4
        let data: Vec<f32> = (0..10).map(|i| i as f32).collect();
        assert_eq!(ring.push(&data), 4);
        let mut out = [0.0f32; 4];
        let n = ring.pop(&mut out);
        assert_eq!(n, 4);
        assert_eq!(out, [0.0, 1.0, 2.0, 3.0]);
    }

    #[test]
    fn full_ring_does_not_touch_unread_slots() {
        let ring = Ring::new(4);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0]), 4);

        assert_eq!(ring.push(&[8.0, 9.0, 10.0, 11.0]), 0);

        let mut out = [0.0; 4];
        assert_eq!(ring.pop(&mut out), 4);
        assert_eq!(out, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn producer_reuses_only_slots_published_by_the_consumer() {
        let ring = Ring::new(4);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0]), 4);
        let mut first = [0.0; 2];
        assert_eq!(ring.pop(&mut first), 2);
        assert_eq!(ring.push(&[5.0, 6.0, 7.0]), 2);
        let mut rest = [0.0; 4];
        assert_eq!(ring.pop(&mut rest), 4);
        assert_eq!(rest, [3.0, 4.0, 5.0, 6.0]);
    }

    #[test]
    fn discard_pending_drops_old_audio_but_keeps_later_writes() {
        let ring = Ring::new(8);
        assert_eq!(ring.push(&[1.0, 2.0, 3.0, 4.0]), 4);
        assert_eq!(ring.discard_pending(), 4);
        assert_eq!(ring.push(&[5.0, 6.0]), 2);

        let mut out = [0.0; 4];
        assert_eq!(ring.pop(&mut out), 2);
        assert_eq!(out, [5.0, 6.0, 0.0, 0.0]);
    }

    #[test]
    fn discard_through_preserves_samples_published_after_boundary() {
        let ring = Ring::new(8);
        assert_eq!(ring.push(&[-1.0, -0.5]), 2);
        let boundary = ring.write_position();
        assert_eq!(ring.push(&[1.0, 0.5]), 2);

        assert_eq!(ring.discard_through(boundary), 2);
        let mut out = [0.0; 4];
        assert_eq!(ring.pop(&mut out), 2);
        assert_eq!(out, [1.0, 0.5, 0.0, 0.0]);

        // Replaying an old boundary must never move the consumer backwards.
        assert_eq!(ring.discard_through(boundary), 0);
    }
}
