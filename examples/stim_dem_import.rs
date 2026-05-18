//! Example: Convert a Stim detector error model (`.dem`) file into a QSSF stream.
//!
//! This file is an illustrative standalone example. To run it, add it as an
//! `[[example]]` target in one of the workspace crates (e.g. stabstream-core)
//! and invoke `cargo run --example stim_dem_import -- path/to/model.dem`.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dem_path = args.get(1).map(String::as_str).unwrap_or("model.dem");

    println!("Loading Stim DEM from: {dem_path}");

    // TODO:
    // 1. Parse the `.dem` file using the Stim detector error model format.
    // 2. Extract detector count, observable count, and error mechanisms.
    // 3. Map each instruction round to a SyndromeFrame:
    //      - detector_events ← error pattern bitfield for that round
    //      - meas_results   ← synthetic ±1 values derived from error mechanisms
    // 4. Encode frames as QSSF binary and write to stdout or a `.qssf` file.

    todo!("implement Stim DEM → QSSF conversion")
}
