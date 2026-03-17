pub fn format_new(n: u64) -> String {
    let mut buf = itoa::Buffer::new();
    buf.format(n).to_string()
}
