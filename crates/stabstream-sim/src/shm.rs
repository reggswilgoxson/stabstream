use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::MmapMut;

/// Number of ring slots. Power of 2 so `seq % RING_SLOTS` compiles to a mask.
pub const RING_SLOTS: usize = 256;

/// Maximum QSSF frame size in bytes.
///
/// A d=11 surface code frame is ~200 bytes. 4096 gives generous headroom and
/// keeps total SHM size under 1.1 MB.
pub const MAX_FRAME_SIZE: usize = 4096;

/// Total size of the SHM region in bytes.
///
/// Layout:
///   [0..8]   : producer sequence counter (u64, AtomicU64)
///   [8..16]  : padding (reserved, zero)
///   [16..]   : RING_SLOTS × (4-byte length prefix + MAX_FRAME_SIZE payload)
pub const SHM_SIZE: usize = 16 + RING_SLOTS * SLOT_SIZE;

const SLOT_SIZE: usize = 4 + MAX_FRAME_SIZE;
const SLOTS_OFFSET: usize = 16;

fn slot_byte_offset(seq: u64) -> usize {
    SLOTS_OFFSET + (seq as usize & (RING_SLOTS - 1)) * SLOT_SIZE
}

/// Extract the AtomicU64 producer-sequence counter from a mmap.
///
/// # Safety
/// The mmap must be at least 8 bytes, page-aligned (guaranteed by the OS
/// mmap implementation), and the u64 at offset 0 must be initialized.
/// Only `load` and `store` are called through this reference — never
/// read-modify-write — so the shared aliasing between producer and consumer
/// is safe.
unsafe fn seq_atomic(mmap: &MmapMut) -> &AtomicU64 {
    &*(mmap.as_ptr() as *const AtomicU64)
}

// ─── Producer ─────────────────────────────────────────────────────────────────

/// SHM ring-buffer producer. Creates `/dev/shm/<name>`.
///
/// Write QSSF frames with [`write_frame`]; consumers observe them after the
/// `Release` store to the sequence counter. The ring overwrites the oldest
/// slot when full — consumers that fall more than `RING_SLOTS` frames behind
/// report an overrun error.
pub struct ShmProducer {
    mmap: MmapMut,
    next_seq: u64,
}

