use std::sync::Arc;

use ndarray::Array2;
use numpy::{IntoPyArray, PyArray1, PyArray2, PyReadonlyArray1};
use pyo3::exceptions::{PyIOError, PyImportError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PySet};
use stabstream_core::{
    code::CodeType,
    error::StabstreamError as CoreError,
    window::{OwnedSyndromeData, SyndromeWindow},
};
use stabstream_decoder::{
    union_find::UnionFindDecoder, Decoder, DecoderResult, LogicalCorrection, PauliOp,
};
use stabstream_dem::{DetectorErrorModel, SpacetimeGraph};
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_metrics::LogicalErrorAccumulator;
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::BufReader;
use tokio::runtime::Runtime;

pyo3::create_exception!(stabstream, StabstreamError, pyo3::exceptions::PyException);

// ---------------------------------------------------------------------------
// PyCodeType
// ---------------------------------------------------------------------------

#[pyclass(name = "CodeType", from_py_object)]
#[derive(Clone)]
pub struct PyCodeType {
    inner: CodeType,
}

#[allow(non_snake_case)]
#[pymethods]
impl PyCodeType {
    #[classattr]
    fn SURFACE_CODE() -> Self {
        Self {
            inner: CodeType::SurfaceCode,
        }
    }
    #[classattr]
    fn HONEYCOMB_CODE() -> Self {
        Self {
            inner: CodeType::HoneycombCode,
        }
    }
    #[classattr]
    fn COLOR_CODE() -> Self {
        Self {
            inner: CodeType::ColorCode,
        }
    }
    #[classattr]
    fn REPETITION_CODE() -> Self {
        Self {
            inner: CodeType::RepetitionCode,
        }
    }
    #[classattr]
    fn TORIC_CODE() -> Self {
        Self {
            inner: CodeType::ToricCode,
        }
    }
    #[classattr]
    fn BIVARIATE_BICYCLE() -> Self {
        Self {
            inner: CodeType::BivariateBicycle,
        }
    }
    #[classattr]
    fn HYPERGRAPH_PRODUCT() -> Self {
        Self {
            inner: CodeType::HypergraphProduct,
        }
    }
    #[classattr]
    fn FIBER_BUNDLE() -> Self {
        Self {
            inner: CodeType::FiberBundle,
        }
    }
    #[classattr]
    fn CUSTOM() -> Self {
        Self {
            inner: CodeType::Custom,
        }
    }

    fn __repr__(&self) -> String {
        format!("CodeType.{:?}", self.inner)
    }
}

// ---------------------------------------------------------------------------
// PyLogicalCorrection / PyDecoderResult
// ---------------------------------------------------------------------------

#[pyclass(name = "LogicalCorrection", from_py_object)]
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
        format!(
            "LogicalCorrection(logical_id={}, pauli={})",
            self.logical_id, self.pauli
        )
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
        Self {
            logical_id: lc.logical_id,
            pauli,
        }
    }
}

#[pyclass(name = "DecoderResult")]
pub struct PyDecoderResult {
    #[pyo3(get)]
    pub corrections: Vec<PyLogicalCorrection>,
    #[pyo3(get)]
    pub confidence: f64,
    #[pyo3(get)]
    pub observable_flips: u64,
}

