use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
pub enum InstallProfile {
    /// Main app + Safari Extension + Share Extension (3 App IDs)
    Full,
    /// Main app + Share Extension only (2 App IDs)
    ShareOnly,
    /// Main app only, no extensions (1 App ID)
    Core,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileReport {
    pub profile: InstallProfile,
    pub app_ids_required: usize,
    pub included_extensions: Vec<&'static str>,
    pub dropped_extensions: Vec<&'static str>,
    pub kept_features: Vec<&'static str>,
    pub trade_offs: Vec<&'static str>,
}

impl InstallProfile {
    pub fn excluded_appex_dirs(self) -> &'static [&'static str] {
        match self {
            InstallProfile::Full => &[],
            InstallProfile::ShareOnly => &["FlowSafariExtension.appex"],
            InstallProfile::Core => &["FlowSafariExtension.appex", "FlowShareExtension.appex"],
        }
    }

    pub fn report(self) -> ProfileReport {
        match self {
            InstallProfile::Full => ProfileReport {
                profile: self,
                app_ids_required: 3,
                included_extensions: vec!["FlowSafariExtension", "FlowShareExtension"],
                dropped_extensions: vec![],
                kept_features: vec![
                    "Wi-Fi + Cellular Boost (HTTP Range + libtorrent)",
                    "Native C++ libtorrent engine",
                    "Built-in AdGuard content-blocking browser",
                    "External USB drive staging & export",
                    "Automatic SHA-256 verification",
                    "Safari toolbar extension & Share Sheet handoff",
                ],
                trade_offs: vec![
                    "Uses all 3 App IDs on a free Apple ID (cannot coexist with other sideloaded apps like SideStore/AltStore).",
                ],
            },
            InstallProfile::ShareOnly => ProfileReport {
                profile: self,
                app_ids_required: 2,
                included_extensions: vec!["FlowShareExtension"],
                dropped_extensions: vec!["FlowSafariExtension"],
                kept_features: vec![
                    "All core FlowDownload features (Boost, libtorrent, AdGuard browser, USB export, SHA-256)",
                    "System-wide Share Sheet handoff (Safari / Files -> Share -> FlowDownload)",
                ],
                trade_offs: vec![
                    "Loses the Safari toolbar popup/extension button.",
                    "Requires at least 2 free App ID slots (max 1 other 1-ID app on a free Apple ID).",
                ],
            },
            InstallProfile::Core => ProfileReport {
                profile: self,
                app_ids_required: 1,
                included_extensions: vec![],
                dropped_extensions: vec!["FlowSafariExtension", "FlowShareExtension"],
                kept_features: vec![
                    "Wi-Fi + Cellular Boost (HTTP Range + libtorrent)",
                    "Native C++ libtorrent engine (magnet & .torrent files)",
                    "Built-in AdGuard content-blocking browser",
                    "External USB drive staging & export",
                    "Automatic SHA-256 verification",
                ],
                trade_offs: vec![
                    "Loses the Safari toolbar extension.",
                    "Loses Share Sheet handoff from other apps (paste links in FlowDownload, open .torrent from Files, or use the built-in browser instead).",
                ],
            },
        }
    }

    pub fn print_all_trade_offs() {
        println!("\n=== FlowDownload App ID Budget & Trade-Offs ===");
        println!("Free Apple IDs allow up to 3 active App IDs per device (10 per 7 days).\n");
        for profile in [InstallProfile::Full, InstallProfile::ShareOnly, InstallProfile::Core] {
            let r = profile.report();
            println!("[{:?}] — {} App ID(s)", r.profile, r.app_ids_required);
            if !r.dropped_extensions.is_empty() {
                println!("  Dropped: {}", r.dropped_extensions.join(", "));
            }
            for t in r.trade_offs {
                println!("  Trade-off: {t}");
            }
            println!();
        }
    }
}
