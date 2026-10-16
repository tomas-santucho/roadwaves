use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Debug, Clone, Serialize)]
pub struct GameInstall {
    pub profile: PathBuf,
    pub radio_file: PathBuf,
    pub mode: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct IntegrationReport {
    pub changed: bool,
    pub backup: Option<PathBuf>,
    pub message: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct PluginInstallReport {
    pub changed: bool,
    pub destination: PathBuf,
    pub backup: Option<PathBuf>,
    pub message: String,
}

pub struct SiiEditor;
impl SiiEditor {
    pub fn install(path: &Path, endpoint: &str) -> io::Result<IntegrationReport> {
        validate_endpoint(endpoint)?;
        let original_bytes = fs::read(path)?;
        let original = String::from_utf8(original_bytes.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "live_streams.sii is not UTF-8 text",
            )
        })?;
        if original.contains("RoadWaves AutoTune") {
            return Ok(IntegrationReport {
                changed: false,
                backup: None,
                message: "RoadWaves entry already present".into(),
            });
        }
        let backup = path.with_extension("sii.roadwaves.bak");
        let had_original = path.exists();
        if had_original {
            fs::copy(path, &backup)?;
        }
        let newline = if original.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let entry = format!(
            "stream_data[{}]: \"{}|RoadWaves AutoTune|Local|EN|128|0\"{newline}",
            next_index(&original),
            endpoint
        );
        let updated = if let Some(pos) = insertion_position(&original) {
            format!("{}{}{}", &original[..pos], entry, &original[pos..])
        } else {
            format!("SiiNunit{newline}{{{newline}{entry}}}{newline}")
        };
        atomic_write_checked(path, updated.as_bytes(), &original_bytes)?;
        Ok(IntegrationReport {
            changed: true,
            backup: had_original.then_some(backup),
            message: "Installed RoadWaves AutoTune; existing radio entries were preserved".into(),
        })
    }

    pub fn install_plugin(source: &Path, game_bin: &Path) -> io::Result<PluginInstallReport> {
        if !source.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "telemetry plugin source does not exist",
            ));
        }
        let source_extension = source
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "telemetry plugin source must have a .dll or .so extension",
                )
            })?;
        if source_extension != "dll" && source_extension != "so" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "telemetry plugin source must have a .dll or .so extension",
            ));
        }
        if let Some(target_name) = game_bin.file_name().and_then(|value| value.to_str()) {
            let expected_extension = match target_name.to_ascii_lowercase().as_str() {
                "win_x64" => Some("dll"),
                "linux_x64" => Some("so"),
                _ => None,
            };
            if expected_extension.is_some_and(|expected| expected != source_extension) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "plugin format .{source_extension} does not match ETS2 target {target_name}"
                    ),
                ));
            }
        }
        let plugins = game_bin.join("plugins");
        fs::create_dir_all(&plugins)?;
        let extension = source_extension.as_str();
        let destination = plugins.join(format!("roadwaves_scs.{extension}"));
        let backup = if destination.exists() {
            let path = destination.with_extension(format!("{extension}.roadwaves.bak"));
            fs::copy(&destination, &path)?;
            Some(path)
        } else {
            None
        };
        let temporary = destination.with_extension(format!("{extension}.roadwaves.tmp"));
        fs::copy(source, &temporary)?;
        if destination.exists() {
            fs::remove_file(&destination)?;
        }
        fs::rename(&temporary, &destination)?;
        Ok(PluginInstallReport {
            changed: true,
            destination,
            backup,
            message:
                "Installed only the RoadWaves SCS plugin; existing plugin files were preserved"
                    .into(),
        })
    }

    pub fn install_integration(
        source: &Path,
        game_bin: &Path,
        radio_file: &Path,
        endpoint: &str,
    ) -> io::Result<(PluginInstallReport, IntegrationReport)> {
        let plugin = Self::install_plugin(source, game_bin)?;
        match Self::install(radio_file, endpoint) {
            Ok(radio) => Ok((plugin, radio)),
            Err(error) => {
                if let Err(rollback_error) = rollback_plugin_install(&plugin) {
                    return Err(io::Error::new(
                        error.kind(),
                        format!("{error}; plugin rollback failed: {rollback_error}"),
                    ));
                }
                Err(error)
            }
        }
    }

    pub fn uninstall_plugin(game_bin: &Path) -> io::Result<PluginInstallReport> {
        let plugins = game_bin.join("plugins");
        let preferred_names = match game_bin
            .file_name()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("linux_x64") => ["roadwaves_scs.so", "roadwaves_scs.dll"],
            _ => ["roadwaves_scs.dll", "roadwaves_scs.so"],
        };
        let destination = preferred_names
            .iter()
            .map(|name| plugins.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| plugins.join(preferred_names[0]));
        if !destination.is_file() {
            return Ok(PluginInstallReport {
                changed: false,
                destination,
                backup: None,
                message: "RoadWaves plugin not found".into(),
            });
        }
        let backup = destination.with_extension(
            destination
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}.roadwaves.uninstall.bak"))
                .unwrap_or_else(|| "roadwaves.uninstall.bak".into()),
        );
        fs::copy(&destination, &backup)?;
        fs::remove_file(&destination)?;
        Ok(PluginInstallReport {
            changed: true,
            destination,
            backup: Some(backup),
            message: "Removed only the RoadWaves telemetry plugin; a backup was retained".into(),
        })
    }

    pub fn uninstall_integration(
        radio_file: &Path,
        game_bin: &Path,
    ) -> io::Result<(IntegrationReport, PluginInstallReport)> {
        let radio = Self::uninstall(radio_file)?;
        match Self::uninstall_plugin(game_bin) {
            Ok(plugin) => Ok((radio, plugin)),
            Err(error) => {
                if let Some(backup) = &radio.backup {
                    if let Err(rollback_error) = fs::copy(backup, radio_file) {
                        return Err(io::Error::new(
                            error.kind(),
                            format!("{error}; radio rollback failed: {rollback_error}"),
                        ));
                    }
                }
                Err(error)
            }
        }
    }

    pub fn uninstall(path: &Path) -> io::Result<IntegrationReport> {
        let original_bytes = fs::read(path)?;
        let original = String::from_utf8(original_bytes.clone()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "live_streams.sii is not UTF-8 text",
            )
        })?;
        let mut updated = String::with_capacity(original.len());
        let mut changed = false;
        for segment in original.split_inclusive('\n') {
            let line = segment.trim_end_matches(['\r', '\n']);
            if line.contains("|RoadWaves AutoTune|") {
                changed = true;
            } else {
                updated.push_str(segment);
            }
        }
        if !changed {
            return Ok(IntegrationReport {
                changed: false,
                backup: None,
                message: "RoadWaves entry not found".into(),
            });
        }
        let backup = path.with_extension("sii.roadwaves.uninstall.bak");
        fs::copy(path, &backup)?;
        atomic_write_checked(path, updated.as_bytes(), &original_bytes)?;
        Ok(IntegrationReport {
            changed: true,
            backup: Some(backup),
            message: "Removed only the RoadWaves radio entry".into(),
        })
    }
}

