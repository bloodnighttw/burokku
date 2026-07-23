pub struct Px(i32);

impl Px {
    pub(crate) const fn value(self) -> i32 {
        self.0
    }
}

// the percent measurement
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Percent(f32);

impl Percent {
    pub(crate) const fn value(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Fr(f32);

pub struct Auto;

trait MeasurementExt {
    fn px(self) -> Px;
    fn percent(self) -> Percent;
    fn fr(self) -> Fr;
}

macro_rules! impl_measurement_ext {
    ($($type:ty),+ $(,)?) => {
        $(
            impl MeasurementExt for $type {
                fn px(self) -> Px {
                    Px(self as i32)
                }

                fn percent(self) -> Percent {
                    Percent(self as f32)
                }

                fn fr(self) -> Fr {
                    Fr(self as f32)
                }
            }
        )+
    };
}

impl_measurement_ext!(f32, f64, i16, i32, u16, u32, i8, u8, usize);
