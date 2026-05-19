use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tokio::sync::broadcast;

/// Default backlog depth for the broadcast channel.
///
/// Dashboard subscribers (display-only) may lag and drop frames; recorder
/// subscribers need a larger buffer or a dedicated per-client queue.
pub const DEFAULT_BROADCAST_CAPACITY: usize = 256;

/// One-to-many QSSF frame broadcaster.
///
/// One producer pushes raw QSSF frame bytes; every subscribed TCP-client
/// relay task receives every frame. Slow consumers that fall more than
/// `capacity` frames behind receive `RecvError::Lagged` — they skip frames
/// but remain connected.
///
/// All methods are `Send + Sync`; the inner `broadcast::Sender` is cloned for
/// the producer task.
pub struct SimBroadcaster {
    tx: broadcast::Sender<Arc<Vec<u8>>>,
    client_count: AtomicUsize,
}

impl SimBroadcaster {
    pub fn new(capacity: usize) -> Arc<Self> {
        let (tx, _) = broadcast::channel(capacity);
        Arc::new(Self {
            tx,
            client_count: AtomicUsize::new(0),
        })
    }

    /// Subscribe a new consumer; call `client_disconnected()` on drop.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Vec<u8>>> {
        self.client_count.fetch_add(1, Ordering::Relaxed);
        self.tx.subscribe()
    }

    /// Decrement client counter. Called by relay tasks on disconnect.
    pub fn client_disconnected(&self) {
        self.client_count.fetch_sub(1, Ordering::Relaxed);
    }

    /// Active subscriber count (approximate, relaxed).
    pub fn client_count(&self) -> usize {
        self.client_count.load(Ordering::Relaxed)
    }

    /// Broadcast one frame. Returns the receiver count (0 if no subscribers).
    pub fn send(&self, frame: Arc<Vec<u8>>) -> usize {
        self.tx.send(frame).unwrap_or_default()
    }

    /// Clone the sender so a `spawn_blocking` producer task can push frames.
    pub fn sender(&self) -> broadcast::Sender<Arc<Vec<u8>>> {
        self.tx.clone()
    }
}

/// Relay QSSF frames from a broadcast channel to a single TCP socket.
///
/// Sends the 26-byte QSSF file header first, then continuously forwards
/// broadcast frames. Lagged frames are skipped with a warning rather than
/// disconnecting the client.
pub async fn relay_to_socket(
    mut socket: tokio::net::TcpStream,
    mut rx: broadcast::Receiver<Arc<Vec<u8>>>,
    file_header: [u8; 26],
    broadcaster: Arc<SimBroadcaster>,
) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    socket.write_all(&file_header).await?;

    loop {
        match rx.recv().await {
            Ok(frame) => {
                if socket.write_all(&frame).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(dropped = n, "broadcast client lagged");
                // continue — client misses frames but stays connected
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }

    broadcaster.client_disconnected();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn broadcaster_delivers_to_multiple_receivers() {
        let bc = SimBroadcaster::new(16);
        let mut rx1 = bc.subscribe();
        let mut rx2 = bc.subscribe();
        assert_eq!(bc.client_count(), 2);

        let frame = Arc::new(vec![0xAAu8, 0xBB, 0xCC]);
        let delivered = bc.send(Arc::clone(&frame));
        assert_eq!(delivered, 2);

        let r1 = rx1.recv().await.unwrap();
        let r2 = rx2.recv().await.unwrap();
        assert_eq!(*r1, *frame);
        assert_eq!(*r2, *frame);
    }

    #[tokio::test]
    async fn send_with_no_subscribers_returns_zero() {
        let bc = SimBroadcaster::new(16);
        let frame = Arc::new(vec![1u8, 2, 3]);
        assert_eq!(bc.send(frame), 0);
    }

    #[tokio::test]
    async fn lagged_receiver_sees_lagged_error() {
        let bc = SimBroadcaster::new(2); // tiny capacity
        let mut rx = bc.subscribe();

        // Send more frames than capacity without consuming
        for i in 0u8..5 {
            bc.send(Arc::new(vec![i]));
        }

        // First recv should return Lagged
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(_)) => {} // expected
            other => panic!("expected Lagged, got {other:?}"),
        }
    }
}
