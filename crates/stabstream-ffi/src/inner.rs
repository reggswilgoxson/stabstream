use std::sync::atomic::AtomicBool;

use stabstream_core::error::StabstreamError;
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
                    let detector_event_count = frame.detector_event_count();
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
    /// Guards against double-close UB. Set to `true` on first `stabstream_close`.
    pub closed: AtomicBool,
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
        closed: AtomicBool::new(false),
    })
}
