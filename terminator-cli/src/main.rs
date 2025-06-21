#!/usr/bin/env cargo

//! Terminator CLI
//!
//! A cross-platform Rust tool to manage the Terminator project, including version management,
//! releases, and development workflows.
//!
//! Usage from workspace root:
//!   cargo run --bin terminator -- patch      # Bump patch version
//!   cargo run --bin terminator -- minor      # Bump minor version  
//!   cargo run --bin terminator -- major      # Bump major version
//!   cargo run --bin terminator -- sync       # Sync all versions
//!   cargo run --bin terminator -- status     # Show current status
//!   cargo run --bin terminator -- tag        # Tag and push current version
//!   cargo run --bin terminator -- release    # Full release: bump patch + tag + push

use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Parser)]
#[command(name = "terminator")]
#[command(about = "🤖 Terminator CLI - AI-native GUI automation")]
#[command(
    long_about = "Terminator CLI provides tools for managing the Terminator project, including version management, releases, and development workflows."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bump patch version (x.y.Z+1)
    Patch,
    /// Bump minor version (x.Y+1.0)
    Minor,
    /// Bump major version (X+1.0.0)
    Major,
    /// Sync all package versions without bumping
    Sync,
    /// Show current version status
    Status,
    /// Tag current version and push (triggers CI)
    Tag,
    /// Full release: bump patch + tag + push
    Release,
}

fn main() {
    // Ensure we're in the project root (workspace root)
    ensure_project_root();

    let cli = Cli::parse();

    match cli.command {
        Commands::Patch => bump_version("patch"),
        Commands::Minor => bump_version("minor"),
        Commands::Major => bump_version("major"),
        Commands::Sync => sync_all_versions(),
        Commands::Status => show_status(),
        Commands::Tag => tag_and_push(),
        Commands::Release => full_release(),
    }
}

fn ensure_project_root() {
    // Check if we're already in the project root
    if Path::new("Cargo.toml").exists() && Path::new("terminator").exists() {
        return;
    }

    // If we're in terminator-cli, go up one level
    if env::current_dir()
        .map(|p| {
            p.file_name()
                .map(|n| n == "terminator-cli")
                .unwrap_or(false)
        })
        .unwrap_or(false)
        && env::set_current_dir("..").is_err()
    {
        eprintln!("❌ Failed to change to project root directory");
        std::process::exit(1);
    }

    // Final check
    if !Path::new("Cargo.toml").exists() || !Path::new("terminator").exists() {
        eprintln!("❌ Not in Terminator project root. Please run from workspace root.");
        eprintln!("💡 Usage: terminator <command>");
        std::process::exit(1);
    }
}

fn get_workspace_version() -> Result<String, Box<dyn std::error::Error>> {
    let cargo_toml = fs::read_to_string("Cargo.toml")?;

    for line in cargo_toml.lines() {
        if line.trim().starts_with("version = ") {
            let version = line.split('"').nth(1).ok_or("Invalid version format")?;
            return Ok(version.to_string());
        }
    }

    Err("Version not found in Cargo.toml".into())
}

fn set_workspace_version(new_version: &str) -> Result<(), Box<dyn std::error::Error>> {
    let cargo_toml = fs::read_to_string("Cargo.toml")?;
    let mut lines: Vec<String> = cargo_toml.lines().map(|s| s.to_string()).collect();

    for line in &mut lines {
        if line.trim().starts_with("version = ") {
            *line = format!(
                "version = \"{}\" # From your original Cargo.toml",
                new_version
            );
            break;
        }
    }

    fs::write("Cargo.toml", lines.join("\n"))?;
    Ok(())
}

fn parse_version(version: &str) -> Result<(u32, u32, u32), Box<dyn std::error::Error>> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return Err("Invalid version format".into());
    }

    let major = parts[0].parse::<u32>()?;
    let minor = parts[1].parse::<u32>()?;
    let patch = parts[2].parse::<u32>()?;

    Ok((major, minor, patch))
}