#[pymethods]
impl PyDecoderResult {
    /// Construct a ``DecoderResult`` directly from an observable-flip bitmask.
    ///
    /// This is the canonical way to bridge Python decoder adapters
    /// (PyMatchingDecoder, ChromobiusDecoder, TesseractDecoder — which return
    /// plain dicts) into the stabstream ``LogicalErrorAccumulator``.
    ///
    /// Parameters
    /// ----------
    /// observable_flips : int
    ///     Bitmask of predicted logical observable flips.
    /// confidence : float, optional
    ///     Decoder confidence in [0, 1]. Hard-decision decoders use 1.0 (default).
    #[new]
    #[pyo3(signature = (observable_flips, confidence=1.0))]
    pub fn new(observable_flips: u64, confidence: f64) -> Self {
        Self {
            corrections: Vec::new(),
            confidence,
            observable_flips,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DecoderResult(corrections={}, confidence={:.4}, observable_flips={:#b})",
            self.corrections.len(),
            self.confidence,
            self.observable_flips,
        )
    }
}

impl PyDecoderResult {
    fn from_rust(r: DecoderResult) -> Self {
        let obs = r.observable_flips;
        Self {
            corrections: r
                .corrections
                .iter()
                .map(PyLogicalCorrection::from_rust)
                .collect(),
            confidence: r.confidence,
            observable_flips: obs,
        }
    }
}

// ---------------------------------------------------------------------------
// PyLogicalErrorAccumulator
// ---------------------------------------------------------------------------

#[pyclass(name = "LogicalErrorAccumulator")]
pub struct PyLogicalErrorAccumulator {
    inner: LogicalErrorAccumulator,
}

#[pymethods]
impl PyLogicalErrorAccumulator {
    #[new]
    fn new(observable_count: usize) -> Self {
        Self {
            inner: LogicalErrorAccumulator::new(observable_count),
        }
    }

    fn record(&self, result: &PyDecoderResult, ground_truth: u64) {
        let rust_result = DecoderResult {
            corrections: Vec::new(),
            confidence: result.confidence,
            observable_flips: result.observable_flips,
        };
        self.inner.record(&rust_result, ground_truth);
    }

    fn logical_error_rate(&self, observable: usize) -> f64 {
        self.inner.logical_error_rate(observable)
    }

    fn mean_logical_error_rate(&self) -> f64 {
        self.inner.mean_logical_error_rate()
    }

    fn total_shots(&self) -> u64 {
        self.inner.total_shots()
    }

    fn reset(&self) {
        self.inner.reset();
    }

    fn __repr__(&self) -> String {
        format!(
            "LogicalErrorAccumulator(shots={}, mean_p_L={:.4e})",
            self.inner.total_shots(),
            self.inner.mean_logical_error_rate(),
        )
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
    /// Decoded detector events (one bool per ancilla).
    detector_events: Vec<bool>,
    observable_flips: Option<u64>,
}

/// Decode QSSF RLE detector events to a flat bool vector.
///
/// Token: bit 7 = mode (0=zeros, 1=ones), bits 0–6 = run length.
fn decode_rle_events(rle: &[u8], ancilla_count: usize) -> Vec<bool> {
    let mut out = Vec::with_capacity(ancilla_count);
    for &tok in rle {
        let mode = (tok & 0x80) != 0;
        let run = (tok & 0x7F) as usize;
        for _ in 0..run {
            out.push(mode);
        }
    }
    out.resize(ancilla_count, false);
    out
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
    /// Decoded detector events (one bool per ancilla, len == ancilla_count).
    detector_events: Vec<bool>,
    observable_flips: Option<u64>,
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
    fn qubit_count(&self) -> u16 {
        self.qubit_count
    }
    #[getter]
    fn ancilla_count(&self) -> u16 {
        self.ancilla_count
    }
    #[getter]
    fn detector_event_count(&self) -> u32 {
        self.detector_event_count
    }
    #[getter]
    fn code_type(&self) -> u8 {
        self.code_type
    }
    #[getter]
    fn distance(&self) -> u8 {
        self.distance
    }
    #[getter]
    fn observable_flips(&self) -> PyResult<u64> {
        self.observable_flips.ok_or_else(|| {
            StabstreamError::new_err(
                "No decoder configured — call stream.set_decoder() or use stabstream.open(..., decoder=...) before reading observable_flips",
            )
        })
    }

    fn meas_results<'py>(&self, py: Python<'py>) -> Bound<'py, PyBytes> {
        PyBytes::new(py, &self.meas_results_raw)
    }

    /// Detector events as a 1-D NumPy bool array of shape `(ancilla_count,)`.
    fn to_numpy_detector_events<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<bool>> {
        self.detector_events.clone().into_pyarray(py)
    }

    /// Ancilla measurement results as a 1-D NumPy int8 array of shape `(ancilla_count,)`.
    fn to_numpy_meas_results<'py>(&self, py: Python<'py>) -> Bound<'py, PyArray1<i8>> {
        let data: Vec<i8> = self.meas_results_raw.iter().map(|&v| v as i8).collect();
        data.into_pyarray(py)
    }

    fn null_decode(&self) -> PyDecoderResult {
        PyDecoderResult::from_rust(DecoderResult::empty())
    }

    /// Serialise this frame as a Python dict suitable for pandas / JSON.
    ///
    /// The `detector_events` key holds a 1-D NumPy bool array. All scalar
    /// fields are plain Python ints/floats.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, pyo3::types::PyDict>> {
        let d = pyo3::types::PyDict::new(py);
        d.set_item("frame_id", self.frame_id)?;
        d.set_item("round", self.round)?;
        d.set_item("timestamp_ns", self.timestamp_ns)?;
        d.set_item("qubit_count", self.qubit_count)?;
        d.set_item("ancilla_count", self.ancilla_count)?;
        d.set_item("detector_event_count", self.detector_event_count)?;
        d.set_item("code_type", self.code_type)?;
        d.set_item("distance", self.distance)?;
        d.set_item("detector_events", self.to_numpy_detector_events(py))?;
        d.set_item("observable_flips", self.observable_flips)?;
        Ok(d)
    }

    fn __repr__(&self) -> String {
        format!(
            "SyndromeFrame(frame_id={}, round={}, detector_events={}/{})",
            self.frame_id, self.round, self.detector_event_count, self.ancilla_count,
        )
    }
}

