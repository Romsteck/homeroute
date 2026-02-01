use ca_service::{CaConfig, CertificateAuthority};
use serde_json::json;
use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: ca-cli <command> [options]");
        eprintln!("Commands:");
        eprintln!("  status              Check CA status");
        eprintln!("  init                Initialize CA");
        eprintln!("  list                List all certificates");
        eprintln!("  issue --domains <domain1,domain2,...>");
        eprintln!("  renew --id <cert-id>");
        eprintln!("  revoke --id <cert-id>");
        eprintln!("  renewal-candidates  List certificates needing renewal");
        std::process::exit(1);
    }

    let command = &args[1];
    let storage_path = env::var("CA_STORAGE_PATH")
        .unwrap_or_else(|_| "/var/lib/server-dashboard/ca".to_string());

    let config = CaConfig {
        storage_path,
        ..Default::default()
    };

    let ca = CertificateAuthority::new(config);

    match command.as_str() {
        "status" => {
            let initialized = ca.is_initialized();
            let result = json!({
                "initialized": initialized
            });
            println!("{}", serde_json::to_string_pretty(&result).unwrap());
        }

        "init" => {
            match ca.init().await {
                Ok(_) => {
                    let result = json!({
                        "success": true,
                        "message": "CA initialized successfully"
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        "list" => {
            match ca.list_certificates() {
                Ok(certs) => {
                    let result = json!({
                        "success": true,
                        "certificates": certs
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        "issue" => {
            // Parse --domains flag
            let mut domains = Vec::new();
            for i in 2..args.len() {
                if args[i] == "--domains" && i + 1 < args.len() {
                    domains = args[i + 1].split(',').map(|s| s.trim().to_string()).collect();
                    break;
                }
            }

            if domains.is_empty() {
                eprintln!("Error: --domains flag required");
                std::process::exit(1);
            }

            // Initialize CA (loads existing or creates new)
            if let Err(e) = ca.init().await {
                eprintln!("Failed to initialize CA: {}", e);
                std::process::exit(1);
            }

            match ca.issue_certificate(domains).await {
                Ok(cert_info) => {
                    let result = json!({
                        "success": true,
                        "certificate": cert_info
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        "renew" => {
            // Parse --id flag
            let mut cert_id = None;
            for i in 2..args.len() {
                if args[i] == "--id" && i + 1 < args.len() {
                    cert_id = Some(args[i + 1].clone());
                    break;
                }
            }

            let cert_id = match cert_id {
                Some(id) => id,
                None => {
                    eprintln!("Error: --id flag required");
                    std::process::exit(1);
                }
            };

            // Initialize CA (loads existing or creates new)
            if let Err(e) = ca.init().await {
                eprintln!("Failed to initialize CA: {}", e);
                std::process::exit(1);
            }

            match ca.renew_certificate(&cert_id).await {
                Ok(cert_info) => {
                    let result = json!({
                        "success": true,
                        "certificate": cert_info
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        "revoke" => {
            // Parse --id flag
            let mut cert_id = None;
            for i in 2..args.len() {
                if args[i] == "--id" && i + 1 < args.len() {
                    cert_id = Some(args[i + 1].clone());
                    break;
                }
            }

            let cert_id = match cert_id {
                Some(id) => id,
                None => {
                    eprintln!("Error: --id flag required");
                    std::process::exit(1);
                }
            };

            match ca.revoke_certificate(&cert_id) {
                Ok(_) => {
                    let result = json!({
                        "success": true,
                        "message": "Certificate revoked successfully"
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        "renewal-candidates" => {
            match ca.certificates_needing_renewal() {
                Ok(certs) => {
                    let result = json!({
                        "success": true,
                        "certificates": certs
                    });
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    let result = json!({
                        "success": false,
                        "error": format!("{}", e)
                    });
                    eprintln!("{}", serde_json::to_string_pretty(&result).unwrap());
                    std::process::exit(1);
                }
            }
        }

        _ => {
            eprintln!("Unknown command: {}", command);
            std::process::exit(1);
        }
    }
}
