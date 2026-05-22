use std::sync::atomic::AtomicBool;

use stabstream_core::error::StabstreamError;
use stabstream_core::window::{OwnedSyndromeData, SyndromeWindow};
use stabstream_decoder::union_find::UnionFindDecoder;
use stabstream_deserialize::stream::{QssfStream, StreamConfig};
use stabstream_validate::policy::ValidationPolicy;
use tokio::io::{AsyncRead, BufReader};
use tokio::runtime::Runtime;

// ---------------------------------------------------------------------------
// Shared types used by both c_api and python modules
// ---------------------------------------------------------------------------

pub(crate) struct OwnedFrame {
    pub frame_id: u64,
    pub round: u32,
    pub timestamp_ns: u64,
    pub qubit_count: u16,
    pub ancilla_count: u16,
    pub detector_event_count: u32,
    /// Decoded detector events (one bool per ancilla). Used to feed the decode window.
    pub detector_events: Vec<bool>,
    pub meas_results: Vec<u8>,
    /// Observable-flip bitmask from TLV metadata (tag 0x10), if present.
    /// Injected by the simulator as ground truth; absent on real hardware frames.
    pub observable_flips: Option<u64>,
}

pub(crate) trait FrameProducer: Send {
    fn next_frame_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError>;
}

pub(crate) struct QssfProducer<R: AsyncRead + Unpin + Send + 'static> {
    pub stream: QssfStream<R>,
}

impl<R: AsyncRead + Unpin + Send + 'static> FrameProducer for QssfProducer<R> {
    fn next_frame_owned(&mut self, rt: &Runtime) -> Result<Option<OwnedFrame>, StabstreamError> {
        rt.block_on(async {
            match self.stream.next_frame().await? {
                Some(frame) => {
                    let ancilla_count = frame.header.ancilla_count as usize;
                    let detector_event_count = frame.detector_event_count();
                    let detector_events =
                        OwnedSyndromeData::decode_rle(frame.payload.detector_events, ancilla_count);
                    let meas_results = frame
                        .payload
                        .meas_results
                        .iter()
                        .map(|&v| v as u8)
                        .collect();
                    let observable_flips = frame.metadata.as_ref().and_then(|m| m.observable_flips);
                    Ok(Some(OwnedFrame {
                        frame_id: frame.header.frame_id,
                        round: frame.header.round,
                        timestamp_ns: frame.header.timestamp_ns,
                        qubit_count: frame.header.qubit_count,
                        ancilla_count: frame.header.ancilla_count,
                        detector_event_count,
                        detector_events,
                        meas_results,
                        observable_flips,
                    }))
                }
                None => Ok(None),
            }
        })
    }
}

pub(crate) struct InnerHandle {
    pub runtime: Runtime,
    pub source: Box<dyn FrameProducer>,
    /// `observable_flips` from the most recently read frame, for `stabstream_decode_frame`.
    pub last_observable_flips: Option<u64>,
    /// Union-Find decoder, set by `stabstream_set_decoder_dem`.
    pub decoder: Option<UnionFindDecoder>,
    /// Sliding syndrome window fed from `next_frame`. Initialised lazily on the
    /// first frame read after a decoder is configured (ancilla_count comes from
    /// the frame header, not the DEM).
    pub window: Option<SyndromeWindow>,
    /// Explicit window depth; 0 means "infer from dem_detector_count / ancilla_count".
    pub window_depth_hint: usize,
    /// Detector count from the most recently loaded DEM; used for auto-inference.
    pub dem_detector_count: usize,
    /// Guards against double-close UB. Set to `true` on first `stabstream_close`.
    pub closed: AtomicBool,
}

impl InnerHandle {
    /// Push a decoded frame into the syndrome window, initialising it on the
    /// first call if a decoder has been configured.
    pub fn push_frame_to_window(&mut self, frame: &OwnedFrame) {
        if self.decoder.is_none() {
            return;
        }

        let ancilla_count = frame.ancilla_count as usize;

        // Lazy window initialisation: ancilla_count comes from the first frame header.
        if self.window.is_none() {
            let depth = if self.window_depth_hint > 0 {
                self.window_depth_hint
            } else {
                // Auto-infer: DEM detectors ≈ depth × ancilla_count.
                (self.dem_detector_count / ancilla_count.max(1)).max(1)
            };
            self.window = Some(SyndromeWindow::new(ancilla_count, depth));
        }

        let window = self.window.as_mut().unwrap();
        window.push_owned(OwnedSyndromeData {
            frame_id: frame.frame_id,
            round: frame.round,
            timestamp_ns: frame.timestamp_ns,
            detector_events: frame.detector_events.clone(),
            meas_results: frame.meas_results.iter().map(|&b| b as i8).collect(),
        });
    }
}

/// Open a QSSF source by URI and return an [`InnerHandle`].
/// Shared by the C API and the Python bindings.
pub(crate) fn open_inner(source_str: &str) -> Result<InnerHandle, StabstreamError> {
    let runtime = Runtime::new()?;

    let config = StreamConfig {
        validation: ValidationPolicy::Disabled,
        ..Default::default()
    };

    let producer: Box<dyn FrameProducer> = if source_str.starts_with("tcp://") {
        let addr = source_str.trim_start_matches("tcp://").to_owned();
        let tcp = runtime.block_on(tokio::net::TcpStream::connect(addr))?;
        Box::new(QssfProducer {
            stream: QssfStream::new(tcp, config),
        })
    } else {
        let path = source_str.to_owned();
        let file = runtime.block_on(tokio::fs::File::open(path))?;
        let reader = BufReader::new(file);
        Box::new(QssfProducer {
            stream: QssfStream::new(reader, config),
        })
    };

    Ok(InnerHandle {
        runtime,
        source: producer,
        last_observable_flips: None,
        decoder: None,
        window: None,
        window_depth_hint: 0,
        dem_detector_count: 0,
        closed: AtomicBool::new(false),
    })
}
