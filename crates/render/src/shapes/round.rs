// a.k.a CornerRadius
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Round {
    pub lt: f32,
    pub rt: f32,
    pub rb: f32,
    pub lb: f32,
}

impl Round {
    pub(crate) fn fit(self, width: f32, height: f32) -> Self {
        let mut radii = [self.lt, self.rt, self.rb, self.lb].map(|radius| {
            if radius.is_finite() {
                radius.max(0.0)
            } else {
                0.0
            }
        });
        let scale = [
            edge_scale(width, radii[0] + radii[1]),
            edge_scale(height, radii[1] + radii[2]),
            edge_scale(width, radii[2] + radii[3]),
            edge_scale(height, radii[3] + radii[0]),
        ]
        .into_iter()
        .fold(1.0_f32, f32::min);

        radii.iter_mut().for_each(|radius| *radius *= scale);
        Self {
            lt: radii[0],
            rt: radii[1],
            rb: radii[2],
            lb: radii[3],
        }
    }
}

fn edge_scale(length: f32, radii: f32) -> f32 {
    if length.is_finite() && length > 0.0 && radii > length {
        length / radii
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scales_overlapping_radii_and_sanitizes_invalid_values() {
        let fitted = Round {
            lt: 30.0,
            rt: 30.0,
            rb: f32::NAN,
            lb: -4.0,
        }
        .fit(40.0, 20.0);

        assert_eq!(
            fitted,
            Round {
                lt: 20.0,
                rt: 20.0,
                rb: 0.0,
                lb: 0.0,
            }
        );
    }
}
