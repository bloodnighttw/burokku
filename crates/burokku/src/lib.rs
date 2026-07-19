use wasm_bindgen::prelude::*;

/// Return a greeting from Rust.
#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

/// Add two numbers in the Rust library.
#[wasm_bindgen]
pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_a_name() {
        assert_eq!(greet("Burokku"), "Hello, Burokku!");
    }

    #[test]
    fn adds_numbers() {
        assert_eq!(add(20, 22), 42);
    }
}
