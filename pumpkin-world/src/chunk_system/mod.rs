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

    const STAGES: usize = 22;
    pub const NAMES: [&str; STAGES] = [
        "none", "empty", "biomes", "structure_start", "structure_refs", "noise",
        "surface", "carvers", "features", "lighting", "spawn",
        // sub-slots for drill-down profiling
        "sr:sampler_build", "sr:spread_candidates", "sr:strongholds", "sr:start_compute", "sr:miss_compute",
        "n:end_density", "n:corners", "n:fill",
        "fill:independent", "dep:spline", "fill:lazy_binop",
    ];

    pub const SLOT_SR_SAMPLER: u8 = 11;
    pub const SLOT_SR_SPREAD: u8 = 12;
    pub const SLOT_SR_STRONGHOLD: u8 = 13;
    pub const SLOT_SR_COMPUTE: u8 = 14;
    pub const SLOT_SR_INSERT: u8 = 15;
    pub const SLOT_N_END_DENSITY: u8 = 16;
    pub const SLOT_N_CORNERS: u8 = 17;
    pub const SLOT_N_FILL: u8 = 18;
    pub const SLOT_LEAF_BLENDED: u8 = 19;
    pub const SLOT_LEAF_NOISE: u8 = 20;
    pub const SLOT_LEAF_SHIFTED: u8 = 21;

    /// Times a closure into the given slot.
    pub fn time<R>(slot: u8, f: impl FnOnce() -> R) -> R {
        let _g = Guard::new(slot);
        f()
    }

    static NANOS: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
    static COUNTS: [AtomicU64; STAGES] = [const { AtomicU64::new(0) }; STAGES];
    static ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    /// Turns profiling on (e.g. when a bench harness runs). Off by default so
    /// production pays no clock-call overhead in hot paths.
    pub fn set_enabled(on: bool) {
        ENABLED.store(on, Ordering::Relaxed);
    }

    /// Records elapsed time for its stage on drop, so early returns and
    /// panics inside `advance` are still accounted.
    pub struct Guard {
        pub stage: u8,
        pub start: Option<Instant>,
    }

    impl Guard {
        #[must_use]
        pub fn new(stage: u8) -> Self {
            let start = ENABLED.load(Ordering::Relaxed).then(Instant::now);
            Self { stage, start }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            let Some(start) = self.start else { return };
            let idx = (self.stage as usize).min(STAGES - 1);
            let nanos = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
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