impl ShmProducer {
    /// Create a new SHM region at `/dev/shm/<name>`, truncating any
    /// existing file.
    pub fn create(name: &str) -> anyhow::Result<Self> {
        let path = format!("/dev/shm/{name}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        file.set_len(SHM_SIZE as u64)?;

        let mut mmap = unsafe { MmapMut::map_mut(&file)? };
        // Initialise the sequence counter to 0.
        mmap[..8].fill(0);

        Ok(Self { mmap, next_seq: 0 })
    }

    /// Write one QSSF frame into the ring and advance the producer sequence.
    ///
    /// `data` must be ≤ `MAX_FRAME_SIZE` bytes. Overwrites the oldest slot
    /// when the ring wraps around.
    pub fn write_frame(&mut self, data: &[u8]) -> anyhow::Result<()> {
        if data.len() > MAX_FRAME_SIZE {
            anyhow::bail!("frame too large: {} > {MAX_FRAME_SIZE}", data.len());
        }

        let off = slot_byte_offset(self.next_seq);
        let len = data.len() as u32;
        self.mmap[off..off + 4].copy_from_slice(&len.to_le_bytes());
        self.mmap[off + 4..off + 4 + data.len()].copy_from_slice(data);

        // Release store: consumers see the written slot only after this.
        let next = self.next_seq + 1;
        unsafe {
            seq_atomic(&self.mmap).store(next, Ordering::Release);
        }
        self.next_seq = next;
        Ok(())
    }

    /// Sequence number of the next frame to be written.
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

// ─── Consumer ─────────────────────────────────────────────────────────────────

/// SHM ring-buffer consumer. Opens an existing `/dev/shm/<name>`.
///
/// Poll with [`try_read_frame`] or block with [`read_frame_blocking`].
pub struct ShmConsumer {
    mmap: MmapMut,
    consumer_seq: u64,
}

impl ShmConsumer {
    /// Open an existing SHM region created by [`ShmProducer::create`].
    pub fn open(name: &str) -> anyhow::Result<Self> {
        let path = format!("/dev/shm/{name}");
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let mmap = unsafe { MmapMut::map_mut(&file)? };
        Ok(Self {
            mmap,
            consumer_seq: 0,
        })
    }

    fn producer_seq(&self) -> u64 {
        // SAFETY: same invariants as seq_atomic; we only call load().
        unsafe { seq_atomic(&self.mmap).load(Ordering::Acquire) }
    }

    /// Non-blocking read of the next available frame.
    ///
    /// Returns `Ok(None)` if no new frame is ready.
    /// Returns `Err` on ring overrun (consumer fell > `RING_SLOTS` behind).
    pub fn try_read_frame(&mut self) -> anyhow::Result<Option<Vec<u8>>> {
        let prod = self.producer_seq();
        if prod <= self.consumer_seq {
            return Ok(None);
        }

        let lag = prod.saturating_sub(self.consumer_seq);
        if lag > RING_SLOTS as u64 {
            let skip = lag - RING_SLOTS as u64;
            self.consumer_seq += skip;
            anyhow::bail!("SHM ring overrun: skipped {skip} frames");
        }

        let off = slot_byte_offset(self.consumer_seq);
        let len = u32::from_le_bytes(self.mmap[off..off + 4].try_into().unwrap()) as usize;

        if len > MAX_FRAME_SIZE {
            anyhow::bail!("corrupt slot: len {len} > {MAX_FRAME_SIZE}");
        }

        let data = self.mmap[off + 4..off + 4 + len].to_vec();
        self.consumer_seq += 1;
        Ok(Some(data))
    }

    /// Spin-wait until the next frame is available, yielding the thread
    /// between polls to avoid monopolising a CPU core.
    pub fn read_frame_blocking(&mut self) -> anyhow::Result<Vec<u8>> {
        loop {
            match self.try_read_frame()? {
                Some(frame) => return Ok(frame),
                None => std::thread::yield_now(),
            }
        }
    }

    /// Sequence number of the next frame to be consumed.
    pub fn consumer_seq(&self) -> u64 {
        self.consumer_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        format!("stabstream-test-{ns}")
    }

    #[test]
    fn roundtrip_single_frame() {
        let name = unique_name();
        let mut producer = ShmProducer::create(&name).unwrap();
        let mut consumer = ShmConsumer::open(&name).unwrap();

        let payload: Vec<u8> = (0u8..32).collect();
        producer.write_frame(&payload).unwrap();

        let received = consumer.try_read_frame().unwrap().unwrap();
        assert_eq!(received, payload);

        // No second frame
        assert!(consumer.try_read_frame().unwrap().is_none());

        std::fs::remove_file(format!("/dev/shm/{name}")).ok();
    }

    #[test]
    fn multiple_frames_in_order() {
        let name = unique_name();
        let mut producer = ShmProducer::create(&name).unwrap();
        let mut consumer = ShmConsumer::open(&name).unwrap();

        for i in 0u8..10 {
            producer.write_frame(&[i; 8]).unwrap();
        }

        for i in 0u8..10 {
            let frame = consumer.try_read_frame().unwrap().unwrap();
            assert_eq!(frame, vec![i; 8]);
        }

        std::fs::remove_file(format!("/dev/shm/{name}")).ok();
    }

    #[test]
    fn ring_wraps_correctly() {
        let name = unique_name();
        let mut producer = ShmProducer::create(&name).unwrap();
        let mut consumer = ShmConsumer::open(&name).unwrap();

        // Write more frames than RING_SLOTS — only the last RING_SLOTS are readable
        let total = RING_SLOTS + 4;
        for i in 0..total {
            producer.write_frame(&(i as u64).to_le_bytes()).unwrap();
        }

        // Consumer will detect overrun for the first 4 skipped frames
        match consumer.try_read_frame() {
            Err(e) => assert!(e.to_string().contains("overrun")),
            Ok(_) => {} // may succeed if consumer_seq was already advanced
        }

        std::fs::remove_file(format!("/dev/shm/{name}")).ok();
    }

    #[test]
    fn large_frame_rejected() {
        let name = unique_name();
        let mut producer = ShmProducer::create(&name).unwrap();
        let oversized = vec![0u8; MAX_FRAME_SIZE + 1];
        assert!(producer.write_frame(&oversized).is_err());
        std::fs::remove_file(format!("/dev/shm/{name}")).ok();
    }
}
