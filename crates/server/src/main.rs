//! Browser mode: `cargo run -p jl-server`, then open http://127.0.0.1:7979.

#[tokio::main]
async fn main() {
    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(7979);
    let settings = jl_server::Settings::from_env(jl_server::Paths::from_repo());
    let (addr, serve) = jl_server::bind(settings, port).await.expect("bind 127.0.0.1");
    eprintln!("JELearner on http://{addr}");
    serve.await.expect("server");
}
