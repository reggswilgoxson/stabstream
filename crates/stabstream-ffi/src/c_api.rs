use std::ffi::c_char;

use stabstream_core::error::StabstreamError;
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::{AsyncRead, BufReader};
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// Private implementation types
// ---------------------------------------------------------------------------

struct OwnedFrame {
    frame_id: u64,
    round: u32,
    timestamp_ns: u64,
    qubit_count: u16,
    ancilla_count: u16,
    detector_event_count: u32,
    meas_results: Vec<u8>,
}

trait FrameProducer: Send {
    fn next_frame_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError>;
}

struct QssfProducer<R: AsyncRead + Unpin + Send + 'static> {
    stream: QssfStream<R>,
}

impl<R: AsyncRead + Unpin + Send + 'static> FrameProducer for QssfProducer<R> {
    fn next_frame_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(frame) => {
                    let detector_event_count = frame.detector_event_count();
                    let meas_results = frame
                        .payload
                        .meas_results
                        .iter()
                        .map(|&v| v as u8)
                        .collect();
                    Ok(Some(OwnedFrame {
                        frame_id: frame.header.frame_id,
                        round: frame.header.round,
                        timestamp_ns: frame.header.timestamp_ns,
                        qubit_count: frame.header.qubit_count,
                        ancilla_count: frame.header.ancilla_count,
                        detector_event_count,
                        meas_results,
                    }))
                }
                None => Ok(None),
            }
        })
    }
}

struct InnerHandle {
    runtime: Runtime,
    source: Box<dyn FrameProducer>,
}

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

    let runtime = match Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return std::ptr::null_mut(),
    };

    let config = StreamConfig {
        validation: ValidationPolicy::Disabled,
        ..Default::default()
    };

    let producer: Box<dyn FrameProducer> = if source_str.starts_with("tcp://") {
        let addr = source_str.trim_start_matches("tcp://").to_owned();
        let tcp = match runtime.block_on(tokio::net::TcpStream::connect(addr)) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        Box::new(QssfProducer {
            stream: QssfStream::new(tcp, config),
        })
    } else {
        let path = source_str.to_owned();
        let file = match runtime.block_on(tokio::fs::File::open(path)) {
            Ok(f) => f,
            Err(_) => return std::ptr::null_mut(),
        };
        let reader = BufReader::new(file);
        Box::new(QssfProducer {
            stream: QssfStream::new(reader, config),
        })
    };

    let inner = Box::new(InnerHandle {
        runtime,
        source: producer,
    });
    Box::into_raw(inner) as *mut StabstreamHandle
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
/// `handle` must be a live pointer obtained from [`stabstream_open`] that has
/// not previously been passed to this function.
#[no_mangle]
pub unsafe extern "C" fn stabstream_close(handle: *mut StabstreamHandle) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle as *mut InnerHandle) });
    }
}

/// Return the library version string as a null-terminated C string.
///
/// The returned pointer points to a static string and must not be freed.
#[no_mangle]
pub extern "C" fn stabstream_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
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
            // Verify frame_id increases monotonically.
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
}
