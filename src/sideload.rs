use crate::profile::InstallProfile;
use anyhow::Result;
use serde::Serialize;
use std::future::Future;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct UsbDeviceSummary {
    pub name: String,
    pub udid: String,
    pub product_version: String,
}

pub fn check_platform_prerequisites() {
    #[cfg(target_os = "windows")]
    {
        println!("Platform: Windows");
        println!("  Note: USB communication requires Apple Mobile Device Support (Apple Devices / iTunes driver).");
    }
    #[cfg(target_os = "macos")]
    {
        println!("Platform: macOS (native usbmuxd ready)");
    }
}

/// Checks if an Apple Developer error message indicates the 3-App-ID / 10-App-ID free account limit.
pub fn is_app_id_limit_error(err_msg: &str) -> bool {
    let lower = err_msg.to_lowercase();
    lower.contains("maximum number of registered")
        || lower.contains("3 apps")
        || (lower.contains("app id") && lower.contains("limit"))
        || lower.contains("7460")
}

#[cfg(feature = "sideload")]
pub async fn list_usb_devices() -> Result<Vec<UsbDeviceSummary>> {
    use idevice::usbmuxd::UsbmuxdConnection;

    let mut conn = UsbmuxdConnection::default()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to usbmuxd: {e}"))?;
    let devs = conn
        .get_devices()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list USB devices: {e}"))?;
    let mut out = Vec::new();
    for dev in devs {
        out.push(UsbDeviceSummary {
            name: format!("iOS Device ({:?})", dev.connection_type),
            udid: dev.udid,
            product_version: String::from("iOS"),
        });
    }
    Ok(out)
}

#[cfg(not(feature = "sideload"))]
pub async fn list_usb_devices() -> Result<Vec<UsbDeviceSummary>> {
    println!("USB device discovery requires `--features sideload` (isideload + idevice).");
    Ok(vec![])
}

#[cfg(feature = "sideload")]
pub async fn sideload_ipa(
    ipa_path: &Path,
    apple_id: &str,
    password: &str,
    anisette_url: &str,
    profile: InstallProfile,
) -> Result<()> {
    use std::io::{self, Write};
    sideload_ipa_with_callbacks(
        ipa_path,
        apple_id,
        password,
        anisette_url,
        profile,
        || async {
            print!("Enter 6-digit Apple ID 2FA code: ");
            io::stdout().flush().ok();
            let mut code = String::new();
            io::stdin().read_line(&mut code).ok();
            code.trim().to_string()
        },
        |pct| {
            println!("  Progress: {:.0}%", pct);
        },
    )
    .await
}

#[cfg(feature = "sideload")]
pub async fn sideload_ipa_with_callbacks<F2fa, Fut2fa, FProg>(
    ipa_path: &Path,
    apple_id: &str,
    password: &str,
    anisette_url: &str,
    profile: InstallProfile,
    on_two_factor: F2fa,
    on_progress: FProg,
) -> Result<()>
where
    F2fa: Fn() -> Fut2fa + Send + Sync + 'static,
    Fut2fa: Future<Output = String> + Send + 'static,
    FProg: Fn(f32) + Send + Sync + Clone + 'static,
{
    use idevice::usbmuxd::{UsbmuxdAddr, UsbmuxdConnection};
    use isideload::{
        anisette::remote_v3::RemoteV3AnisetteProvider,
        auth::apple_account::{AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse},
        dev::developer_session::DeveloperSession,
        sideload::builder::{
            ExtensionsBehavior, ExtensionsBehaviorSelection, MaxCertsBehavior, SideloaderBuilder,
            TeamSelection,
        },
        util::{
            callbacks::{ExtensionsBehaviorCallbackBox, MaxCertsCallbackBox},
            storage::new_storage,
        },
    };
    use std::sync::Arc;

    if apple_id.trim().is_empty() || password.is_empty() {
        anyhow::bail!("Pass --apple-id and --password to sign and install onto your iPhone.");
    }

    let _ = isideload::init();

    println!("Connecting to usbmuxd...");
    let mut conn = UsbmuxdConnection::default()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to usbmuxd: {e}"))?;
    let devs = conn
        .get_devices()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to list USB devices: {e}"))?;
    let device = devs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("No iOS USB device found. Plug in your iPhone and tap Trust."))?;

    println!("Using device UDID: {}", device.udid);
    let provider = device.to_provider(UsbmuxdAddr::default(), "FlowDownload-Installer");

    println!("Authenticating Apple ID via Anisette ({anisette_url})...");
    let storage = Box::new(new_storage());
    let anisette = RemoteV3AnisetteProvider::new(anisette_url, storage, "0".to_string())
        .map_err(|e| anyhow::anyhow!("Anisette init failed: {e}"))?;

    let two_fa_cb = Arc::new(on_two_factor);
    let mut account = AppleAccount::builder(apple_id)
        .anisette_provider(anisette)
        .login(password, move |_params: TwoFactorCallbackParams| {
            let cb = Arc::clone(&two_fa_cb);
            async move {
                let code = cb().await;
                Ok(TwoFactorCallbackResponse::SubmitCode(code))
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("Apple ID login failed: {e}"))?;

    let dev_session = DeveloperSession::from_account(&mut account)
        .await
        .map_err(|e| anyhow::anyhow!("Developer session failed: {e}"))?;

    let ext_behavior = match profile {
        InstallProfile::Full | InstallProfile::ShareOnly => ExtensionsBehavior::RegisterAll,
        InstallProfile::Core => ExtensionsBehavior::ReuseMain,
    };

    let mut sideloader =
        SideloaderBuilder::<MaxCertsCallbackBox, ExtensionsBehaviorCallbackBox>::new(
            dev_session,
            apple_id.to_string(),
        )
        .team_selection(TeamSelection::First)
        .max_certs_behavior(MaxCertsBehavior::Revoke)
        .extensions_behavior(ExtensionsBehaviorSelection::Fixed(ext_behavior))
        .build();

    println!("Signing and installing {}...", ipa_path.display());
    sideloader
        .install_app(
            &provider,
            ipa_path.to_path_buf(),
            false,
            Some(move |pct: f32| {
                let cb = on_progress.clone();
                async move {
                    cb(pct);
                }
            }),
            None,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Sideload failed: {e}"))?;

    println!("Installed FlowDownload successfully!");
    Ok(())
}

#[cfg(not(feature = "sideload"))]
pub async fn sideload_ipa(
    ipa_path: &Path,
    _apple_id: &str,
    _password: &str,
    _anisette_url: &str,
    profile: InstallProfile,
) -> Result<()> {
    println!(
        "[Dry Run] Prepared {} for profile {:?} ({} App ID(s)).",
        ipa_path.display(),
        profile,
        profile.report().app_ids_required
    );
    println!("Rebuild with `cargo run --features sideload` to enable USB signing & install.");
    Ok(())
}
