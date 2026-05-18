use pyo3::exceptions::PyIOError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use stabstream_core::{code::CodeType, error::StabstreamError};
use stabstream_decoder::{DecoderResult, LogicalCorrection, PauliOp};
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::BufReader;
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// PyCodeType
// ---------------------------------------------------------------------------

#[pyclass(name = "CodeType")]
#[derive(Clone)]
pub struct PyCodeType {
    inner: CodeType,
}

#[allow(non_snake_case)]
#[pymethods]
impl PyCodeType {
    #[classattr]
    fn SURFACE_CODE() -> Self { Self { inner: CodeType::SurfaceCode } }
    #[classattr]
    fn HONEYCOMB_CODE() -> Self { Self { inner: CodeType::HoneycombCode } }
    #[classattr]
    fn COLOR_CODE() -> Self { Self { inner: CodeType::ColorCode } }
    #[classattr]
    fn REPETITION_CODE() -> Self { Self { inner: CodeType::RepetitionCode } }
    #[classattr]
    fn TORIC_CODE() -> Self { Self { inner: CodeType::ToricCode } }
    #[classattr]
    fn CUSTOM() -> Self { Self { inner: CodeType::Custom } }

    fn __repr__(&self) -> String {
        format!("CodeType.{:?}", self.inner)
    }
}

// ---------------------------------------------------------------------------
// PyDecoderResult
// ---------------------------------------------------------------------------

#[pyclass(name = "LogicalCorrection")]
#[derive(Clone)]
pub struct PyLogicalCorrection {
    #[pyo3(get)]
    pub logical_id: u8,
    #[pyo3(get)]
    pub pauli: String,
}

#[pymethods]
impl PyLogicalCorrection {
    fn __repr__(&self) -> String {
        format!("LogicalCorrection(logical_id={}, pauli={})", self.logical_id, self.pauli)
    }
}

impl PyLogicalCorrection {
    fn from_rust(lc: &LogicalCorrection) -> Self {
        let pauli = match lc.pauli {
            PauliOp::I => "I",
            PauliOp::X => "X",
            PauliOp::Y => "Y",
            PauliOp::Z => "Z",
        }
        .to_string();
        Self { logical_id: lc.logical_id, pauli }
    }
}

#[pyclass(name = "DecoderResult")]
pub struct PyDecoderResult {
    #[pyo3(get)]
    pub corrections: Vec<PyLogicalCorrection>,
    #[pyo3(get)]
    pub confidence: f64,
}

#[pymethods]
impl PyDecoderResult {
    fn __repr__(&self) -> String {
        format!(
            "DecoderResult(corrections={}, confidence={:.4})",
            self.corrections.len(),
            self.confidence
        )
    }
}

impl PyDecoderResult {
    fn from_rust(r: DecoderResult) -> Self {
        Self {
            corrections: r.corrections.iter().map(PyLogicalCorrection::from_rust).collect(),
            confidence: r.confidence,
        }
    }
}

// ---------------------------------------------------------------------------
// Owned frame (heap-allocated, Python-safe)
// ---------------------------------------------------------------------------

struct OwnedFrame {
    frame_id: u64,
    round: u32,
    timestamp_ns: u64,
    qubit_count: u16,
    ancilla_count: u16,
    detector_event_count: u32,
    code_type: u8,
    distance: u8,
    meas_results: Vec<u8>,
}

// ---------------------------------------------------------------------------
// PySyndromeFrame
// ---------------------------------------------------------------------------

#[pyclass(name = "SyndromeFrame")]
pub struct PySyndromeFrame {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    pub qubit_count: u16,
    pub ancilla_count: u16,
    pub detector_event_count: u32,
    pub code_type: u8,
    pub distance: u8,
    meas_results_raw: Vec<u8>,
}

#[pymethods]
impl PySyndromeFrame {
    #[getter] fn frame_id(&self) -> u64 { self.frame_id }
    #[getter] fn round(&self) -> u32 { self.round }
    #[getter] fn timestamp_ns(&self) -> u64 { self.timestamp_ns }
    #[getter] fn qubit_count(&self) -> u16 { self.qubit_count }
    #[getter] fn ancilla_count(&self) -> u16 { self.ancilla_count }
    #[getter] fn detector_event_count(&self) -> u32 { self.detector_event_count }
    #[getter] fn code_type(&self) -> u8 { self.code_type }
    #[getter] fn distance(&self) -> u8 { self.distance }

    fn meas_results<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new_bound(py, &self.meas_results_raw)
    }

    fn null_decode(&self) -> PyDecoderResult {
        PyDecoderResult::from_rust(DecoderResult::empty())
    }

    fn __repr__(&self) -> String {
        format!(
            "SyndromeFrame(frame_id={}, round={}, detector_events={}/{})",
            self.frame_id, self.round, self.detector_event_count, self.ancilla_count,
        )
    }
}

