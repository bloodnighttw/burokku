use render::Transform as RenderTransform;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) enum Transform {
    #[default]
    None,
    Matrix([f32; 6]),
}

impl Transform {
    pub(crate) const IDENTITY_MATRIX: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    pub(crate) const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }

    pub(crate) const fn matrix(self) -> [f32; 6] {
        match self {
            Self::None => Self::IDENTITY_MATRIX,
            Self::Matrix(matrix) => matrix,
        }
    }
}

impl From<Transform> for RenderTransform {
    fn from(value: Transform) -> Self {
        Self {
            matrix: value.matrix(),
        }
    }
}
