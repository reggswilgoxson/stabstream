use std::ffi::c_char;

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

/// Open a QSSF source. `source` may be a TCP URI (`tcp://host:port`) or a
/// path to a `.qssf.gz` file.
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
    // TODO: parse source string, open stream, box a handle, return raw pointer
    std::ptr::null_mut()
}

/// Read the next syndrome frame from `handle` into `out_buf` (up to `buf_len` bytes).
///
/// Returns the number of bytes written, or a negative [`StabstreamStatus`] on error.
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
    // TODO: drive the async runtime, parse next frame, write bytes to out_buf
    -(StabstreamStatus::EndOfStream as i64)
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
        // SAFETY: caller guarantees handle is a valid Box<StabstreamHandle>.
        // TODO: drop(unsafe { Box::from_raw(handle) });
    }
}

/// Return the library version string as a null-terminated C string.
///
/// The returned pointer points to a static string and must not be freed.
#[no_mangle]
pub extern "C" fn stabstream_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}
