//! App scaffolding — generates minimal project templates per stack.
//!
//! Only writes files if the app directory is empty (no existing project overwrite).

use std::fs;
use std::path::Path;

use anyhow::Result;
use hr_environment::types::AppStackType;
use tracing::info;

/// Scaffold a minimal app project in the given directory.
/// Does nothing if the directory already contains files.
pub fn scaffold_app(app_dir: &Path, slug: &str, stack: AppStackType, port: u16) -> Result<()> {
    // Only scaffold if the directory is empty (or doesn't exist)
    if app_dir.exists() {
        let has_files = fs::read_dir(app_dir)?
            .any(|e| e.is_ok());
        if has_files {
            info!(slug, "directory not empty, skipping scaffold");
            return Ok(());
        }
    } else {
        fs::create_dir_all(app_dir)?;
    }

    match stack {
        AppStackType::NextJs => scaffold_nextjs(app_dir, slug, port)?,
        AppStackType::AxumVite => scaffold_axum_vite(app_dir, slug, port)?,
    }

    info!(slug, ?stack, "app scaffolded");
    Ok(())
}

fn scaffold_nextjs(dir: &Path, slug: &str, port: u16) -> Result<()> {
    // package.json
    fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": slug,
            "private": true,
            "scripts": {
                "dev": format!("next dev -p {port}"),
                "build": "next build",
                "start": format!("node server.js")
            },
            "dependencies": {
                "next": "^15",
                "react": "^19",
                "react-dom": "^19"
            }
        }))?,
    )?;

    // server.js (custom server with configurable PORT)
    fs::write(
        dir.join("server.js"),
        format!(
            r#"const {{ createServer }} = require("http");
const {{ parse }} = require("url");
const next = require("next");

const port = parseInt(process.env.PORT || "{port}", 10);
const app = next({{ dev: false }});
const handle = app.getRequestHandler();

app.prepare().then(() => {{
  createServer((req, res) => handle(req, res, parse(req.url, true)))
    .listen(port, () => console.log(`> Ready on http://localhost:${{port}}`));
}});
"#
        ),
    )?;

    // app/ directory
    let app_dir_next = dir.join("app");
    fs::create_dir_all(&app_dir_next)?;

    // app/layout.tsx
    fs::write(
        app_dir_next.join("layout.tsx"),
        r#"export const metadata = { title: "App" };
export default function RootLayout({ children }: { children: React.ReactNode }) {
  return <html lang="en"><body>{children}</body></html>;
}
"#,
    )?;

    // app/page.tsx
    fs::write(
        app_dir_next.join("page.tsx"),
        format!(
            r#"export default function Home() {{
  return <h1>{slug}</h1>;
}}
"#
        ),
    )?;

    // app/api/health/route.ts
    let health_dir = app_dir_next.join("api/health");
    fs::create_dir_all(&health_dir)?;
    fs::write(
        health_dir.join("route.ts"),
        r#"export function GET() {
  return Response.json({ status: "ok" });
}
"#,
    )?;

    Ok(())
}

fn scaffold_axum_vite(dir: &Path, slug: &str, port: u16) -> Result<()> {
    // --- server/ ---
    let server_dir = dir.join("server");
    let server_src = server_dir.join("src");
    fs::create_dir_all(&server_src)?;

    // server/Cargo.toml
    fs::write(
        server_dir.join("Cargo.toml"),
        format!(
            r#"[package]
name = "{slug}"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = "0.8"
tokio = {{ version = "1", features = ["full"] }}
tower-http = {{ version = "0.6", features = ["fs"] }}
"#
        ),
    )?;

    // server/src/main.rs
    fs::write(
        server_src.join("main.rs"),
        format!(
            r#"use axum::{{Router, routing::get}};
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {{
    let port: u16 = std::env::var("PORT").ok()
        .and_then(|p| p.parse().ok()).unwrap_or({port});
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "client/dist".into());

    let app = Router::new()
        .route("/api/health", get(|| async {{ axum::Json(serde_json::json!({{"status": "ok"}})) }}))
        .fallback_service(ServeDir::new(static_dir));

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{{}}", port)).await.unwrap();
    println!("Listening on port {{}}", port);
    axum::serve(listener, app).await.unwrap();
}}
"#
        ),
    )?;

    // --- client/ ---
    let client_dir = dir.join("client");
    let client_src = client_dir.join("src");
    fs::create_dir_all(&client_src)?;

    // client/package.json
    fs::write(
        client_dir.join("package.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "name": format!("{slug}-client"),
            "private": true,
            "scripts": {
                "dev": "vite",
                "build": "vite build"
            },
            "dependencies": {
                "react": "^19",
                "react-dom": "^19"
            },
            "devDependencies": {
                "vite": "^6",
                "@vitejs/plugin-react": "^4"
            }
        }))?,
    )?;

    // client/index.html
    fs::write(
        client_dir.join("index.html"),
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="UTF-8"><title>{slug}</title></head>
<body><div id="root"></div><script type="module" src="/src/App.tsx"></script></body>
</html>
"#
        ),
    )?;

    // client/src/App.tsx
    fs::write(
        client_src.join("App.tsx"),
        format!(
            r#"import React from "react";
import ReactDOM from "react-dom/client";

function App() {{
  return <h1>{slug}</h1>;
}}

ReactDOM.createRoot(document.getElementById("root")!).render(<App />);
"#
        ),
    )?;

    Ok(())
}

