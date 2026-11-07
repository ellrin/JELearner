// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Bundled app: web/ and workspace-template/ ship as resources.
            // `cargo run -p jelearner`: fall back to the repository folders.
            let paths = app
                .path()
                .resource_dir()
                .ok()
                .map(|dir| jl_server::Paths { web: dir.join("web"), template: dir.join("workspace-template") })
                .filter(|p| p.web.join("index.html").exists())
                .unwrap_or_else(jl_server::Paths::from_repo);
            let settings = jl_server::Settings::from_env(paths);

            // The server runs on its own runtime thread for the life of the app.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                rt.block_on(async move {
                    match jl_server::bind(settings, 0).await {
                        Ok((addr, serve)) => {
                            let _ = tx.send(Ok(addr));
                            if let Err(e) = serve.await {
                                eprintln!("server stopped: {e}");
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e));
                        }
                    }
                });
            });
            let addr = rx.recv()??;

            let url = url::Url::parse(&format!("http://{addr}/"))?;
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
                .title("JELearner")
                .inner_size(1280.0, 860.0)
                .min_inner_size(380.0, 600.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building JELearner")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // A local model this app started must not outlive it.
                jl_server::kill_local_model();
            }
        });
}
