use crate::profile::InstallProfile;
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Debug)]
pub struct IpaInspection {
    pub app_bundle_name: String,
    pub bundle_id: Option<String>,
    pub plugins: Vec<String>,
}

pub fn inspect_ipa(ipa_path: &Path) -> Result<IpaInspection> {
    let file = File::open(ipa_path).with_context(|| format!("Cannot open {}", ipa_path.display()))?;
    let mut archive = ZipArchive::new(file)?;

    let mut app_bundle_name = String::from("FlowDownload.app");
    let mut plugins = BTreeSet::new();
    let mut bundle_id = None;

    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_string();

        if let Some(rest) = name.strip_prefix("Payload/") {
            if let Some(app_end) = rest.find(".app/") {
                app_bundle_name = rest[..app_end + 4].to_string();
                let inner = &rest[app_end + 5..];

                if inner == "Info.plist" {
                    let mut buf = Vec::new();
                    let mut entry = entry;
                    if entry.read_to_end(&mut buf).is_ok() {
                        if let Ok(val) = plist::from_bytes::<plist::Value>(&buf) {
                            bundle_id = val
                                .as_dictionary()
                                .and_then(|d| d.get("CFBundleIdentifier"))
                                .and_then(|v| v.as_string())
                                .map(|s| s.to_string());
                        }
                    }
                    continue;
                }

                if let Some(plugin_rest) = inner.strip_prefix("PlugIns/") {
                    if let Some(appex_end) = plugin_rest.find(".appex/") {
                        plugins.insert(plugin_rest[..appex_end + 6].to_string());
                    }
                }
            }
        }
    }

    Ok(IpaInspection {
        app_bundle_name,
        bundle_id,
        plugins: plugins.into_iter().collect(),
    })
}

/// Writes a trimmed IPA excluding extensions not included in `profile`.
/// For `InstallProfile::Full`, returns the original path unchanged.
pub fn prepare_ipa_for_profile(ipa_path: &Path, profile: InstallProfile, work_dir: &Path) -> Result<PathBuf> {
    let excluded = profile.excluded_appex_dirs();
    if excluded.is_empty() {
        return Ok(ipa_path.to_path_buf());
    }

    fs::create_dir_all(work_dir)?;
    let out_name = match profile {
        InstallProfile::Full => "FlowDownload-Full.ipa",
        InstallProfile::ShareOnly => "FlowDownload-ShareOnly.ipa",
        InstallProfile::Core => "FlowDownload-Core.ipa",
    };
    let out_path = work_dir.join(out_name);

    let in_file = File::open(ipa_path)?;
    let mut reader = ZipArchive::new(in_file)?;

    let out_file = File::create(&out_path)?;
    let mut writer = ZipWriter::new(out_file);

    for i in 0..reader.len() {
        let mut entry = reader.by_index(i)?;
        let name = entry.name().to_string();

        let should_skip = excluded.iter().any(|appex| {
            name.contains(&format!("/PlugIns/{appex}/")) || name.ends_with(&format!("/PlugIns/{appex}"))
        });
        if should_skip {
            continue;
        }

        let options = SimpleFileOptions::default()
            .compression_method(entry.compression())
            .unix_permissions(entry.unix_mode().unwrap_or(0o644));

        if entry.is_dir() {
            writer.add_directory(name, options)?;
        } else {
            writer.start_file(name, options)?;
            io::copy(&mut entry, &mut writer)?;
        }
    }

    writer.finish()?;
    Ok(out_path)
}
