/// A fixed-capacity, single-producer single-consumer ring buffer
/// for zero-copy syndrome payload slices.
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

    pub fn available_read(&self) -> usize {
        self.write_pos.wrapping_sub(self.read_pos) % self.buf.len()
    }

    /// Write bytes into the buffer. Returns the number of bytes actually written.
    pub fn write(&mut self, _data: &[u8]) -> usize {
        // TODO: implement wrapping write
        todo!()
    }

    /// Return a contiguous slice of `len` bytes at the read position without
    /// advancing the cursor. Returns `None` if fewer bytes are available or if
    /// the region wraps (two-segment case is not yet implemented).
    pub fn peek(&self, _len: usize) -> Option<&[u8]> {
        // TODO: handle wrap-around (two-segment case)
        todo!()
    }

    /// Advance the read cursor by `len` bytes, releasing buffer space.
    pub fn consume(&mut self, len: usize) {
        self.read_pos = self.read_pos.wrapping_add(len) % self.buf.len();
    }
}