// ---------------------------------------------------------------------------
// PySyndromeWindow
// ---------------------------------------------------------------------------

#[pyclass(name = "SyndromeWindow")]
pub struct PySyndromeWindow {
    inner: SyndromeWindow,
}

#[pymethods]
impl PySyndromeWindow {
    /// Create an empty window.
    ///
    /// Parameters
    /// ----------
    /// ancilla_count : int
    ///     Number of ancilla qubits (columns in the detector matrix).
    /// window_depth : int
    ///     Maximum number of rounds to retain before the oldest is evicted.
    #[new]
    fn new(ancilla_count: usize, window_depth: usize) -> Self {
        Self {
            inner: SyndromeWindow::new(ancilla_count, window_depth),
        }
    }

    /// Push a `SyndromeFrame` into the window, evicting the oldest if full.
    fn push(&mut self, frame: &PySyndromeFrame) {
        let data = OwnedSyndromeData {
            frame_id: frame.frame_id,
            round: frame.round,
            timestamp_ns: frame.timestamp_ns,
            detector_events: frame.detector_events.clone(),
            meas_results: frame.meas_results_raw.iter().map(|&v| v as i8).collect(),
        };
        self.inner.push_owned(data);
    }

    /// Number of rounds currently held in the window.
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn is_full(&self) -> bool {
        self.inner.is_full()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Detector matrix as a 2-D NumPy bool array of shape `(rounds, ancilla_count)`.
    ///
    /// Row 0 is the oldest round; row `len()-1` is the newest.
    fn to_numpy_matrix<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyArray2<bool>>> {
        let rounds = self.inner.len();
        let ancillas = self.inner.ancilla_count;
        if rounds == 0 {
            let arr = Array2::<bool>::default((0, ancillas));
            return Ok(PyArray2::from_owned_array(py, arr));
        }
        let flat = self.inner.detector_matrix().to_vec();
        let arr = Array2::from_shape_vec((rounds, ancillas), flat)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(PyArray2::from_owned_array(py, arr))
    }

    /// Push detector events directly from a 1-D NumPy bool array.
    ///
    /// Use this when building frames from vendor adapters (IBM, Cirq, etc.)
    /// instead of constructing a full `SyndromeFrame`.
    ///
    /// Parameters
    /// ----------
    /// detector_events : np.ndarray[bool]
    ///     Shape ``(ancilla_count,)``.
    /// frame_id : int, optional
    ///     Monotonic frame counter (default 0).
    /// round : int, optional
    ///     Round index within an experiment (default 0).
    fn push_numpy(
        &mut self,
        detector_events: PyReadonlyArray1<'_, bool>,
        frame_id: u64,
        round: u32,
    ) {
        let events = detector_events.as_slice().unwrap_or(&[]).to_vec();
        self.inner.push_owned(OwnedSyndromeData {
            frame_id,
            round,
            timestamp_ns: 0,
            detector_events: events,
            meas_results: vec![],
        });
    }

    /// Indices (into the flat detector matrix) of all fired detectors across all rounds.
    fn active_detectors(&self) -> Vec<u32> {
        self.inner.active_detectors()
    }

    fn __repr__(&self) -> String {
        format!(
            "SyndromeWindow(rounds={}/{}, ancillas={})",
            self.inner.len(),
            self.inner.window_depth,
            self.inner.ancilla_count,
        )
    }
}

// ---------------------------------------------------------------------------
// PyDetectorErrorModel
// ---------------------------------------------------------------------------

#[pyclass(name = "DetectorErrorModel")]
pub struct PyDetectorErrorModel {
    inner: DetectorErrorModel,
}

#[pymethods]
impl PyDetectorErrorModel {
    /// Parse a Stim DEM from its text representation.
    #[staticmethod]
    fn parse(text: &str) -> PyResult<Self> {
        DetectorErrorModel::parse(text)
            .map(|dem| Self { inner: dem })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Load a Stim DEM from a file path.
    #[staticmethod]
    fn from_file(path: &str) -> PyResult<Self> {
        let text = std::fs::read_to_string(path).map_err(|e| PyIOError::new_err(e.to_string()))?;
        DetectorErrorModel::parse(&text)
            .map(|dem| Self { inner: dem })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[getter]
    fn detector_count(&self) -> usize {
        self.inner.detector_count
    }

    #[getter]
    fn observable_count(&self) -> usize {
        self.inner.observable_count
    }

    #[getter]
    fn error_count(&self) -> usize {
        self.inner.errors.len()
    }

    /// Build a `pymatching.Matching` object from this DEM.
    ///
    /// Requires `pymatching` to be installed (`pip install pymatching`).
    /// Edges are added with weight `-ln(p/(1-p))` and `fault_ids` identifying
    /// which observables flip when the error fires.
    fn to_pymatching(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let graph = SpacetimeGraph::from_dem(&self.inner);

        let pymatching = py.import("pymatching").map_err(|_| {
            PyImportError::new_err("pymatching not found — install it with: pip install pymatching")
        })?;

        let matching = pymatching.call_method0("Matching")?;

        let boundary = graph.boundary_node;

        // Collect boundary node indices into a Python set
        let boundary_set = PySet::new(py, [boundary])?;

        for edge in &graph.edges {
            let u = edge.u as usize;
            let v = edge.v as usize;
            let weight = edge.weight as f64;
            let fault_ids: Vec<usize> = edge.fault_ids.iter().map(|&id| id as usize).collect();

            let kwargs = pyo3::types::PyDict::new(py);
            kwargs.set_item("weight", weight)?;
            if !fault_ids.is_empty() {
                kwargs.set_item("fault_ids", fault_ids)?;
            }
            matching.call_method("add_edge", (u, v), Some(&kwargs))?;
        }

        // Mark the boundary node
        matching.call_method1("set_boundary_nodes", (boundary_set,))?;

        Ok(matching.into())
    }

    /// Serialise this DEM as a `HardwareSchema`-compatible JSON string.
    fn to_schema_json(&self, name: &str) -> PyResult<String> {
        let schema = stabstream_dem::schema_gen::schema_from_dem(&self.inner, name);
        serde_json::to_string_pretty(&schema).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "DetectorErrorModel(detectors={}, observables={}, errors={})",
            self.inner.detector_count,
            self.inner.observable_count,
            self.inner.errors.len(),
        )
    }
}

// ---------------------------------------------------------------------------
// Frame producer trait (file vs TCP)
// ---------------------------------------------------------------------------

trait FrameSource: Send + Sync {
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, CoreError>;
}

struct FileSource {
    stream: QssfStream<BufReader<tokio::fs::File>>,
}

impl FrameSource for FileSource {
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, CoreError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(f) => {
                    let ancilla_count = f.header.ancilla_count as usize;
                    let det = f.detector_event_count();
                    let meas = f.payload.meas_results.iter().map(|&v| v as u8).collect();
                    let events = decode_rle_events(f.payload.detector_events, ancilla_count);
                    let obs_flips = f.metadata.as_ref().and_then(|m| m.observable_flips);
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
                        detector_events: events,
                        observable_flips: obs_flips,
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
    fn next_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, CoreError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(f) => {
                    let ancilla_count = f.header.ancilla_count as usize;
                    let det = f.detector_event_count();
                    let meas = f.payload.meas_results.iter().map(|&v| v as u8).collect();
                    let events = decode_rle_events(f.payload.detector_events, ancilla_count);
                    let obs_flips = f.metadata.as_ref().and_then(|m| m.observable_flips);
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
                        detector_events: events,
                        observable_flips: obs_flips,
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
    decoder: Option<UnionFindDecoder>,
    window: Option<SyndromeWindow>,
    window_depth_hint: usize,
    dem_detector_count: usize,
}

fn open_source(source_str: &str, rt: &Runtime) -> Result<Box<dyn FrameSource>, CoreError> {
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

fn extract_dem_text(dem: &Bound<'_, PyAny>) -> PyResult<String> {
    if let Ok(s) = dem.extract::<String>() {
        if std::path::Path::new(&s).exists() {
            return std::fs::read_to_string(&s).map_err(|e| PyIOError::new_err(e.to_string()));
        }
        return Ok(s);
    }
    if dem.hasattr("__fspath__")? {
        let path: String = dem.call_method0("__fspath__")?.extract()?;
        return std::fs::read_to_string(&path).map_err(|e| PyIOError::new_err(e.to_string()));
    }
    // stim.DetectorErrorModel and anything else: str() gives DEM text
    dem.str()?.extract::<String>()
}

#[pymethods]
impl PyStabstreamStream {
    #[new]
    fn new(source: &str) -> PyResult<Self> {
        if source.starts_with("shm://") {
            return Err(PyValueError::new_err(
                "shm:// transport is not available in the Python bindings; \
                 use the C FFI stabstream_shm_open for SHM access",
            ));
        }
        let rt = Runtime::new().map_err(|e| PyIOError::new_err(e.to_string()))?;
        let src = open_source(source, &rt).map_err(|e| PyIOError::new_err(e.to_string()))?;
        Ok(Self {
            runtime: Some(rt),
            source: Some(src),
            decoder: None,
            window: None,
            window_depth_hint: 0,
            dem_detector_count: 0,
        })
    }

    /// Configure the Union-Find decoder from a DEM.
    ///
    /// Parameters
    /// ----------
    /// dem:
    ///     Accepts a file path (str or pathlib.Path), an inline DEM text string,
    ///     or a ``stim.DetectorErrorModel`` object (``str(dem)`` gives the text).
    /// window_depth:
    ///     Number of syndrome rounds per decode window. 0 = auto-infer from
    ///     ``detector_count / ancilla_count`` (default).
    #[pyo3(signature = (dem, window_depth = 0))]
    fn set_decoder(
        &mut self,
        _py: Python<'_>,
        dem: Bound<'_, PyAny>,
        window_depth: usize,
    ) -> PyResult<()> {
        let text = extract_dem_text(&dem)?;
        let parsed =
            DetectorErrorModel::parse(&text).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let detector_count = parsed.detector_count;
        let graph = Arc::new(SpacetimeGraph::from_dem(&parsed));
        let uf = UnionFindDecoder::new(graph);
        self.decoder = Some(uf);
        self.dem_detector_count = detector_count;
        self.window_depth_hint = window_depth;
        self.window = None;
        Ok(())
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> PyResult<Option<PySyndromeFrame>> {
        if slf.runtime.is_none() || slf.source.is_none() {
            return Err(PyIOError::new_err("stream is closed"));
        }
        // SAFETY: both fields are Some (checked above); raw pointer lets us
        // borrow source mutably while holding an immutable ref to runtime.
        let rt_ptr = slf.runtime.as_ref().unwrap() as *const Runtime;
        let src = slf.source.as_mut().unwrap();
        let rt_ref = unsafe { &*rt_ptr };

        let raw = match src.next_owned(rt_ref) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(e) => return Err(PyIOError::new_err(e.to_string())),
        };

        // Lazily initialise syndrome window on first frame after set_decoder.
        if slf.decoder.is_some() {
            let ancilla_count = raw.ancilla_count as usize;
            if slf.window.is_none() {
                let depth = if slf.window_depth_hint > 0 {
                    slf.window_depth_hint
                } else {
                    (slf.dem_detector_count / ancilla_count.max(1)).max(1)
                };
                slf.window = Some(SyndromeWindow::new(ancilla_count, depth));
            }
            let window = slf.window.as_mut().unwrap();
            window.push_owned(OwnedSyndromeData {
                frame_id: raw.frame_id,
                round: raw.round,
                timestamp_ns: raw.timestamp_ns,
                detector_events: raw.detector_events.clone(),
                meas_results: raw.meas_results.iter().map(|&v| v as i8).collect(),
            });
        }

        let observable_flips = if let (Some(decoder), Some(window)) = (&slf.decoder, &slf.window) {
            Some(decoder.decode_window(window).observable_flips)
        } else {
            // No decoder: use simulator ground-truth TLV tag (may be None for real hardware).
            raw.observable_flips
        };

        Ok(Some(PySyndromeFrame {
            frame_id: raw.frame_id,
            round: raw.round,
            timestamp_ns: raw.timestamp_ns,
            qubit_count: raw.qubit_count,
            ancilla_count: raw.ancilla_count,
            detector_event_count: raw.detector_event_count,
            code_type: raw.code_type,
            distance: raw.distance,
            meas_results_raw: raw.meas_results,
            detector_events: raw.detector_events,
            observable_flips,
        }))
    }

    fn close(mut slf: PyRefMut<'_, Self>) {
        slf.source.take();
        slf.runtime.take();
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __exit__(&mut self, _exc_type: Py<PyAny>, _exc_val: Py<PyAny>, _tb: Py<PyAny>) -> bool {
        self.source.take();
        self.runtime.take();
        false
    }
}

// ---------------------------------------------------------------------------
// SpacetimeGraph Python wrapper (read-only inspection)
// ---------------------------------------------------------------------------

#[pyclass(name = "SpacetimeGraph")]
pub struct PySpacetimeGraph {
    inner: Arc<SpacetimeGraph>,
}

#[pymethods]
impl PySpacetimeGraph {
    #[getter]
    fn node_count(&self) -> usize {
        self.inner.nodes.len()
    }

    #[getter]
    fn edge_count(&self) -> usize {
        self.inner.edges.len()
    }

    #[getter]
    fn boundary_node(&self) -> usize {
        self.inner.boundary_node
    }

    fn __repr__(&self) -> String {
        format!(
            "SpacetimeGraph(nodes={}, edges={}, boundary={})",
            self.inner.nodes.len(),
            self.inner.edges.len(),
            self.inner.boundary_node,
        )
    }
}

// ---------------------------------------------------------------------------
// Module
// ---------------------------------------------------------------------------

#[pymodule]
#[pyo3(name = "_stabstream")]
fn stabstream_module(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PySyndromeFrame>()?;
    m.add_class::<PySyndromeWindow>()?;
    m.add_class::<PyStabstreamStream>()?;
    m.add_class::<PyCodeType>()?;
    m.add_class::<PyDecoderResult>()?;
    m.add_class::<PyLogicalCorrection>()?;
    m.add_class::<PyDetectorErrorModel>()?;
    m.add_class::<PySpacetimeGraph>()?;
    m.add_class::<PyLogicalErrorAccumulator>()?;
    m.add("StabstreamError", py.get_type::<StabstreamError>())?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
