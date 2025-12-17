// Legacy test program; kept as a binary target to avoid breaking build tooling.
// The llama-cpp-2 crate API has changed and the original chat_template module is no longer available.
fn main() {
    println!("test_chat_template: disabled (llama-cpp-2 chat_template API not available)");
}
