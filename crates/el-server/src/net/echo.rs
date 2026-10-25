//! Echo mode: the transport smoke test. Datagrams and control-stream bytes are sent straight back.

use std::sync::Arc;

use wtransport::Connection;

use super::NetStats;

pub async fn run(conn: Connection, stats: Arc<NetStats>) {
    let streams = conn.clone();
    tokio::spawn(async move {
        while let Ok((mut tx, mut rx)) = streams.accept_bi().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(Some(n)) = rx.read(&mut buf).await {
                    if tx.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    while let Ok(datagram) = conn.receive_datagram().await {
        NetStats::add(&stats.datagrams_in, 1);
        NetStats::add(&stats.bytes_in, datagram.len() as u64);
        if conn.send_datagram(&*datagram).is_ok() {
            NetStats::add(&stats.datagrams_out, 1);
            NetStats::add(&stats.bytes_out, datagram.len() as u64);
        }
    }
}
