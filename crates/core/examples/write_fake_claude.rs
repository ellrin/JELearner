//! Writes the fake `claude` CLI used by the tests into a folder, for trying
//! the subscription settings in the browser without a real account:
//! `cargo run -p jl-core --example write_fake_claude -- <dir>`, then start the
//! server with `JELEARNER_CLAUDE_BIN=<dir>/claude`. Create `<dir>/logged-out`
//! to start signed out; put a lesson JSON in `<dir>/reply.json`.
fn main() {
    let dir = std::env::args().nth(1).expect("usage: write_fake_claude <dir>");
    #[cfg(unix)]
    println!("{}", jl_core::claude_code::fake::write(std::path::Path::new(&dir)).display());
}
