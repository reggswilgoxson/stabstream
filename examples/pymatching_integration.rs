//! Example: Feed stabstream syndrome frames to PyMatching via the C FFI layer.
//!
//! This file is an illustrative standalone example. It demonstrates how a C or
//! Python host program might use `libstabstream` together with PyMatching.
//!
//! Compile from the workspace root after building the C shared library:
//!   cargo build -p stabstream-ffi --release
//!   rustc examples/pymatching_integration.rs --edition 2021

fn main() {
    println!("stabstream × PyMatching integration example");

    // TODO:
    // 1. Call stabstream_open("tcp://localhost:9000") via the C API.
    // 2. In a loop, call stabstream_next_frame to fill a buffer.
    // 3. Deserialize the buffer into a SyndromeFrame.
    // 4. Build a PyMatching detector graph from the hardware schema.
    // 5. Call PyMatching's decode() with the detector_events bitfield.
    // 6. Collect the correction and apply it to the logical observable.

    todo!("implement PyMatching integration via FFI")
}