fn bump_version(bump_type: &str) {
    println!("🔄 Bumping {} version...", bump_type);

    let current_version = match get_workspace_version() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to get current version: {}", e);
            return;
        }
    };

    let (major, minor, patch) = match parse_version(&current_version) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to parse version {}: {}", current_version, e);
            return;
        }
    };

    let new_version = match bump_type {
        "patch" => format!("{}.{}.{}", major, minor, patch + 1),
        "minor" => format!("{}.{}.0", major, minor + 1),
        "major" => format!("{}.0.0", major + 1),
        _ => {
            eprintln!("❌ Invalid bump type: {}", bump_type);
            return;
        }
    };

    println!("📝 {} → {}", current_version, new_version);

    if let Err(e) = set_workspace_version(&new_version) {
        eprintln!("❌ Failed to update workspace version: {}", e);
        return;
    }

    println!("✅ Updated workspace version to {}", new_version);
    sync_all_versions();
}

fn sync_all_versions() {
    println!("🔄 Syncing all package versions...");

    let workspace_version = match get_workspace_version() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to get workspace version: {}", e);
            return;
        }
    };

    println!("📦 Workspace version: {}", workspace_version);

    // Sync Node.js bindings
    sync_nodejs_bindings(&workspace_version);

    // Sync MCP agent
    sync_mcp_agent(&workspace_version);

    // Update Cargo.lock
    println!("🔒 Updating Cargo.lock...");
    if let Err(e) = run_command("cargo", &["check", "--quiet"]) {
        eprintln!("⚠️  Warning: Failed to update Cargo.lock: {}", e);
    }

    println!("✅ All versions synchronized!");
}

fn sync_nodejs_bindings(version: &str) {
    println!("📦 Syncing Node.js bindings to version {}...", version);

    if Path::new("bindings/nodejs").exists() {
        // First, try to update the package.json directly
        if let Err(e) = update_package_json("bindings/nodejs/package.json", version) {
            eprintln!(
                "⚠️  Warning: Failed to update Node.js package.json directly: {}",
                e
            );
        } else {
            println!("✅ Updated Node.js package.json to {}", version);
        }

        // Then run the sync script if it exists
        let mut success = false;
        if std::env::set_current_dir("bindings/nodejs").is_ok() {
            println!("🔄 Running npm run sync-version...");
            #[allow(clippy::redundant_pattern_matching)]
            if let Ok(_) = run_command("npm", &["run", "sync-version"]) {
                if std::env::set_current_dir("../..").is_ok() {
                    success = true;
                    println!("✅ Node.js sync script completed");
                }
            } else {
                eprintln!("⚠️  Warning: npm run sync-version failed");
                let _ = std::env::set_current_dir("../..");
            }
        }

        if !success {
            eprintln!("⚠️  Warning: Failed to run Node.js sync script, but package.json was updated directly");
        }
    } else {
        println!("⚠️  Node.js bindings directory not found, skipping");
    }
}

fn sync_mcp_agent(version: &str) {
    println!("📦 Syncing MCP agent...");

    if !Path::new("terminator-mcp-agent").exists() {
        return;
    }

    // Update main package.json
    if let Err(e) = update_package_json("terminator-mcp-agent/package.json", version) {
        eprintln!(
            "⚠️  Warning: Failed to update MCP agent package.json: {}",
            e
        );
        return;
    }

    // Update platform packages
    let npm_dir = Path::new("terminator-mcp-agent/npm");
    if npm_dir.exists() {
        if let Ok(entries) = fs::read_dir(npm_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let package_json = entry.path().join("package.json");
                    if package_json.exists() {
                        if let Err(e) =
                            update_package_json(&package_json.to_string_lossy(), version)
                        {
                            eprintln!(
                                "⚠️  Warning: Failed to update {}: {}",
                                entry.path().display(),
                                e
                            );
                        } else {
                            println!("📦 Updated {}", entry.file_name().to_string_lossy());
                        }
                    }
                }
            }
        }
    }

    // Update package-lock.json
    let mut success = false;
    if std::env::set_current_dir("terminator-mcp-agent").is_ok() {
        #[allow(clippy::redundant_pattern_matching)]
        if let Ok(_) = run_command("npm", &["install", "--package-lock-only", "--silent"]) {
            if std::env::set_current_dir("..").is_ok() {
                success = true;
            }
        }
    }

    if !success {
        eprintln!("⚠️  Warning: Failed to update MCP agent package-lock.json");
    }

    println!("✅ MCP agent synced");
}

