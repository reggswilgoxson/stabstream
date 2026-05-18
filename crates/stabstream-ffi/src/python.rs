use pyo3::exceptions::PyIOError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

use crate::inner::{open_inner, InnerHandle};

/// Python-facing wrapper for a parsed syndrome frame.
#[pyclass(name = "SyndromeFrame")]
pub struct PySyndromeFrame {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    pub ancilla_count: u16,
    pub detector_event_count: u32,
    pub meas_results: Vec<u8>,
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
    fn ancilla_count(&self) -> u16 {
        self.ancilla_count
    }

    #[getter]
    fn detector_event_count(&self) -> u32 {
        self.detector_event_count
    }

    fn meas_results<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.meas_results)
    }

    fn __repr__(&self) -> String {
        format!(
            "SyndromeFrame(frame_id={}, round={}, detector_events={}/{})",
            self.frame_id, self.round, self.detector_event_count, self.ancilla_count
        )
    }
}

/// Async QSSF stream exposed as a Python iterator and context manager.
///
/// Usage:
/// ```python
/// from stabstream import StabstreamStream
///
/// with StabstreamStream("tcp://localhost:9000") as stream:
///     for frame in stream:
///         print(frame.frame_id, frame.detector_event_count)
/// ```
#[pyclass(name = "StabstreamStream")]
pub struct PyStabstreamStream {
    inner: Option<Box<InnerHandle>>,
}

#[pymethods]
impl PyStabstreamStream {
    #[new]
    fn new(source: &str) -> PyResult<Self> {
        open_inner(source)
            .map(|h| Self {
                inner: Some(Box::new(h)),
            })
            .map_err(|e| PyIOError::new_err(e.to_string()))
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PySyndromeFrame>> {
        let inner = slf
            .inner
            .as_mut()
            .ok_or_else(|| PyIOError::new_err("stream is closed"))?;
        match inner.source.next_frame_owned(&inner.runtime) {
            Ok(Some(f)) => Ok(Some(PySyndromeFrame {
                frame_id: f.frame_id,
                round: f.round,
                timestamp_ns: f.timestamp_ns,
                ancilla_count: f.ancilla_count,
                detector_event_count: f.detector_event_count,
                meas_results: f.meas_results,
            })),
            Ok(None) => Ok(None),
            Err(e) => Err(PyIOError::new_err(e.to_string())),
        }
    }

    fn close(mut slf: PyRefMut<'_, Self>) {
        slf.inner.take();
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(&mut self, _exc_type: PyObject, _exc_val: PyObject, _tb: PyObject) -> bool {
        self.inner.take();
        false
    }
}

/// The `stabstream` Python extension module.
#[pymodule]
fn stabstream(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySyndromeFrame>()?;
    m.add_class::<PyStabstreamStream>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
