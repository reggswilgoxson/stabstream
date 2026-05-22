/// A fixed-capacity, single-producer single-consumer ring buffer for
/// zero-copy syndrome payload slices.
///
/// Both `read_pos` and `write_pos` are *global* monotonically-increasing
/// byte counts (they wrap at `usize::MAX`, never mod capacity). The actual
/// index into `buf` is always `pos % buf.len()`. This makes `available_read`
/// and two-segment wrap-around logic straightforward.
pub struct RingBuffer {
    buf: Box<[u8]>,
    read_pos: usize,
    write_pos: usize,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: vec![0u8; capacity].into_boxed_slice(),
            read_pos: 0,
            write_pos: 0,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    /// Bytes available to read.
    pub fn available_read(&self) -> usize {
        self.write_pos.wrapping_sub(self.read_pos)
    }

    /// Free bytes available for writing.
    pub fn available_write(&self) -> usize {
        self.buf.len() - self.available_read()
    }

    /// Write as many bytes from `data` as fit. Returns the number written.
    pub fn write(&mut self, data: &[u8]) -> usize {
        let to_write = data.len().min(self.available_write());
        if to_write == 0 {
            return 0;
        }

        let write_idx = self.write_pos % self.buf.len();
        let first_len = (self.buf.len() - write_idx).min(to_write);

        self.buf[write_idx..write_idx + first_len].copy_from_slice(&data[..first_len]);

        if first_len < to_write {
            let second_len = to_write - first_len;
            self.buf[..second_len].copy_from_slice(&data[first_len..first_len + second_len]);
        }

        self.write_pos = self.write_pos.wrapping_add(to_write);
        to_write
    }

    /// Return a contiguous slice of `len` bytes at the read position without
    /// advancing the cursor. Returns `None` if fewer bytes are available or
    /// if the requested region wraps around the end of the buffer.
    pub fn peek(&self, len: usize) -> Option<&[u8]> {
        if self.available_read() < len {
            return None;
        }
        let read_idx = self.read_pos % self.buf.len();
        if read_idx + len <= self.buf.len() {
            Some(&self.buf[read_idx..read_idx + len])
        } else {
            None
        }
    }

    /// Return a slice of `len` bytes at the read position, writing into
    /// `scratch` only when the region wraps around the buffer end.
    ///
    /// The fast path (no wrap) returns a direct borrow of the ring's backing
    /// store with no copy. The wrap path copies at most `len` bytes into
    /// `scratch` (which callers should pre-allocate to avoid heap churn).
    ///
    /// Returns `None` if fewer than `len` bytes are available.
    pub fn peek_wrapped<'a>(&'a self, len: usize, scratch: &'a mut Vec<u8>) -> Option<&'a [u8]> {
        if self.available_read() < len {
            return None;
        }
        let read_idx = self.read_pos % self.buf.len();
        if read_idx + len <= self.buf.len() {
            Some(&self.buf[read_idx..read_idx + len])
        } else {
            // Wrap path: at most one field per frame reaches here (a ring ≥ max_frame_size
            // guarantees subsequent fields start past the boundary and use the fast path).
            // reserve before clear so reallocation happens before any new pointer is taken.
            scratch.reserve(len);
            scratch.clear();
            let first = self.buf.len() - read_idx;
            scratch.extend_from_slice(&self.buf[read_idx..]);
            scratch.extend_from_slice(&self.buf[..len - first]);
            Some(scratch.as_slice())
        }
    }

