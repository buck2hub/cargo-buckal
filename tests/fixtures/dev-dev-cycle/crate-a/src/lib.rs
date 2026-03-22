pub fn a_value() -> i32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_a_uses_b() {
        // crate-b is a dev-dependency, so it's available in tests
        assert_eq!(crate_b::b_value(), 2);
    }
}
