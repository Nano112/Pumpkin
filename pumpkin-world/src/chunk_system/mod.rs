/*
TODO
1. add proto chunk dirty flag
2. better priority
5. add lifetime to loading ticket
6. solve entity not unload problem
*/

pub type HashMapType<K, V> = rustc_hash::FxHashMap<K, V>;
pub type HashSetType<K> = rustc_hash::FxHashSet<K>;
pub type ChunkPos = pumpkin_util::math::vector2::Vector2<i32>;
pub type ChunkLevel = HashMapType<ChunkPos, i8>;
pub type IOLock = std::sync::Arc<(
    std::sync::Mutex<HashMapType<ChunkPos, u8>>,
    tokio::sync::Notify,
)>;

pub mod channel;
/// lantern: per-generation-stage wall-time accounting for profiling.
pub mod gen_timing {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    const STAGES: usize = 11;
    pub const NAMES: [&str; STAGES] = [
        "none", "empty", "biomes", "structure_start", "structure_refs", "noise",
        "surface", "carvers", "features", "lighting", "spawn",
    ];

    static NANOS: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
    static COUNTS: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];

    /// Records elapsed time for its stage on drop, so early returns and
    /// panics inside `advance` are still accounted.
    pub struct Guard {
        pub stage: u8,
        pub start: Instant,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let idx = (self.stage as usize).min(STAGES - 1);
            let nanos = u64::try_from(self.start.elapsed().as_nanos()).unwrap_or(u64::MAX);
            NANOS[idx].fetch_add(nanos, Ordering::Relaxed);
            COUNTS[idx].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// (stage name, executions, total ms) for every stage that ran.
    #[must_use]
    pub fn snapshot() -> Vec<(&'static str, u64, f64)> {
        (0..STAGES)
            .filter_map(|i| {
                let count = COUNTS[i].load(Ordering::Relaxed);
                if count == 0 {
                    return None;
                }
                let ms = NANOS[i].load(Ordering::Relaxed) as f64 / 1_000_000.0;
                Some((NAMES[i], count, ms))
            })
            .collect()
    }
}

pub mod chunk_holder;
pub mod chunk_listener;
pub mod chunk_loading;
pub mod chunk_state;
pub mod dag;
pub mod generation;
pub mod generation_cache;
pub mod schedule;
pub mod worker_logic;

#[cfg(test)]
mod tests;

pub use channel::LevelChannel;
pub use chunk_holder::ChunkHolder;
pub use chunk_listener::ChunkListener;
pub use chunk_loading::ChunkLoading;
pub use chunk_state::{Chunk, StagedChunkEnum};
pub use dag::DAG;
pub use generation::generate_single_chunk;
pub use generation_cache::Cache;
pub use schedule::GenerationSchedule;
