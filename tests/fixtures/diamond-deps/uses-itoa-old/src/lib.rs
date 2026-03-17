pub fn format_old(n: u64) -> String {
    let mut buf = itoa::Buffer::new();
    buf.format(n).to_string()
}
