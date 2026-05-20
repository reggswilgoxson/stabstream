#![no_main]

use libfuzzer_sys::fuzz_target;
use stabstream_dem::DetectorErrorModel;

// Feed arbitrary UTF-8 strings to the DEM text parser.
// The parser must never panic — it may return Ok or Err.
fuzz_target!(|data: &str| {
    let _ = DetectorErrorModel::parse(data);
});
