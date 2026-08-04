use super::FlatLayer;
use crate::chunk_system::StagedChunkEnum;
use crate::generation::Seed;
use crate::generation::positions::chunk_pos::{start_block_x, start_block_z};
use crate::generation::proto_chunk::ProtoChunk;
use pumpkin_data::Block;
use pumpkin_data::dimension::Dimension;

/// lantern: pluggable density source — world coords in, block out (None = air).
pub type DensityFn =
    dyn Fn(i32, i32, i32) -> Option<&'static pumpkin_data::BlockState> + Send + Sync;

pub struct FlatGenerator {
    pub seed: u64,
    pub dimension: Dimension,
    pub layers: Vec<FlatLayer>,
    pub biome: String,
    /// When set, step_to_noise fills blocks from this instead of the layers —
    /// the SDF world generator rides the whole flat pipeline.
    pub density: Option<std::sync::Arc<DensityFn>>,
}

impl FlatGenerator {
    #[must_use]
    pub const fn new(
        seed: Seed,
        dimension: Dimension,
        layers: Vec<FlatLayer>,
        biome: String,
    ) -> Self {
        Self {
            seed: seed.0,
            dimension,
            layers,
            biome,
            density: None,
        }
    }

    pub fn step_to_biomes(&self, chunk: &mut ProtoChunk) {
        let clean_biome = self.biome.strip_prefix("minecraft:").unwrap_or(&self.biome);
        let biome_id = pumpkin_data::chunk::Biome::from_name(clean_biome)
            .map_or(pumpkin_data::chunk::Biome::PLAINS.id, |b| b.id);
        chunk.flat_biome_map.fill(biome_id);
        chunk.stage = StagedChunkEnum::Biomes;
    }

    pub fn step_to_noise(&self, chunk: &mut ProtoChunk) {
        let start_x = start_block_x(chunk.x);
        let start_z = start_block_z(chunk.z);
        if let Some(density) = &self.density {
            let bottom = chunk.bottom_y() as i32;
            let top = bottom + chunk.height() as i32;
            for x in 0..16 {
                for z in 0..16 {
                    let (wx, wz) = (start_x + x, start_z + z);
                    for y in bottom..top {
                        if let Some(state) = density(wx, y, wz) {
                            chunk.set_block_state(wx, y, wz, state);
                        }
                    }
                }
            }
            chunk.stage = StagedChunkEnum::Noise;
            return;
        }
        for x in 0..16 {
            for z in 0..16 {
                let mut current_y = chunk.bottom_y() as i32;
                for layer in &self.layers {
                    let block = Block::from_name(&layer.block);
                    let state = block.map_or(Block::AIR.default_state, |b| b.default_state);
                    for _ in 0..layer.height {
                        if current_y < chunk.bottom_y() as i32 + chunk.height() as i32 {
                            chunk.set_block_state(start_x + x, current_y, start_z + z, state);
                            current_y += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        chunk.stage = StagedChunkEnum::Noise;
    }

    pub const fn step_to_surface(&self, chunk: &mut ProtoChunk) {
        chunk.stage = StagedChunkEnum::Surface;
    }

    pub const fn step_to_carvers(&self, chunk: &mut ProtoChunk) {
        chunk.stage = StagedChunkEnum::Carvers;
    }
}
