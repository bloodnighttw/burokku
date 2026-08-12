pub enum DrawCommand {
    PushClip,
    PopClip,
    Fill,
    Stroke,
}

pub struct Canvas {}

#[cfg(test)]
mod test {

    pub struct OffscreenCanvas {}
}