fn rollback_plugin_install(report: &PluginInstallReport) -> io::Result<()> {
    if let Some(backup) = &report.backup {
        fs::copy(backup, &report.destination)?;
    } else if report.destination.exists() {
        fs::remove_file(&report.destination)?;
    }
    Ok(())
}

fn next_index(text: &str) -> usize {
    text.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("stream_data[")?
                .split(']')
                .next()?
                .parse::<usize>()
                .ok()
        })
        .max()
        .map_or(0, |n| n + 1)
}
fn insertion_position(text: &str) -> Option<usize> {
    let marker = "live_stream_def : .live_streams";
    if let Some(marker_start) = text.find(marker) {
        let open = text[marker_start..].find('{')? + marker_start;
        let mut depth = 0usize;
        for (offset, byte) in text.as_bytes()[open..].iter().enumerate() {
            match byte {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        return Some(open + offset);
                    }
                }
                _ => {}
            }
        }
    }
    text.rfind('}')
}
fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("sii.roadwaves.tmp");
    fs::write(&tmp, data)?;
    fs::rename(tmp, path)
}

fn atomic_write_checked(path: &Path, data: &[u8], expected: &[u8]) -> io::Result<()> {
    if fs::read(path)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "live_streams.sii changed while RoadWaves was preparing the update",
        ));
    }
    atomic_write(path, data)
}

