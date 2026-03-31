use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use hr_environment::AppStackType;
use sha2::{Digest, Sha256};
use tracing::{info, warn};

const ARTIFACTS_DIR: &str = "/opt/homeroute/data/artifacts";

pub struct BuildContext {
    pub app_slug: String,
    pub stack: AppStackType,
    pub version: String,
    /// Bare repo path (e.g., /opt/homeroute/data/git/repos/{slug}.git)
    pub repo_path: PathBuf,
    /// Build command from TOML config.
    pub build_command: String,
}

pub struct BuildResult {
    pub artifact_path: PathBuf,
    pub sha256: String,
    pub size_bytes: u64,
}

/// Build an application and produce a versioned artifact.
/// If an artifact already exists for this app+version, skip the build and return the existing one.
pub async fn build_app(ctx: &BuildContext) -> anyhow::Result<BuildResult> {
    let artifact_dir = PathBuf::from(ARTIFACTS_DIR).join(&ctx.app_slug);
    let artifact_filename = format!("{}-{}.tar.gz", ctx.app_slug, ctx.version);
    let artifact_path = artifact_dir.join(&artifact_filename);

    // Check if artifact already exists (reuse for chain promotions)
    if artifact_path.exists() {
        info!(app = %ctx.app_slug, version = %ctx.version, "Artifact already exists, reusing");
        let sha256 = compute_sha256(&artifact_path).await?;
        let meta = tokio::fs::metadata(&artifact_path).await?;
        return Ok(BuildResult {
            artifact_path,
            sha256,
            size_bytes: meta.len(),
        });
    }

    // 1. Clone bare repo to temp build dir
    let build_dir = PathBuf::from(format!(
        "/tmp/hr-build-{}-{}",
        ctx.app_slug,
        chrono::Utc::now().timestamp()
    ));

    let output = tokio::process::Command::new("git")
        .args(["clone", "--depth", "1"])
        .arg(&ctx.repo_path)
        .arg(&build_dir)
        .output()
        .await
        .context("Failed to clone repo")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Git clone failed: {stderr}");
    }

    // 2. Install dependencies if needed (Next.js / Node.js stacks)
    if matches!(ctx.stack, AppStackType::NextJs) {
        info!(app = %ctx.app_slug, "Installing dependencies (pnpm install)");
        let install_cmd = if build_dir.join("pnpm-lock.yaml").exists() {
            "pnpm install && pnpm rebuild"
        } else if build_dir.join("package-lock.json").exists() {
            "npm install && npm rebuild"
        } else {
            "npm install"
        };
        let output = tokio::process::Command::new("bash")
            .args(["-c", install_cmd])
            .current_dir(&build_dir)
            .env("HOME", "/root")
            .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
            .output()
            .await
            .context("Failed to install dependencies")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let _ = tokio::fs::remove_dir_all(&build_dir).await;
            bail!("Dependency install failed:\nstdout: {stdout}\nstderr: {stderr}");
        }
    }

    // 3. Run build command
    info!(app = %ctx.app_slug, cmd = %ctx.build_command, "Running build command");
    let output = tokio::process::Command::new("bash")
        .args(["-c", &ctx.build_command])
        .current_dir(&build_dir)
        .env("HOME", "/root")
        .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .await
        .context("Failed to run build command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() {
        // Clean up
        let _ = tokio::fs::remove_dir_all(&build_dir).await;
        bail!("Build failed:\nstdout: {stdout}\nstderr: {stderr}");
    }

    // 4. Package artifact based on stack
    tokio::fs::create_dir_all(&artifact_dir).await?;
    package_artifact(ctx.stack, &build_dir, &artifact_path).await?;

    // 5. Compute SHA256
    let sha256 = compute_sha256(&artifact_path).await?;
    let meta = tokio::fs::metadata(&artifact_path).await?;

    // 6. Clean up build dir
    let _ = tokio::fs::remove_dir_all(&build_dir).await;

    info!(
        app = %ctx.app_slug,
        version = %ctx.version,
        size_mb = meta.len() as f64 / 1_048_576.0,
        "Build complete"
    );

    Ok(BuildResult {
        artifact_path,
        sha256,
        size_bytes: meta.len(),
    })
}

/// Package build output as tar.gz based on stack type.
async fn package_artifact(
    stack: AppStackType,
    build_dir: &Path,
    artifact_path: &Path,
) -> anyhow::Result<()> {
    // Build the list of paths to include based on stack
    let includes: Vec<String> = match stack {
        AppStackType::AxumVite => {
            vec![
                "server/target/release/".to_string(),
                "client/dist/".to_string(),
            ]
        }
        AppStackType::Axum => {
            vec!["target/release/".to_string()]
        }
        AppStackType::NextJs => {
            vec![
                ".next/".to_string(),
                "node_modules/".to_string(),
                "public/".to_string(),
                "package.json".to_string(),
                "server.js".to_string(),
                "server.ts".to_string(),
            ]
        }
    };

    // Build tar arguments: for each existing include, add -C and the path
    let mut args = vec![
        "czf".to_string(),
        artifact_path.to_string_lossy().to_string(),
    ];

    let mut found_any = false;
    for include in &includes {
        let full_path = build_dir.join(include);
        if full_path.exists() {
            args.push("-C".to_string());
            args.push(build_dir.to_string_lossy().to_string());
            args.push(include.clone());
            found_any = true;
        }
    }

    // If no standard paths found, just package everything
    if !found_any {
        warn!(
            "No standard paths found for {:?} stack, packaging entire build dir",
            stack
        );
        args.push("-C".to_string());
        args.push(build_dir.to_string_lossy().to_string());
        args.push(".".to_string());
    }

    let output = tokio::process::Command::new("tar")
        .args(&args)
        .output()
        .await
        .context("Failed to create tar archive")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("tar packaging failed: {stderr}");
    }

    Ok(())
}

/// Compute SHA256 of a file.
async fn compute_sha256(path: &Path) -> anyhow::Result<String> {
    let data = tokio::fs::read(path)
        .await
        .context("Failed to read artifact for SHA256")?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Get the artifact URL for a given app and version.
pub fn artifact_url(app_slug: &str, version: &str) -> String {
    format!(
        "http://10.0.0.254:4001/artifacts/{}/{}-{}.tar.gz",
        app_slug, app_slug, version
    )
}
