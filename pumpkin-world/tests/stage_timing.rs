//! lantern: native per-stage generation timing, for comparison against the
//! wasm bench (`?bench=N` in the lantern console). Run with:
//! `cargo test --release -p pumpkin-world --test stage_timing -- --nocapture --ignored`
#![allow(clippy::pedantic, clippy::nursery, clippy::all)]

use pumpkin_config::lighting::LightingEngineConfig;
use pumpkin_data::BlockStateId;
use pumpkin_data::dimension::Dimension;
use pumpkin_util::world_seed::Seed;
use pumpkin_world::chunk_system::{StagedChunkEnum, gen_timing, generate_single_chunk};
use pumpkin_world::generation::get_world_gen;
use pumpkin_world::world::WorldPortalExt;

struct BlockRegistry;

impl WorldPortalExt for BlockRegistry {
    fn can_place_at(
        &self,
        _block: &pumpkin_data::Block,
        _state: &pumpkin_data::BlockState,
        _block_accessor: &dyn pumpkin_world::world::BlockAccessor,
        _block_pos: &pumpkin_util::math::position::BlockPos,
    ) -> bool {
        true
    }

    fn mirror(
        &self,
        block: &pumpkin_data::Block,
        state_id: BlockStateId,
        mirror: pumpkin_data::Mirror,
    ) -> &'static pumpkin_data::BlockState {
        block.mirror(state_id, mirror)
    }

    fn rotate(
        &self,
        block: &pumpkin_data::Block,
        state_id: BlockStateId,
        rotation: pumpkin_data::Rotation,
    ) -> &'static pumpkin_data::BlockState {
        block.rotate(state_id, rotation)
    }

    fn spawn_mobs_for_chunk_generation(
        &self,
        _cache: &mut dyn pumpkin_world::generation::proto_chunk::GenerationCache,
        _biome: &'static pumpkin_data::chunk::Biome,
        _chunk_x: i32,
        _chunk_z: i32,
    ) {
    }
}

#[test]
#[ignore = "profiling harness, run explicitly with --ignored"]
fn stage_timing() {
    let _ = LightingEngineConfig::default();
    let world_gen = get_world_gen(Seed(42), Dimension::OVERWORLD, false, Vec::new(), String::new());
    let registry = BlockRegistry;

    let start = std::time::Instant::now();
    let mut count = 0u32;
    for x in 0..3 {
        for z in 0..3 {
            generate_single_chunk(
                &Dimension::OVERWORLD,
                0,
                &world_gen,
                &registry,
                x,
                z,
                StagedChunkEnum::Full,
            );
            count += 1;
        }
    }
    let secs = start.elapsed().as_secs_f64();
    println!("native: {count} chunks in {secs:.1}s ({:.2}/s, radius-amplified)", f64::from(count) / secs);

    let mut snap = gen_timing::snapshot();
    snap.sort_by(|a, b| b.2.total_cmp(&a.2));
    let total: f64 = snap.iter().map(|s| s.2).sum();
    for (name, runs, ms) in snap {
        println!(
            "native:   {name:<20} {ms:>9.1}ms total  {:>7.3}ms/run  x{runs}  ({:.0}%)",
            ms / runs as f64,
            ms / total * 100.0
        );
    }
}
