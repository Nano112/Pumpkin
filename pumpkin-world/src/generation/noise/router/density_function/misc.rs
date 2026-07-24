use std::sync::Arc;

use pumpkin_data::noise_router::{ClampedYGradientData, RangeChoiceData};
use pumpkin_util::{
    math::{clamped_map, vector3::Vector3},
    noise::simplex::SimplexNoiseSampler,
    random::{RandomImpl, legacy_rand::LegacyRand},
};

use crate::generation::noise::router::{
    chunk_density_function::ChunkNoiseFunctionSampleOptions,
    chunk_noise_router::{ChunkNoiseFunctionComponent, StaticChunkNoiseFunctionComponentImpl},
};

use super::{
    IndexToNoisePos, NoiseFunctionComponentRange, StaticIndependentChunkNoiseFunctionComponentImpl,
};

pub struct EndIsland {
    sampler: Arc<SimplexNoiseSampler>,
}

impl EndIsland {
    pub fn new(seed: u64) -> Self {
        let mut rand = LegacyRand::from_seed(seed);
        rand.skip(17292);
        Self {
            sampler: Arc::new(SimplexNoiseSampler::new(&mut rand)),
        }
    }

    fn sample_2d(sampler: &SimplexNoiseSampler, x: i32, z: i32) -> f32 {
        let i = x / 2;
        let j = z / 2;
        let k = x % 2;
        let l = z % 2;

        let f = ((x * x + z * z) as f32).sqrt().mul_add(-8.0, 100.0);
        let mut f = f.clamp(-100.0, 80.0);

        for m in -12..=12 {
            for n in -12..=12 {
                let o = (i + m) as i64;
                let p = (j + n) as i64;

                if (o * o + p * p) > 4096 && sampler.sample_2d(o as f64, p as f64) < -0.9 {
                    let g = (o as f32).abs().mul_add(3439.0, (p as f32).abs() * 147.0) % 13.0 + 9.0;
                    let h = (k - m * 2) as f32;
                    let q = (l - n * 2) as f32;
                    let r = h.hypot(q).mul_add(-g, 100.0);
                    let s = r.clamp(-100.0, 80.0);

                    f = f.max(s);
                }
            }
        }

        f
    }
}

// These values are hardcoded from java
impl NoiseFunctionComponentRange for EndIsland {
    #[inline]
    fn min(&self) -> f64 {
        -0.84375
    }

    #[inline]
    fn max(&self) -> f64 {
        0.5625
    }
}

impl StaticIndependentChunkNoiseFunctionComponentImpl for EndIsland {
    fn sample(&self, pos: &Vector3<i32>) -> f64 {
        (Self::sample_2d(&self.sampler, pos.x / 8, pos.z / 8) as f64 - 8.0) / 128.0
    }
}

pub struct IntervalSelect {
    pub input_index: usize,
    pub thresholds: &'static [f64],
    pub functions_indices: &'static [usize],
    min_value: f64,
    max_value: f64,
}

impl IntervalSelect {
    pub const fn new(
        input_index: usize,
        thresholds: &'static [f64],
        functions_indices: &'static [usize],
        min_value: f64,
        max_value: f64,
    ) -> Self {
        Self {
            input_index,
            thresholds,
            functions_indices,
            min_value,
            max_value,
        }
    }
}

impl StaticChunkNoiseFunctionComponentImpl for IntervalSelect {
    fn sample(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        pos: &Vector3<i32>,
        sample_options: &ChunkNoiseFunctionSampleOptions,
    ) -> f64 {
        let input_val = ChunkNoiseFunctionComponent::sample_from_stack(
            &mut component_stack[..=self.input_index],
            pos,
            sample_options,
        );

        let mut selected_index = self.thresholds.len();
        for (i, &threshold) in self.thresholds.iter().enumerate() {
            if input_val < threshold {
                selected_index = i;
                break;
            }
        }

        let func_index = self.functions_indices[selected_index];
        ChunkNoiseFunctionComponent::sample_from_stack(
            &mut component_stack[..=func_index],
            pos,
            sample_options,
        )
    }

    fn fill(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        array: &mut [f64],
        mapper: &impl IndexToNoisePos,
        sample_options: &mut ChunkNoiseFunctionSampleOptions,
    ) {
        ChunkNoiseFunctionComponent::fill_from_stack(
            &mut component_stack[..=self.input_index],
            array,
            mapper,
            sample_options,
        );

        let select = |input_val: f64| {
            let mut selected_index = self.thresholds.len();
            for (i, &threshold) in self.thresholds.iter().enumerate() {
                if input_val < threshold {
                    selected_index = i;
                    break;
                }
            }
            selected_index
        };

        let bucket_count = self.thresholds.len() + 1;
        let mut counts = vec![0usize; bucket_count];
        for value in array.iter() {
            counts[select(*value)] += 1;
        }

        let mut bufs: Vec<Option<Vec<f64>>> = Vec::with_capacity(bucket_count);
        for (bucket, &count) in counts.iter().enumerate() {
            if super::batch::batch_worthwhile(count, array.len()) {
                let mut buf = super::batch::take(array.len());
                ChunkNoiseFunctionComponent::fill_from_stack(
                    &mut component_stack[..=self.functions_indices[bucket]],
                    &mut buf,
                    mapper,
                    sample_options,
                );
                bufs.push(Some(buf));
            } else {
                bufs.push(None);
            }
        }

        for index in 0..array.len() {
            let bucket = select(array[index]);
            array[index] = if let Some(buf) = &bufs[bucket] {
                buf[index]
            } else {
                let pos = mapper.at(index, Some(sample_options));
                ChunkNoiseFunctionComponent::sample_from_stack(
                    &mut component_stack[..=self.functions_indices[bucket]],
                    &pos,
                    sample_options,
                )
            };
        }
        for buf in bufs.into_iter().flatten() {
            super::batch::give(buf);
        }
    }
}

