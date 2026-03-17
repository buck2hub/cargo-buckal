pub fn format_pinned(n: u64) -> String {
    let mut buf = itoa::Buffer::new();
    buf.format(n).to_string()
}
