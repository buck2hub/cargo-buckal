pub fn b_value() -> i32 {
    2
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_b_uses_a() {
        // crate-a is a dev-dependency, so it's available in tests
        assert_eq!(crate_a::a_value(), 1);
    }
}