fn update_package_json(path: &str, version: &str) -> Result<(), Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let mut pkg: serde_json::Value = serde_json::from_str(&content)?;

    // Update main version
    pkg["version"] = serde_json::Value::String(version.to_string());

    // Update optional dependencies that start with terminator-mcp- or terminator.js-
    if let Some(deps) = pkg
        .get_mut("optionalDependencies")
        .and_then(|v| v.as_object_mut())
    {
        for (key, value) in deps.iter_mut() {
            if key.starts_with("terminator-mcp-") || key.starts_with("terminator.js-") {
                *value = serde_json::Value::String(version.to_string());
            }
        }
    }

    // Write back with pretty formatting
    let formatted = serde_json::to_string_pretty(&pkg)?;
    fs::write(path, formatted + "\n")?;

    Ok(())
}

fn show_status() {
    println!("📊 Terminator Project Status");
    println!("============================");

    let workspace_version = get_workspace_version().unwrap_or_else(|_| "ERROR".to_string());
    println!("📦 Workspace version: {}", workspace_version);

    // Show package versions
    let nodejs_version = get_package_version("bindings/nodejs/package.json");
    let mcp_version = get_package_version("terminator-mcp-agent/package.json");

    println!();
    println!("Package versions:");
    println!("  Node.js bindings: {}", nodejs_version);
    println!("  MCP agent:        {}", mcp_version);

    // Git status
    println!();
    println!("Git status:");
    if let Ok(output) = Command::new("git").args(["status", "--porcelain"]).output() {
        let status = String::from_utf8_lossy(&output.stdout);
        if status.trim().is_empty() {
            println!("  ✅ Working directory clean");
        } else {
            println!("  ⚠️  Uncommitted changes:");
            for line in status.lines().take(5) {
                println!("     {}", line);
            }
        }
    }
}

fn get_package_version(path: &str) -> String {
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(pkg) => pkg
                .get("version")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "No version field".to_string()),
            Err(_) => "Parse error".to_string(),
        },
        Err(_) => "Not found".to_string(),
    }
}

fn tag_and_push() {
    let version = match get_workspace_version() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("❌ Failed to get current version: {}", e);
            return;
        }
    };

    println!("🏷️  Tagging and pushing version {}...", version);

    // Check for uncommitted changes
    if let Ok(output) = Command::new("git").args(["diff", "--name-only"]).output() {
        let diff = String::from_utf8_lossy(&output.stdout);
        if !diff.trim().is_empty() {
            println!("⚠️  Uncommitted changes detected. Committing...");
            if let Err(e) = run_command("git", &["add", "."]) {
                eprintln!("❌ Failed to git add: {}", e);
                return;
            }
            if let Err(e) = run_command(
                "git",
                &["commit", "-m", &format!("Bump version to {}", version)],
            ) {
                eprintln!("❌ Failed to git commit: {}", e);
                return;
            }
        }
    }

    // Create tag
    let tag = format!("v{}", version);
    if let Err(e) = run_command(
        "git",
        &[
            "tag",
            "-a",
            &tag,
            "-m",
            &format!("Release version {}", version),
        ],
    ) {
        eprintln!("❌ Failed to create tag: {}", e);
        return;
    }

    // Push changes and tag
    if let Err(e) = run_command("git", &["push", "origin", "main"]) {
        eprintln!("❌ Failed to push changes: {}", e);
        return;
    }

    if let Err(e) = run_command("git", &["push", "origin", &tag]) {
        eprintln!("❌ Failed to push tag: {}", e);
        return;
    }

    println!("✅ Successfully released version {}!", version);
    println!("🔗 Check CI: https://github.com/mediar-ai/terminator/actions");
}

fn full_release() {
    println!("🚀 Starting full release process...");
    bump_version("patch");
    tag_and_push();
}

fn run_command(program: &str, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Command failed: {} {}\nError: {}",
            program,
            args.join(" "),
            stderr
        )
        .into());
    }

    Ok(())
}
