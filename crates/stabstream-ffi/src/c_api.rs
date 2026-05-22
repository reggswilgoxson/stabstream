use std::ffi::c_char;

use stabstream_sim::shm::ShmProducer;

use crate::inner::{open_inner, InnerHandle};

// ---------------------------------------------------------------------------
// Public C ABI types
// ---------------------------------------------------------------------------

/// Opaque handle to a QSSF stream reader.
///
/// Obtain via [`stabstream_open`]; free via [`stabstream_close`].
#[repr(C)]
pub struct StabstreamHandle {
    _private: [u8; 0],
}

/// Result codes returned by the C API.
#[repr(C)]
pub enum StabstreamStatus {
    Ok = 0,
    IoError = 1,
    ParseError = 2,
    InvalidArg = 3,
    EndOfStream = 4,
}

// ---------------------------------------------------------------------------
// Exported functions
// ---------------------------------------------------------------------------

/// Open a QSSF source. `source` may be a TCP URI (`tcp://host:port`) or a
/// path to a file.
///
/// Returns a non-null handle on success, or null on failure.
///
/// # Safety
///
/// `source` must be a valid, null-terminated UTF-8 string. The returned pointer
/// must be freed with [`stabstream_close`].
#[no_mangle]
pub unsafe extern "C" fn stabstream_open(source: *const c_char) -> *mut StabstreamHandle {
    if source.is_null() {
        return std::ptr::null_mut();
    }
    let source_str = match unsafe { std::ffi::CStr::from_ptr(source) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    match open_inner(source_str) {
        Ok(inner) => Box::into_raw(Box::new(inner)) as *mut StabstreamHandle,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Read the next syndrome frame from `handle` into `out_buf`.
///
/// The buffer is filled with a flat little-endian layout:
/// ```text
/// [0..8]   frame_id             u64 LE
/// [8..12]  round                u32 LE
/// [12..20] timestamp_ns         u64 LE
/// [20..22] qubit_count          u16 LE
/// [22..24] ancilla_count        u16 LE
/// [24..28] detector_event_count u32 LE
/// [28..]   meas_results         ancilla_count bytes (i8 reinterpreted as u8)
/// ```
///
/// Returns the number of bytes written on success, or a negative
/// [`StabstreamStatus`] cast to `i64` on error.
///
/// # Safety
///
/// - `handle` must be a live pointer obtained from [`stabstream_open`].
/// - `out_buf` must point to a buffer of at least `buf_len` bytes.
#[no_mangle]
pub unsafe extern "C" fn stabstream_next_frame(
    handle: *mut StabstreamHandle,
    out_buf: *mut u8,
    buf_len: usize,
) -> i64 {
    if handle.is_null() || out_buf.is_null() || buf_len == 0 {
        return -(StabstreamStatus::InvalidArg as i64);
    }

    let inner = unsafe { &mut *(handle as *mut InnerHandle) };

    match inner.source.next_frame_owned(&inner.runtime) {
        Ok(None) => -(StabstreamStatus::EndOfStream as i64),
        Ok(Some(frame)) => {
            let needed = 28 + frame.meas_results.len();
            if buf_len < needed {
                return -(StabstreamStatus::InvalidArg as i64);
            }
            inner.last_observable_flips = frame.observable_flips;
            let buf = unsafe { std::slice::from_raw_parts_mut(out_buf, buf_len) };
            buf[0..8].copy_from_slice(&frame.frame_id.to_le_bytes());
            buf[8..12].copy_from_slice(&frame.round.to_le_bytes());
            buf[12..20].copy_from_slice(&frame.timestamp_ns.to_le_bytes());
            buf[20..22].copy_from_slice(&frame.qubit_count.to_le_bytes());
            buf[22..24].copy_from_slice(&frame.ancilla_count.to_le_bytes());
            buf[24..28].copy_from_slice(&frame.detector_event_count.to_le_bytes());
            buf[28..needed].copy_from_slice(&frame.meas_results);
            needed as i64
        }
        Err(_) => -(StabstreamStatus::ParseError as i64),
    }
}

/// Close and free a stream handle obtained from [`stabstream_open`].
///
/// # Safety
///
/// `handle` must be a live pointer obtained from [`stabstream_open`]. Calling
/// this function twice on the same pointer is safe (the second call is a no-op)
/// but the caller must not dereference the handle after the first close.
#[no_mangle]
pub unsafe extern "C" fn stabstream_close(handle: *mut StabstreamHandle) {
    if handle.is_null() {
        return;
    }
    let inner = handle as *mut InnerHandle;
    // Atomically mark closed; if already closed return without double-freeing.
    if unsafe {
        (*inner)
            .closed
            .swap(true, std::sync::atomic::Ordering::Acquire)
    } {
        return;
    }
    drop(unsafe { Box::from_raw(inner) });
}

/// Return the library version string as a null-terminated C string.
///
/// The returned pointer points to a static string and must not be freed.
#[no_mangle]
pub extern "C" fn stabstream_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// Return the `observable_flips` bitmask for the most recently read frame.
///
/// Each set bit `i` means logical qubit `i` requires a correction.  The value
/// is taken from TLV metadata tag `0x10` when present (simulator ground-truth
/// path) or set to `0` when the frame carries no metadata (real hardware path
/// where a full decoder must be wired in separately).
///
/// Must be called after a successful [`stabstream_next_frame`] call on the same
/// handle.  Calling before any frame has been read returns `0`.
///
/// # Safety
///
/// `handle` must be a live pointer obtained from [`stabstream_open`].
#[no_mangle]
pub unsafe extern "C" fn stabstream_decode_frame(handle: *mut StabstreamHandle) -> i64 {
    if handle.is_null() {
        return -(StabstreamStatus::InvalidArg as i64);
    }
    let inner = unsafe { &*(handle as *const InnerHandle) };
    inner.last_observable_flips.unwrap_or(0) as i64
}

// ---------------------------------------------------------------------------
// SHM producer API — for FPGA/C code writing syndrome frames into the SHM ring
// ---------------------------------------------------------------------------

/// Opaque handle to a SHM syndrome-frame producer.
///
/// Obtain via [`stabstream_shm_open`]; free via [`stabstream_shm_close`].
#[repr(C)]
pub struct StabstreamShmHandle {
    _private: [u8; 0],
}

/// Create a POSIX SHM ring at `/dev/shm/<name>` and return a producer handle.
///
/// Any existing file at that path is truncated.  Returns null on failure
/// (e.g., permission denied, name is null or not valid UTF-8).
///
/// # Safety
///
/// `name` must be a valid, null-terminated UTF-8 string.  The returned pointer
/// must be freed with [`stabstream_shm_close`].
#[no_mangle]
pub unsafe extern "C" fn stabstream_shm_open(name: *const c_char) -> *mut StabstreamShmHandle {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    let name_str = match unsafe { std::ffi::CStr::from_ptr(name) }.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    match ShmProducer::create(name_str) {
        Ok(producer) => Box::into_raw(Box::new(producer)) as *mut StabstreamShmHandle,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Write one QSSF frame into the SHM ring.
///
/// `data` must point to a valid QSSF frame of exactly `len` bytes.  The frame
/// must be ≤ 4096 bytes (the per-slot maximum).  The ring silently overwrites
/// the oldest slot when full — consumers that fall more than 256 frames behind
/// will observe an overrun.
///
/// Returns `0` on success, `-1` on failure (frame too large or mmap error).
///
/// # Safety
///
/// - `handle` must be a live pointer obtained from [`stabstream_shm_open`].
/// - `data` must point to at least `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn stabstream_shm_write(
    handle: *mut StabstreamShmHandle,
    data: *const u8,
    len: usize,
) -> i32 {
    if handle.is_null() || data.is_null() {
        return -1;
    }
    let producer = unsafe { &mut *(handle as *mut ShmProducer) };
    let frame = unsafe { std::slice::from_raw_parts(data, len) };
    match producer.write_frame(frame) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

/// Close and free a SHM producer handle obtained from [`stabstream_shm_open`].
///
/// The `/dev/shm/<name>` file is **not** deleted — consumers may still drain
/// buffered frames.  To remove the file, call `shm_unlink` (POSIX) or
/// `unlink("/dev/shm/<name>")` after closing.
///
/// # Safety
///
/// `handle` must be a live pointer obtained from [`stabstream_shm_open`].
/// Calling this function twice on the same pointer is undefined behaviour.
#[no_mangle]
pub unsafe extern "C" fn stabstream_shm_close(handle: *mut StabstreamShmHandle) {
    if handle.is_null() {
        return;
    }
    drop(unsafe { Box::from_raw(handle as *mut ShmProducer) });
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use stabstream_deserialize::testutil::synthetic_surface_d5_stream;

    #[test]
    fn open_next_close_file() {
        let bytes = synthetic_surface_d5_stream(3, 0.05);
        let tmp = std::env::temp_dir().join("stabstream_ffi_test.qssf");
        std::fs::write(&tmp, &bytes).unwrap();
        let path = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();

        let handle = unsafe { stabstream_open(path.as_ptr()) };
        assert!(!handle.is_null(), "stabstream_open returned null");

        let mut buf = vec![0u8; 1024];
        for i in 0..3 {
            let n = unsafe { stabstream_next_frame(handle, buf.as_mut_ptr(), buf.len()) };
            assert!(n > 0, "frame {i}: expected bytes written, got {n}");
            let frame_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
            assert_eq!(frame_id, i as u64, "frame_id mismatch at round {i}");
        }

        let eof = unsafe { stabstream_next_frame(handle, buf.as_mut_ptr(), buf.len()) };
        assert_eq!(eof, -(StabstreamStatus::EndOfStream as i64), "expected EOF");

        unsafe { stabstream_close(handle) };
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn open_null_returns_null() {
        let handle = unsafe { stabstream_open(std::ptr::null()) };
        assert!(handle.is_null());
    }

    #[test]
    fn next_frame_null_handle() {
        let mut buf = vec![0u8; 64];
        let ret =
            unsafe { stabstream_next_frame(std::ptr::null_mut(), buf.as_mut_ptr(), buf.len()) };
        assert_eq!(ret, -(StabstreamStatus::InvalidArg as i64));
    }

    #[test]
    fn version_not_null() {
        let v = stabstream_version();
        assert!(!v.is_null());
        let s = unsafe { std::ffi::CStr::from_ptr(v) }.to_str().unwrap();
        assert!(!s.is_empty());
    }

    #[test]
    fn double_close_is_safe() {
        let bytes = synthetic_surface_d5_stream(1, 0.05);
        let tmp = std::env::temp_dir().join("stabstream_ffi_double_close.qssf");
        std::fs::write(&tmp, &bytes).unwrap();
        let path = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();

        let handle = unsafe { stabstream_open(path.as_ptr()) };
        assert!(!handle.is_null());

        unsafe { stabstream_close(handle) };
        // Second close must not trigger UB (double-free). The AtomicBool guard
        // makes this a safe no-op.
        unsafe { stabstream_close(handle) };

        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn decode_frame_before_any_read_returns_zero() {
        let bytes = synthetic_surface_d5_stream(1, 0.05);
        let tmp = std::env::temp_dir().join("stabstream_ffi_decode_before.qssf");
        std::fs::write(&tmp, &bytes).unwrap();
        let path = std::ffi::CString::new(tmp.to_str().unwrap()).unwrap();

        let handle = unsafe { stabstream_open(path.as_ptr()) };
        assert!(!handle.is_null());

        // No frame read yet — must return 0.
        let flips = unsafe { stabstream_decode_frame(handle) };
        assert_eq!(flips, 0);

        unsafe { stabstream_close(handle) };
        std::fs::remove_file(&tmp).ok();
    }

    #[test]
    fn decode_frame_null_handle() {
        let ret = unsafe { stabstream_decode_frame(std::ptr::null_mut()) };
        assert_eq!(ret, -(StabstreamStatus::InvalidArg as i64));
    }

    #[test]
    fn shm_open_null_returns_null() {
        let handle = unsafe { stabstream_shm_open(std::ptr::null()) };
        assert!(handle.is_null());
    }

    #[test]
    fn shm_write_null_handle_returns_error() {
        let data = [0u8; 16];
        let ret = unsafe { stabstream_shm_write(std::ptr::null_mut(), data.as_ptr(), data.len()) };
        assert_eq!(ret, -1);
    }

    #[test]
    fn shm_producer_roundtrip() {
        use stabstream_sim::shm::ShmConsumer;

        let name = "stabstream_ffi_shm_test";
        let name_c = std::ffi::CString::new(name).unwrap();

        // Create producer via C API.
        let prod = unsafe { stabstream_shm_open(name_c.as_ptr()) };
        assert!(!prod.is_null(), "stabstream_shm_open returned null");

        // Write a short frame.
        let payload = b"hello_shm_frame";
        let ret = unsafe { stabstream_shm_write(prod, payload.as_ptr(), payload.len()) };
        assert_eq!(ret, 0, "stabstream_shm_write failed");

        // Read back via Rust consumer.
        let mut consumer = ShmConsumer::open(name).expect("consumer open failed");
        let frame = consumer
            .read_frame_blocking()
            .expect("read_frame_blocking failed");
        assert_eq!(&frame, payload);

        unsafe { stabstream_shm_close(prod) };
        std::fs::remove_file(format!("/dev/shm/{name}")).ok();
    }
}