impl NoiseFunctionComponentRange for IntervalSelect {
    #[inline]
    fn min(&self) -> f64 {
        self.min_value
    }

    #[inline]
    fn max(&self) -> f64 {
        self.max_value
    }
}

pub struct ClampedYGradient {
    data: &'static ClampedYGradientData,
}

impl ClampedYGradient {
    pub const fn new(data: &'static ClampedYGradientData) -> Self {
        Self { data }
    }
}

impl NoiseFunctionComponentRange for ClampedYGradient {
    #[inline]
    fn min(&self) -> f64 {
        self.data.from_value.min(self.data.to_value)
    }

    #[inline]
    fn max(&self) -> f64 {
        self.data.from_value.max(self.data.to_value)
    }
}

impl StaticIndependentChunkNoiseFunctionComponentImpl for ClampedYGradient {
    fn sample(&self, pos: &Vector3<i32>) -> f64 {
        clamped_map(
            pos.y as f64,
            self.data.from_y,
            self.data.to_y,
            self.data.from_value,
            self.data.to_value,
        )
    }
}

pub struct RangeChoice {
    input_index: usize,
    pub(crate) when_in_index: usize,
    pub(crate) when_out_index: usize,
    data: &'static RangeChoiceData,
    min_value: f64,
    max_value: f64,
}

impl RangeChoice {
    pub const fn new(
        input_index: usize,
        when_in_index: usize,
        when_out_index: usize,
        min_value: f64,
        max_value: f64,
        data: &'static RangeChoiceData,
    ) -> Self {
        Self {
            input_index,
            when_in_index,
            when_out_index,
            data,
            min_value,
            max_value,
        }
    }
}

impl NoiseFunctionComponentRange for RangeChoice {
    #[inline]
    fn min(&self) -> f64 {
        self.min_value
    }

    #[inline]
    fn max(&self) -> f64 {
        self.max_value
    }
}

impl StaticChunkNoiseFunctionComponentImpl for RangeChoice {
    fn sample(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        pos: &Vector3<i32>,
        sample_options: &ChunkNoiseFunctionSampleOptions,
    ) -> f64 {
        let input_sample = ChunkNoiseFunctionComponent::sample_from_stack(
            &mut component_stack[..=self.input_index],
            pos,
            sample_options,
        );

        if self.data.min_inclusive <= input_sample && input_sample < self.data.max_exclusive {
            ChunkNoiseFunctionComponent::sample_from_stack(
                &mut component_stack[..=self.when_in_index],
                pos,
                sample_options,
            )
        } else {
            ChunkNoiseFunctionComponent::sample_from_stack(
                &mut component_stack[..=self.when_out_index],
                pos,
                sample_options,
            )
        }
    }

    fn fill(
        &self,
        component_stack: &mut [ChunkNoiseFunctionComponent],
        array: &mut [f64],
        mapper: &impl IndexToNoisePos,
        sample_options: &mut ChunkNoiseFunctionSampleOptions,
    ) {
        ChunkNoiseFunctionComponent::fill_from_stack(
            &mut component_stack[..=self.input_index],
            array,
            mapper,
            sample_options,
        );

        let in_range = |v: f64| self.data.min_inclusive <= v && v < self.data.max_exclusive;
        let in_count = array.iter().filter(|v| in_range(**v)).count();

        let in_buf = super::batch::batch_worthwhile(in_count, array.len()).then(|| {
            let mut buf = super::batch::take(array.len());
            ChunkNoiseFunctionComponent::fill_from_stack(
                &mut component_stack[..=self.when_in_index],
                &mut buf,
                mapper,
                sample_options,
            );
            buf
        });
        let out_buf = super::batch::batch_worthwhile(array.len() - in_count, array.len()).then(|| {
            let mut buf = super::batch::take(array.len());
            ChunkNoiseFunctionComponent::fill_from_stack(
                &mut component_stack[..=self.when_out_index],
                &mut buf,
                mapper,
                sample_options,
            );
            buf
        });

        for index in 0..array.len() {
            let value = array[index];
            let (batched, branch_index) = if in_range(value) {
                (&in_buf, self.when_in_index)
            } else {
                (&out_buf, self.when_out_index)
            };
            array[index] = if let Some(buf) = batched {
                buf[index]
            } else {
                let pos = mapper.at(index, Some(sample_options));
                ChunkNoiseFunctionComponent::sample_from_stack(
                    &mut component_stack[..=branch_index],
                    &pos,
                    sample_options,
                )
            };
        }
        if let Some(buf) = in_buf {
            super::batch::give(buf);
        }
        if let Some(buf) = out_buf {
            super::batch::give(buf);
        }
    }
}