    /// Advance the read cursor by `len` bytes, releasing buffer space.
    pub fn consume(&mut self, len: usize) {
        self.read_pos = self.read_pos.wrapping_add(len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_peek_consume_no_wrap() {
        let mut rb = RingBuffer::new(64);
        let data = b"hello world";
        assert_eq!(rb.write(data), data.len());
        assert_eq!(rb.available_read(), data.len());
        assert_eq!(rb.peek(data.len()), Some(data.as_slice()));
        rb.consume(data.len());
        assert_eq!(rb.available_read(), 0);
    }

    #[test]
    fn write_honours_capacity() {
        let mut rb = RingBuffer::new(8);
        let data = [0u8; 10];
        assert_eq!(rb.write(&data), 8); // can only fit 8
        assert_eq!(rb.available_read(), 8);
    }

    #[test]
    fn wrap_around_write() {
        let mut rb = RingBuffer::new(8);
        rb.write(&[1, 2, 3, 4, 5, 6]);
        rb.consume(4); // read_pos = 4, write_pos = 6
        rb.write(&[7, 8, 9, 10]); // should wrap: 2 bytes at end, 2 at start
        assert_eq!(rb.available_read(), 6);
    }

    #[test]
    fn wrap_around_peek_returns_none() {
        // Capacity = 8.  Write 6 bytes, consume 5 → read_pos=5, write_pos=6.
        // Then write 4 more → wraps: bytes [6,7] go to indices 6,7; [8,9] at 0,1.
        // Now read_idx = 5, available = 5 bytes, but they span indices 5..7 + 0..2
        // which is non-contiguous. peek(5) must return None.
        let mut rb = RingBuffer::new(8);
        rb.write(&[1, 2, 3, 4, 5, 6]);
        rb.consume(5); // read_pos=5, write_pos=6, avail=1
        let written = rb.write(&[7, 8, 9, 10]); // wraps; 2 at [6..8], 2 at [0..2]
        assert_eq!(written, 4);
        assert_eq!(rb.available_read(), 5);
        // The 5 bytes wrap around the ring end → peek must be None.
        assert!(rb.peek(5).is_none(), "expected None for wrapping peek");
        // But a smaller non-wrapping peek from index 5 still works (1 byte available at [5]).
        assert!(rb.peek(1).is_some());
    }

    #[test]
    fn peek_wrapped_handles_wrap_around() {
        // Same setup as wrap_around_peek_returns_none, but peek_wrapped must
        // return the correct bytes rather than None.
        let mut rb = RingBuffer::new(8);
        rb.write(&[1, 2, 3, 4, 5, 6]);
        rb.consume(5);
        rb.write(&[7, 8, 9, 10]);
        assert_eq!(rb.available_read(), 5);
        // Data in ring: index 5→6, index 6→7, index 7→8, index 0→9, index 1→10
        let mut scratch = Vec::new();
        let result = rb.peek_wrapped(5, &mut scratch);
        assert_eq!(result, Some([6u8, 7, 8, 9, 10].as_slice()));
    }

    #[test]
    fn peek_wrapped_fast_path_no_alloc() {
        // Non-wrapping case: peek_wrapped returns a direct borrow (scratch unused).
        let mut rb = RingBuffer::new(64);
        rb.write(&[10, 20, 30, 40, 50]);
        let mut scratch = Vec::new();
        let result = rb.peek_wrapped(5, &mut scratch);
        assert_eq!(result, Some([10u8, 20, 30, 40, 50].as_slice()));
        assert!(
            scratch.is_empty(),
            "scratch must not be written for non-wrapping peek"
        );
    }

    #[test]
    fn back_to_back_wraps() {
        // 5 write/consume cycles; each cycle writes more than half the capacity
        // so the write position crosses the buffer end repeatedly.
        let mut rb = RingBuffer::new(16);
        for cycle in 0u8..5 {
            let data: Vec<u8> = (0..10).map(|i| cycle * 10 + i).collect();
            assert_eq!(rb.write(&data), 10);
            assert_eq!(rb.available_read(), 10);
            // Consume all bytes; positions advance by 10 each cycle.
            rb.consume(10);
            assert_eq!(rb.available_read(), 0);
        }
        // After 5 cycles: write_pos = read_pos = 50. Buffer is empty and healthy.
        assert_eq!(rb.available_write(), 16);
    }

    #[test]
    fn available_read_after_wrap() {
        let mut rb = RingBuffer::new(8);
        // Advance positions so read_pos and write_pos both sit near usize wrap-around.
        // Simulate by doing many small cycles.  usize overflow tested only on platforms
        // where usize is small; here we just verify the wrapping arithmetic holds.
        rb.write(&[1, 2, 3]);
        rb.consume(3);
        rb.write(&[4, 5, 6, 7]);
        assert_eq!(rb.available_read(), 4);
        rb.consume(2);
        assert_eq!(rb.available_read(), 2);
        rb.write(&[8, 9, 10, 11]); // write_pos wraps past buf end
        assert_eq!(rb.available_read(), 6);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// After any sequence of writes and partial consumes the invariant
        /// `available_read + available_write == capacity` must always hold.
        #[test]
        fn capacity_invariant(
            ops in prop::collection::vec((1usize..=32usize, 0.0f64..=1.0f64), 1..=20),
        ) {
            let cap = 64usize;
            let mut rb = RingBuffer::new(cap);
            for (write_size, consume_frac) in ops {
                let write_size = write_size.min(rb.available_write());
                if write_size > 0 {
                    let data = vec![0u8; write_size];
                    rb.write(&data);
                }
                let to_consume = ((rb.available_read() as f64) * consume_frac) as usize;
                rb.consume(to_consume);
                prop_assert_eq!(
                    rb.available_read() + rb.available_write(),
                    cap,
                    "capacity invariant broken after op"
                );
            }
        }

        /// Writing a payload and immediately peeking the same length must
        /// return the original bytes (when no wrap-around occurs).
        #[test]
        fn write_peek_consume_roundtrip(payload in prop::collection::vec(any::<u8>(), 1..=512)) {
            let cap = 1024usize;
            let mut rb = RingBuffer::new(cap);
            let n = rb.write(&payload);
            prop_assert_eq!(n, payload.len());
            let peeked = rb.peek(payload.len()).expect("peek should succeed without wrap");
            prop_assert_eq!(peeked, payload.as_slice());
            rb.consume(payload.len());
            prop_assert_eq!(rb.available_read(), 0);
        }
    }
}
