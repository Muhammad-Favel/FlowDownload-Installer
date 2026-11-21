#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use flowdownload_installer::feed::{
    download_ipa_with_progress, fetch_latest_release, AltVersion, DEFAULT_SOURCE_URL,
};
use flowdownload_installer::ipa::prepare_ipa_for_profile;
use flowdownload_installer::profile::InstallProfile;
use flowdownload_installer::sideload::{self, UsbDeviceSummary};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
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

fn emit_log(app: &AppHandle, log_path: Option<&PathBuf>, line: &str) {
    let _ = app.emit("sideload-log", line.to_string());
    if let Some(path) = log_path {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
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

    let log_path = app.path().app_log_dir().ok().map(|dir| {
        let _ = fs::create_dir_all(&dir);
        dir.join("installer.log")
    });

    emit_log(&app, log_path.as_ref(), &format!("Starting install (profile: {profile})"));
    let _ = app.emit(
        "sideload-progress",
        ProgressPayload {
            stage: "Fetching release...".into(),
            percent: 3.0,
        },
    );

    let (_meta, latest) = fetch_latest_release(DEFAULT_SOURCE_URL).await.map_err(|e| {
        let msg = format!("Release check failed: {e}");
        emit_log(&app, log_path.as_ref(), &msg);
        msg
    })?;

    emit_log(
        &app,
        log_path.as_ref(),
        &format!("Latest release: v{} ({:.1} MB)", latest.version, latest.size as f64 / 1_000_000.0),
    );

    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("FlowDownload");

    let app_dl = app.clone();
    let mut last_pct: i32 = -1;
    let raw_ipa = download_ipa_with_progress(&latest, &cache_dir, |done, total| {
        let ratio = (done as f64 / total.max(1) as f64).clamp(0.0, 1.0);
        let pct = (5.0 + ratio * 20.0) as f32;
        let rounded = pct as i32;
        if rounded != last_pct {
            last_pct = rounded;
            let _ = app_dl.emit(
                "sideload-progress",
                ProgressPayload {
                    stage: format!(
                        "Downloading IPA ({:.1} / {:.1} MB)...",
                        done as f64 / 1_000_000.0,
                        total as f64 / 1_000_000.0
                    ),
                    percent: pct,
                },
            );
        }
    })
    .await
    .map_err(|e| {
        let msg = format!("IPA download failed: {e}");
        emit_log(&app, log_path.as_ref(), &msg);
        msg
    })?;

    emit_log(&app, log_path.as_ref(), &format!("IPA ready: {}", raw_ipa.display()));
    let _ = app.emit(
        "sideload-progress",
        ProgressPayload {
            stage: "Preparing app bundle...".into(),
            percent: 27.0,
        },
    );

    let raw_clone = raw_ipa.clone();
    let cache_clone = cache_dir.clone();
    let prepared_ipa = tokio::task::spawn_blocking(move || {
        prepare_ipa_for_profile(&raw_clone, install_profile, &cache_clone)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| {
        let msg = format!("Bundle preparation failed: {e}");
        emit_log(&app, log_path.as_ref(), &msg);
        msg
    })?;

    let two_fa_slot = Arc::clone(&state.two_factor_tx);
    let app_for_2fa = app.clone();
    let app_for_prog = app.clone();
    let app_for_log = app.clone();
    let log_path_clone = log_path.clone();

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
        move |line| {
            emit_log(&app_for_log, log_path_clone.as_ref(), line);
        },
    )
    .await
    .map_err(|e| {
        let msg = e.to_string();
        emit_log(&app, log_path.as_ref(), &format!("ERROR: {msg}"));
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
        .setup(|app| {
            if let Ok(data_dir) = app.path().app_data_dir() {
                let _ = fs::create_dir_all(&data_dir);
                let _ = std::env::set_current_dir(&data_dir);
            }
            Ok(())
        })
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
