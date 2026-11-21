#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use flowdownload_installer::feed::{download_ipa, fetch_latest_release, AltVersion, DEFAULT_SOURCE_URL};
use flowdownload_installer::ipa::prepare_ipa_for_profile;
use flowdownload_installer::profile::InstallProfile;
use flowdownload_installer::sideload::{self, UsbDeviceSummary};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, Mutex};

struct AppState {
    two_factor_tx: Arc<Mutex<Option<oneshot::Sender<String>>>>,
}

#[derive(Clone, Serialize)]
struct ProgressPayload {
    stage: String,
    percent: f32,
}

#[tauri::command]
async fn get_latest_release() -> Result<AltVersion, String> {
    let (_app, latest) = fetch_latest_release(DEFAULT_SOURCE_URL)
        .await
        .map_err(|e| e.to_string())?;
    Ok(latest)
}

#[tauri::command]
async fn list_usb_devices() -> Result<Vec<UsbDeviceSummary>, String> {
    sideload::list_usb_devices().await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn submit_two_factor_code(state: State<'_, AppState>, code: String) -> Result<(), String> {
    if let Some(tx) = state.two_factor_tx.lock().await.take() {
        let _ = tx.send(code);
    }
    Ok(())
}

#[tauri::command]
async fn start_install(
    app: AppHandle,
    state: State<'_, AppState>,
    apple_id: String,
    password: String,
    profile: String,
) -> Result<(), String> {
    let install_profile = match profile.as_str() {
        "share-only" => InstallProfile::ShareOnly,
        "core" => InstallProfile::Core,
        _ => InstallProfile::Full,
    };

    let _ = app.emit(
        "sideload-progress",
        ProgressPayload {
            stage: "Fetching release...".into(),
            percent: 5.0,
        },
    );

    let (_meta, latest) = fetch_latest_release(DEFAULT_SOURCE_URL)
        .await
        .map_err(|e| e.to_string())?;

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("FlowDownload");

    let _ = app.emit(
        "sideload-progress",
        ProgressPayload {
            stage: "Downloading & verifying IPA...".into(),
            percent: 20.0,
        },
    );

    let raw_ipa = download_ipa(&latest, &cache_dir)
        .await
        .map_err(|e| e.to_string())?;

    let prepared_ipa = prepare_ipa_for_profile(&raw_ipa, install_profile, &cache_dir)
        .map_err(|e| e.to_string())?;

    let two_fa_slot = Arc::clone(&state.two_factor_tx);
    let app_for_2fa = app.clone();
    let app_for_prog = app.clone();

    sideload::sideload_ipa_with_callbacks(
        &prepared_ipa,
        &apple_id,
        &password,
        "https://ani.iloader.app",
        install_profile,
        move || {
            let slot = Arc::clone(&two_fa_slot);
            let handle = app_for_2fa.clone();
            async move {
                let (tx, rx) = oneshot::channel();
                *slot.lock().await = Some(tx);
                let _ = handle.emit("two-factor-required", ());
                rx.await.unwrap_or_default()
            }
        },
        move |pct| {
            let scaled = 30.0 + (pct * 0.7);
            let _ = app_for_prog.emit(
                "sideload-progress",
                ProgressPayload {
                    stage: "Signing & installing via USB...".into(),
                    percent: scaled,
                },
            );
        },
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if sideload::is_app_id_limit_error(&msg) {
            format!("APP_ID_LIMIT:{msg}")
        } else {
            msg
        }
    })?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            two_factor_tx: Arc::new(Mutex::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            get_latest_release,
            list_usb_devices,
            start_install,
            submit_two_factor_code
        ])
        .run(tauri::generate_context!())
        .expect("error while running FlowDownload Installer");
}
