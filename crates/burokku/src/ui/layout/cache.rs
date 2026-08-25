use taffy::{LayoutInput, LayoutOutput, RunMode};

const MAX_MEASURE_ENTRIES: usize = 16;

#[derive(Clone, Copy, Debug)]
struct CacheEntry {
    input: LayoutInput,
    output: LayoutOutput,
}

/// A bounded exact-input cache that preserves the complete Taffy output.
///
/// Taffy 0.11's built-in `Cache` discards baselines for `ComputeSize` entries.
/// Paragraph baseline alignment must not depend on whether a probe was cached,
/// so Burokku stores the full `LayoutOutput` instead.
#[derive(Clone, Debug, Default)]
pub(super) struct NodeLayoutCache {
    final_layout: Option<CacheEntry>,
    measurements: Vec<CacheEntry>,
    next_eviction: usize,
}

impl NodeLayoutCache {
    pub(super) fn get(&self, input: &LayoutInput) -> Option<LayoutOutput> {
        match input.run_mode {
            RunMode::PerformLayout => self
                .final_layout
                .filter(|entry| entry.input == *input)
                .map(|entry| entry.output),
            RunMode::ComputeSize => self
                .measurements
                .iter()
                .find(|entry| entry.input == *input)
                .map(|entry| entry.output),
            RunMode::PerformHiddenLayout => None,
        }
    }

    pub(super) fn store(&mut self, input: LayoutInput, output: LayoutOutput) {
        match input.run_mode {
            RunMode::PerformLayout => self.final_layout = Some(CacheEntry { input, output }),
            RunMode::ComputeSize => {
                if let Some(entry) = self
                    .measurements
                    .iter_mut()
                    .find(|entry| entry.input == input)
                {
                    entry.output = output;
                    return;
                }

                let entry = CacheEntry { input, output };
                if self.measurements.len() < MAX_MEASURE_ENTRIES {
                    self.measurements.push(entry);
                } else {
                    self.measurements[self.next_eviction] = entry;
                    self.next_eviction = (self.next_eviction + 1) % MAX_MEASURE_ENTRIES;
                }
            }
            RunMode::PerformHiddenLayout => {}
        }
    }

    pub(super) fn clear(&mut self) {
        self.final_layout = None;
        self.measurements.clear();
        self.next_eviction = 0;
    }
}

#[cfg(test)]
mod tests {
    use taffy::{
        geometry::{Line, Point, Size},
        AvailableSpace, RequestedAxis, SizingMode,
    };

    use super::*;

    fn input(run_mode: RunMode, width: AvailableSpace) -> LayoutInput {
        LayoutInput {
            run_mode,
            sizing_mode: SizingMode::InherentSize,
            axis: RequestedAxis::Both,
            known_dimensions: Size::NONE,
            parent_size: Size::NONE,
            available_space: Size {
                width,
                height: AvailableSpace::MaxContent,
            },
            vertical_margins_are_collapsible: Line::FALSE,
        }
    }

    #[test]
    fn compute_size_cache_preserves_baselines() {
        let input = input(RunMode::ComputeSize, AvailableSpace::MaxContent);
        let output = LayoutOutput::from_sizes_and_baselines(
            Size {
                width: 30.0,
                height: 12.0,
            },
            Size {
                width: 30.0,
                height: 12.0,
            },
            Point {
                x: None,
                y: Some(9.0),
            },
        );
        let mut cache = NodeLayoutCache::default();

        cache.store(input, output);

        assert_eq!(cache.get(&input), Some(output));
        assert_eq!(cache.get(&input).unwrap().first_baselines.y, Some(9.0));
    }

    #[test]
    fn complete_inputs_do_not_collide() {
        let first = input(RunMode::ComputeSize, AvailableSpace::Definite(100.0));
        let second = input(RunMode::ComputeSize, AvailableSpace::Definite(101.0));
        let mut cache = NodeLayoutCache::default();
        cache.store(
            first,
            LayoutOutput::from_outer_size(Size {
                width: 100.0,
                height: 10.0,
            }),
        );

        assert!(cache.get(&second).is_none());
    }
}
