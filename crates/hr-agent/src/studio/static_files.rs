use hyper::Response;

use crate::proxy::{BoxBody, full_body};

#[derive(rust_embed::Embed)]
#[folder = "../../web-studio/dist/"]
struct StudioAssets;

pub fn serve_asset(path: &str) -> Response<BoxBody> {
    let path = path.strip_prefix('/').unwrap_or(path);
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = StudioAssets::get(path) {
        let mime = mime_from_ext(path);
        let cache = if path.starts_with("assets/") {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        Response::builder()
            .status(200)
            .header("Content-Type", mime)
            .header("Cache-Control", cache)
            .body(full_body(file.data.to_vec()))
            .unwrap()
    } else {
        // SPA fallback: serve index.html for non-file paths
        match StudioAssets::get("index.html") {
            Some(index) => Response::builder()
                .status(200)
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Cache-Control", "no-cache")
                .body(full_body(index.data.to_vec()))
                .unwrap(),
            None => Response::builder()
                .status(404)
                .body(full_body("Studio not available"))
                .unwrap(),
        }
    }
}

fn mime_from_ext(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ttf" => "font/ttf",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}
