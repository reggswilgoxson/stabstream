use pyo3::prelude::*;

/// Python-facing wrapper for a parsed syndrome frame.
#[pyclass(name = "SyndromeFrame")]
pub struct PySyndromeFrame {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    pub detector_event_count: u32,
}

#[pymethods]
impl PySyndromeFrame {
    #[getter]
    fn frame_id(&self) -> u64 {
        self.frame_id
    }

    #[getter]
    fn round(&self) -> u32 {
        self.round
    }

    #[getter]
    fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    #[getter]
    fn detector_event_count(&self) -> u32 {
        self.detector_event_count
    }

    fn __repr__(&self) -> String {
        format!(
            "SyndromeFrame(frame_id={}, round={}, detector_events={})",
            self.frame_id, self.round, self.detector_event_count
        )
    }
}

/// The `stabstream` Python extension module.
#[pymodule]
fn stabstream(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySyndromeFrame>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
