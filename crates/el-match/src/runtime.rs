//! The shard thread: fixed 30 Hz pacing (sleep, then spin to the deadline), metrics, and the
//! egress wake (an `unpark`, never tokio).

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crate::Shard;
use crate::queues::ShardShared;

/// A running shard thread.
pub struct ShardThread {
    pub shared: Arc<ShardShared>,
    handle: Option<JoinHandle<()>>,
}

impl ShardThread {
    pub fn thread(&self) -> Thread {
        self.handle.as_ref().expect("running until stopped").thread().clone()
    }

    /// Signals the thread to stop and waits for it.
    pub fn stop(mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Runs `shard` on a thread called `name`, paced at the tick rate, waking `egress` after every
/// tick.
#[allow(clippy::disallowed_methods, clippy::disallowed_macros)]
pub fn spawn(mut shard: Shard, egress: Option<Thread>, name: &str) -> std::io::Result<ShardThread> {
    let shared = shard.shared().clone();
    let hot = shard.config().hot_guard;
    let handle = thread::Builder::new().name(name.into()).spawn(move || {
        let period = Duration::from_secs_f64(1.0 / f64::from(el_sim::TICK_HZ));
        let spin_window = Duration::from_micros(1_500);
        let shared = shard.shared().clone();
        let m = &shared.metrics;
        let mut deadline = Instant::now() + period;
        while !shared.stop.load(Ordering::Acquire) {
            let start = Instant::now();
            if let Some(g) = hot {
                g(true);
            }
            shard.tick();
            if let Some(g) = hot {
                g(false);
            }
            m.record_tick(start.elapsed().as_micros() as u64);
            if let Some(t) = &egress {
                t.unpark();
            }
            let now = Instant::now();
            if now > deadline + period * 3 {
                // Badly behind (a debugger, a suspend): start again rather than burst.
                crate::Metrics::add(&m.overruns, 1);
                deadline = now + period;
                continue;
            }
            if now > deadline {
                crate::Metrics::add(&m.overruns, 1);
            }
            if let Some(sleep) = deadline.checked_duration_since(now).and_then(|d| d.checked_sub(spin_window))
            {
                thread::sleep(sleep);
            }
            while Instant::now() < deadline {
                std::hint::spin_loop();
            }
            deadline += period;
        }
    })?;
    Ok(ShardThread { shared, handle: Some(handle) })
}
