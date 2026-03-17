pub fn format_loose(n: u64) -> String {
    let mut buf = itoa::Buffer::new();
    buf.format(n).to_string()
}