// ---------------------------------------------------------------------------
// Frame producer trait (file vs TCP)
// ---------------------------------------------------------------------------

trait FrameSource: Send {
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError>;
}

struct FileSource {
    stream: QssfStream<BufReader<tokio::fs::File>>,
}

impl FrameSource for FileSource {
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(f) => {
                    let det = f.detector_event_count();
                    let meas = f.payload.meas_results.iter().map(|&v| v as u8).collect();
                    Ok(Some(OwnedFrame {
                        frame_id: f.header.frame_id,
                        round: f.header.round,
                        timestamp_ns: f.header.timestamp_ns,
                        qubit_count: f.header.qubit_count,
                        ancilla_count: f.header.ancilla_count,
                        detector_event_count: det,
                        code_type: f.header.code_type,
                        distance: f.header.distance,
                        meas_results: meas,
                    }))
                }
                None => Ok(None),
            }
        })
    }
}

struct TcpSource {
    stream: QssfStream<tokio::net::TcpStream>,
}

impl FrameSource for TcpSource {
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(f) => {
                    let det = f.detector_event_count();
                    let meas = f.payload.meas_results.iter().map(|&v| v as u8).collect();
                    Ok(Some(OwnedFrame {
                        frame_id: f.header.frame_id,
                        round: f.header.round,
                        timestamp_ns: f.header.timestamp_ns,
                        qubit_count: f.header.qubit_count,
                        ancilla_count: f.header.ancilla_count,
                        detector_event_count: det,
                        code_type: f.header.code_type,
                        distance: f.header.distance,
                        meas_results: meas,
                    }))
                }
                None => Ok(None),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// StabstreamStream
// ---------------------------------------------------------------------------

#[pyclass(name = "StabstreamStream")]
pub struct PyStabstreamStream {
    runtime: Option<Runtime>,
    source: Option<Box<dyn FrameSource>>,
}

fn open_source(
    source_str: &str,
    rt: &Runtime,
) -> Result<Box<dyn FrameSource>, StabstreamError> {
    let config = StreamConfig {
        validation: ValidationPolicy::Disabled,
        ..Default::default()
    };
    if source_str.starts_with("tcp://") {
        let addr = source_str.trim_start_matches("tcp://").to_owned();
        let tcp = rt.block_on(tokio::net::TcpStream::connect(addr))?;
        Ok(Box::new(TcpSource {
            stream: QssfStream::new(tcp, config),
        }))
    } else {
        let path = source_str.to_owned();
        let file = rt.block_on(tokio::fs::File::open(path))?;
        let reader = BufReader::new(file);
        Ok(Box::new(FileSource {
            stream: QssfStream::new(reader, config),
        }))
    }
}

#[pymethods]
impl PyStabstreamStream {
    #[new]
    fn new(source: &str) -> PyResult<Self> {
        let rt = Runtime::new().map_err(|e| PyIOError::new_err(e.to_string()))?;
        let src = open_source(source, &rt).map_err(|e| PyIOError::new_err(e.to_string()))?;
        Ok(Self {
            runtime: Some(rt),
            source: Some(src),
        })
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> { slf }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PySyndromeFrame>> {
        if slf.runtime.is_none() || slf.source.is_none() {
            return Err(PyIOError::new_err("stream is closed"));
        }
        // SAFETY: both fields are Some (checked above); we take a raw pointer to
        // runtime so we can simultaneously borrow source mutably. The runtime is
        // heap-allocated and stable for the lifetime of the PyRefMut borrow.
        let rt_ptr = slf.runtime.as_ref().unwrap() as *const Runtime;
        let src = slf.source.as_mut().unwrap();
        let rt_ref = unsafe { &*rt_ptr };

        match src.next_owned(rt_ref) {
            Ok(Some(f)) => Ok(Some(PySyndromeFrame {
                frame_id: f.frame_id,
                round: f.round,
                timestamp_ns: f.timestamp_ns,
                qubit_count: f.qubit_count,
                ancilla_count: f.ancilla_count,
                detector_event_count: f.detector_event_count,
                code_type: f.code_type,
                distance: f.distance,
                meas_results_raw: f.meas_results,
            })),
            Ok(None) => Ok(None),
            Err(e) => Err(PyIOError::new_err(e.to_string())),
        }
    }

    fn close(mut slf: PyRefMut<'_, Self>) {
        slf.source.take();
        slf.runtime.take();
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> { slf }
    fn __exit__(&mut self, _exc_type: PyObject, _exc_val: PyObject, _tb: PyObject) -> bool {
        self.source.take();
        self.runtime.take();
        false
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

#[pymodule]
fn stabstream(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySyndromeFrame>()?;
    m.add_class::<PyStabstreamStream>()?;
    m.add_class::<PyCodeType>()?;
    m.add_class::<PyDecoderResult>()?;
    m.add_class::<PyLogicalCorrection>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
