#![forbid(unsafe_code)]

/// Returns the bootstrap greeting used to verify the Rust build and test path.
#[must_use]
pub const fn hello_world() -> &'static str {
    "Hello, world!"
}

#[cfg(test)]
mod tests {
    use super::hello_world;

    #[test]
    fn says_hello_world() {
        assert_eq!(hello_world(), "Hello, world!");
    }
}
