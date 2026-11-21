use anyhow::Result;
use clap::{Parser, Subcommand};
use flowdownload_installer::feed::{download_ipa, fetch_latest_release, DEFAULT_SOURCE_URL};
use flowdownload_installer::ipa::{inspect_ipa, prepare_ipa_for_profile};
use flowdownload_installer::profile::InstallProfile;
use flowdownload_installer::sideload;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "flowdownload-installer")]
#[command(about = "Download and sideload FlowDownload (.ipa) to iPhone over USB (Windows + macOS)")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch latest FlowDownload release metadata from source.json
    Check {
        #[arg(long, default_value = DEFAULT_SOURCE_URL)]
        source: String,
    },
    /// Show the 3-App-ID / 2-App-ID / 1-App-ID trade-offs for free Apple IDs
    TradeOffs,
    /// Download latest IPA, inspect App IDs, and prepare (or sideload) for a profile
    Install {
        #[arg(long, default_value = DEFAULT_SOURCE_URL)]
        source: String,
        #[arg(long, value_enum, default_value_t = InstallProfile::Full)]
        profile: InstallProfile,
        #[arg(long, default_value = ".flowdownload-cache")]
        cache_dir: PathBuf,
        #[arg(long, default_value = "https://ani.sidestore.io")]
        anisette_url: String,
        #[arg(long)]
        apple_id: Option<String>,
        #[arg(long)]
        password: Option<String>,
    },
    /// List connected iOS devices over USB
    Devices,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check { source } => {
            let (app, latest) = fetch_latest_release(&source).await?;
            println!("{} ({})", app.name, app.bundle_identifier);
            println!("  Latest: v{} ({})", latest.version, latest.date);
            println!("  Size:   {:.1} MB", latest.size as f64 / 1_000_000.0);
            println!("  URL:    {}", latest.download_url);
            if let Some(sha) = latest.sha256 {
                println!("  SHA256: {sha}");
            }
        }
        Commands::TradeOffs => {
            InstallProfile::print_all_trade_offs();
        }
        Commands::Devices => {
            sideload::check_platform_prerequisites();
            let devices = sideload::list_usb_devices().await?;
            if devices.is_empty() {
                println!("No iOS USB devices detected.");
            } else {
                for d in devices {
                    println!("{} — {} ({})", d.name, d.udid, d.product_version);
                }
            }
        }
        Commands::Install {
            source,
            profile,
            cache_dir,
            anisette_url,
            apple_id,
            password,
        } => {
            sideload::check_platform_prerequisites();
            let (_app, latest) = fetch_latest_release(&source).await?;
            let ipa_path = download_ipa(&latest, &cache_dir).await?;
            let info = inspect_ipa(&ipa_path)?;

            println!(
                "Inspected {} ({}): {} extension(s) found {:?}",
                info.app_bundle_name,
                info.bundle_id.as_deref().unwrap_or("unknown"),
                info.plugins.len(),
                info.plugins
            );

            let report = profile.report();
            println!(
                "\nSelected profile: {:?} ({} App ID(s))",
                report.profile, report.app_ids_required
            );
            for trade_off in &report.trade_offs {
                println!("  Note: {trade_off}");
            }

            let prepared_ipa = prepare_ipa_for_profile(&ipa_path, profile, &cache_dir)?;
            if let Err(err) = sideload::sideload_ipa(
                &prepared_ipa,
                apple_id.as_deref().unwrap_or(""),
                password.as_deref().unwrap_or(""),
                &anisette_url,
                profile,
            )
            .await
            {
                let msg = err.to_string();
                if sideload::is_app_id_limit_error(&msg) {
                    eprintln!("\nApple ID App ID limit reached!");
                    InstallProfile::print_all_trade_offs();
                    eprintln!("Retry with `--profile share-only` (2 IDs) or `--profile core` (1 ID).");
                }
                return Err(err);
            }
        }
    }

    Ok(())
}
