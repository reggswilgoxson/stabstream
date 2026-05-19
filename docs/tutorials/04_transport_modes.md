# Tutorial 4: Transport Modes

`stabstream-sim` supports three transport modes for serving QSSF syndrome
frames. Choose based on your latency requirements and number of consumers.

## Direct (default)

One TCP connection per shot source. Each client gets its own independent stream.
This is the original behaviour and the right choice when:

- You have a single consumer (dashboard, recorder, decoder)
- You want to serve different numbers of shots to different clients
- Simplicity is more important than multi-client efficiency

```bash
stabstream-sim \
    --simulator native --dem circuit.dem \
    --transport direct \
    --port 9000 --shots 10000
```

**Latency**: ~2–5 µs (TCP stack, loopback).

## Broadcast

One shot source shared across all TCP clients. The producer generates frames
once; every subscriber receives every frame. Clients that fall more than
`--broadcast-capacity` frames behind skip frames (they don't disconnect).

Use broadcast when:
- Multiple consumers need the same stream simultaneously (dashboard + recorder + decoder)
- You want to avoid generating the same shot twice

```bash
stabstream-sim \
    --simulator native --dem circuit.dem \
    --transport broadcast \
    --broadcast-capacity 512 \
    --port 9000 --shots 100000
```

**Latency**: same as direct (~2–5 µs). The broadcast channel itself adds < 1 µs.

### Connecting multiple consumers

```bash
# Consumer 1: dashboard
cargo run -p stabstream-dashboard -- --source tcp://localhost:9000

# Consumer 2: recorder
stabstream-record --source tcp://localhost:9000 --output recording.qssf.zst

# Consumer 3: custom decoder (Python)
python my_decoder.py --host localhost --port 9000
```

## SHM (Shared Memory)

Writes frames directly to a POSIX shared memory ring buffer at
`/dev/shm/<name>`. No TCP server is started. Decoder processes on the same host
read with `ShmConsumer` at ~50–200 ns IPC latency — 10–25× faster than TCP.

Use SHM when:
- The decoder runs on the same host as the simulator
- You need to stay under 1 µs total decode budget
- The hardware syndrome cycle is ≤ 2 µs

```bash
# Producer
stabstream-sim \
    --simulator native --dem circuit.dem \
    --transport shm \
    --shm-name my_experiment \
    --shots 1000000
```

```rust
// Consumer (Rust)
use stabstream_sim::ShmConsumer;

let mut consumer = ShmConsumer::open("my_experiment")?;
loop {
    match consumer.try_read_frame()? {
        Some(frame_bytes) => { /* parse and decode */ }
        None => std::thread::yield_now(),
    }
}
```

**Ring size**: 256 slots × 4096 bytes/slot = ~1 MB at `/dev/shm/my_experiment`.
The producer overwrites the oldest slot when the ring is full; consumers that
fall > 256 frames behind will receive a ring-overrun error.

## Latency comparison

| Transport | Typical IPC latency | Multi-consumer | Hardware decode path |
|-----------|--------------------|--------------|--------------------|
| Direct | 2–5 µs | No (separate stream per client) | No |
| Broadcast | 2–5 µs | Yes (all share one stream) | No |
| SHM | 50–200 ns | Single (ring has one write pointer) | ✅ Yes |

## Choosing between native and Stim simulator

| Mode | When to use |
|------|-------------|
| `--simulator stim` | Circuit-level fidelity, precise error model, requires Stim on PATH |
| `--simulator native` | No Stim dependency, ~10× faster for bulk shot generation |

Native mode requires `--dem <model.dem>`. Stim mode requires `--circuit <circuit.stim>`.
For SHM transport, only native mode is supported (Stim subprocess can't be used with SHM).

## Next steps

- [Tutorial 1: Hello Syndrome](01_hello_syndrome.md)
- [Tutorial 2: Offline Analysis](02_offline_analysis.md)
- [Theory: Decoder Guide](../theory/decoder_guide.md)