fn validate_endpoint(endpoint: &str) -> io::Result<()> {
    if endpoint
        .chars()
        .any(|character| matches!(character, '\r' | '\n' | '"' | '|'))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "radio endpoint contains forbidden SII characters",
        ));
    }
    let url = Url::parse(endpoint)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "radio endpoint is not a URL"))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "radio endpoint must be an HTTP(S) URL",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn install_is_idempotent_and_uninstall_preserves_other_entries() {
        let path = std::env::temp_dir().join(format!("roadwaves-test-{}.sii", std::process::id()));
        fs::write(
            &path,
            "SiiNunit\n{\nstream_data[3]: \"http://example|Other|Local|EN|128|0\"\n}\n",
        )
        .unwrap();
        assert!(
            SiiEditor::install(&path, "http://127.0.0.1:47926/radio.mp3")
                .unwrap()
                .changed
        );
        assert!(
            !SiiEditor::install(&path, "http://127.0.0.1:47926/radio.mp3")
                .unwrap()
                .changed
        );
        assert!(SiiEditor::uninstall(&path).unwrap().changed);
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Other"));
        assert!(!contents.contains("RoadWaves AutoTune"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn install_preserves_crlf_line_endings() {
        let path =
            std::env::temp_dir().join(format!("roadwaves-crlf-install-{}.sii", std::process::id()));
        let original = "SiiNunit\r\n{\r\nlive_stream_def : .live_streams {\r\n}\r\n}\r\n";
        fs::write(&path, original).unwrap();
        SiiEditor::install(&path, "http://127.0.0.1:47926/radio.mp3").unwrap();
        let updated = fs::read(&path).unwrap();
        assert!(updated
            .iter()
            .enumerate()
            .all(|(index, byte)| { *byte != b'\n' || (index > 0 && updated[index - 1] == b'\r') }));
        assert_eq!(
            updated.iter().filter(|byte| **byte == b'\r').count(),
            updated.iter().filter(|byte| **byte == b'\n').count()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn plugin_install_is_scoped_and_backups_existing_plugin() {
        let root = std::env::temp_dir().join(format!("roadwaves-plugin-{}", std::process::id()));
        let source = root.join("source.dll");
        let game_bin = root.join("bin/win_x64");
        fs::create_dir_all(&game_bin).unwrap();
        fs::write(&source, b"new plugin").unwrap();
        let first = SiiEditor::install_plugin(&source, &game_bin).unwrap();
        assert!(first.changed);
        assert!(first.destination.is_file());
        fs::write(&source, b"updated plugin").unwrap();
        let second = SiiEditor::install_plugin(&source, &game_bin).unwrap();
        assert!(second.backup.is_some());
        assert_eq!(fs::read(&second.destination).unwrap(), b"updated plugin");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linux_plugin_source_keeps_so_extension() {
        let root = std::env::temp_dir().join(format!("roadwaves-plugin-so-{}", std::process::id()));
        let source = root.join("roadwaves_scs.so");
        let game_bin = root.join("bin/linux_x64");
        fs::create_dir_all(&game_bin).unwrap();
        fs::write(&source, b"linux plugin").unwrap();
        let report = SiiEditor::install_plugin(&source, &game_bin).unwrap();
        assert_eq!(report.destination.file_name().unwrap(), "roadwaves_scs.so");
        fs::write(&source, b"updated linux plugin").unwrap();
        let replacement = SiiEditor::install_plugin(&source, &game_bin).unwrap();
        assert_eq!(
            replacement.backup.unwrap().file_name().unwrap(),
            "roadwaves_scs.so.roadwaves.bak"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_format_cannot_be_installed_into_the_wrong_ets2_target() {
        let root =
            std::env::temp_dir().join(format!("roadwaves-plugin-format-{}", std::process::id()));
        let source = root.join("roadwaves_scs.so");
        let game_bin = root.join("bin/win_x64");
        fs::create_dir_all(&game_bin).unwrap();
        fs::write(&source, b"linux plugin").unwrap();
        let error = SiiEditor::install_plugin(&source, &game_bin).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!game_bin.join("plugins/roadwaves_scs.so").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn combined_install_rolls_back_plugin_when_radio_install_fails() {
        let root = std::env::temp_dir().join(format!(
            "roadwaves-integration-rollback-{}",
            std::process::id()
        ));
        let source = root.join("roadwaves_scs.dll");
        let game_bin = root.join("bin/win_x64");
        let radio = root.join("missing/live_streams.sii");
        fs::create_dir_all(&game_bin).unwrap();
        fs::write(&source, b"new plugin").unwrap();
        let error = SiiEditor::install_integration(
            &source,
            &game_bin,
            &radio,
            "http://127.0.0.1:47926/radio.mp3",
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(!game_bin.join("plugins/roadwaves_scs.dll").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn combined_repair_restores_existing_plugin_when_radio_install_fails() {
        let root = std::env::temp_dir().join(format!(
            "roadwaves-integration-repair-rollback-{}",
            std::process::id()
        ));
        let source = root.join("roadwaves_scs.dll");
        let game_bin = root.join("bin/win_x64");
        let radio = root.join("missing/live_streams.sii");
        fs::create_dir_all(game_bin.join("plugins")).unwrap();
        fs::write(&source, b"new plugin").unwrap();
        fs::write(game_bin.join("plugins/roadwaves_scs.dll"), b"old plugin").unwrap();
        let _ = SiiEditor::install_integration(
            &source,
            &game_bin,
            &radio,
            "http://127.0.0.1:47926/radio.mp3",
        )
        .unwrap_err();
        assert_eq!(
            fs::read(game_bin.join("plugins/roadwaves_scs.dll")).unwrap(),
            b"old plugin"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn combined_uninstall_does_not_remove_plugin_when_radio_uninstall_fails() {
        let root = std::env::temp_dir().join(format!(
            "roadwaves-integration-uninstall-rollback-{}",
            std::process::id()
        ));
        let radio = root.join("missing/live_streams.sii");
        let game_bin = root.join("bin/win_x64");
        let plugin = game_bin.join("plugins/roadwaves_scs.dll");
        fs::create_dir_all(plugin.parent().unwrap()).unwrap();
        fs::write(&plugin, b"plugin").unwrap();
        let error = SiiEditor::uninstall_integration(&radio, &game_bin).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(plugin.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_uninstall_only_removes_roadwaves_file_and_keeps_backup() {
        let root =
            std::env::temp_dir().join(format!("roadwaves-plugin-remove-{}", std::process::id()));
        let source = root.join("roadwaves_scs.dll");
        let game_bin = root.join("bin/win_x64");
        fs::create_dir_all(&game_bin).unwrap();
        fs::write(&source, b"plugin").unwrap();
        SiiEditor::install_plugin(&source, &game_bin).unwrap();
        fs::write(game_bin.join("plugins/other.dll"), b"keep").unwrap();
        let result = SiiEditor::uninstall_plugin(&game_bin).unwrap();
        assert!(result.changed);
        assert!(result.backup.unwrap().is_file());
        assert!(!game_bin.join("plugins/roadwaves_scs.dll").exists());
        assert!(game_bin.join("plugins/other.dll").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_uninstall_uses_the_selected_target_format() {
        let root = std::env::temp_dir().join(format!(
            "roadwaves-plugin-format-remove-{}",
            std::process::id()
        ));
        let game_bin = root.join("bin/linux_x64");
        let plugins = game_bin.join("plugins");
        fs::create_dir_all(&plugins).unwrap();
        fs::write(plugins.join("roadwaves_scs.dll"), b"windows plugin").unwrap();
        fs::write(plugins.join("roadwaves_scs.so"), b"linux plugin").unwrap();
        let result = SiiEditor::uninstall_plugin(&game_bin).unwrap();
        assert_eq!(result.destination, plugins.join("roadwaves_scs.so"));
        assert!(!plugins.join("roadwaves_scs.so").exists());
        assert!(plugins.join("roadwaves_scs.dll").is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn actual_ets2_template_gets_entry_inside_live_stream_object() {
        let path =
            std::env::temp_dir().join(format!("roadwaves-template-{}.sii", std::process::id()));
        fs::write(
            &path,
            "SiiNunit\n{\nlive_stream_def : .live_streams {\n# comment\n}\n}\n",
        )
        .unwrap();
        SiiEditor::install(&path, "http://127.0.0.1:47926/radio.mp3").unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        let object_end = contents.find("\n}\n}\n").unwrap();
        assert!(contents[..object_end].contains("RoadWaves AutoTune"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn install_rejects_missing_or_malformed_files_and_bad_endpoints() {
        let missing =
            std::env::temp_dir().join(format!("roadwaves-missing-{}.sii", std::process::id()));
        assert_eq!(
            SiiEditor::install(&missing, "http://127.0.0.1:47926/radio.mp3")
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
        let path =
            std::env::temp_dir().join(format!("roadwaves-invalid-{}.sii", std::process::id()));
        fs::write(&path, b"SiiNunit\n{\n}\n").unwrap();
        assert_eq!(
            SiiEditor::install(&path, "file:///unsafe")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            SiiEditor::install(&path, "http://127.0.0.1:47926/radio.mp3|bad")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn uninstall_preserves_crlf_and_unrelated_bytes() {
        let path = std::env::temp_dir().join(format!(
            "roadwaves-crlf-uninstall-{}.sii",
            std::process::id()
        ));
        let original = "SiiNunit\r\n{\r\n# keep this comment\r\nstream_data[1]: \"http://other|Other|Local|EN|128|0\"\r\nstream_data[2]: \"http://127.0.0.1:47926/radio.mp3|RoadWaves AutoTune|Local|EN|128|0\"\r\n}\r\n";
        fs::write(&path, original).unwrap();
        assert!(SiiEditor::uninstall(&path).unwrap().changed);
        let updated = fs::read_to_string(&path).unwrap();
        assert_eq!(
            updated,
            "SiiNunit\r\n{\r\n# keep this comment\r\nstream_data[1]: \"http://other|Other|Local|EN|128|0\"\r\n}\r\n"
        );
        let _ = fs::remove_file(path);
    }
}
