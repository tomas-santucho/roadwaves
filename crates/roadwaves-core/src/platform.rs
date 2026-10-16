use crate::game::GameInstall;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Finds likely ETS2 radio profiles without modifying any user files.
/// Steam library parsing is deliberately conservative: only quoted `path` values are used.
pub fn discover_ets2_profiles() -> Vec<GameInstall> {
    let roots = discovery_roots();

    let mut profiles = Vec::new();
    for root in roots {
        add_native_profile(&mut profiles, &root);
        for library in steam_libraries(&root) {
            let game_root = library.join("steamapps/common/Euro Truck Simulator 2");
            add_native_profile(&mut profiles, &game_root);
            add_proton_profiles(&mut profiles, &library);
        }
    }
    profiles.sort_by(|a, b| a.radio_file.cmp(&b.radio_file));
    profiles.dedup_by(|a, b| a.radio_file == b.radio_file);
    profiles
}

pub fn discover_ets2_game_bins() -> Vec<PathBuf> {
    let roots = discovery_roots();
    let mut bins = Vec::new();
    for root in roots {
        for library in steam_libraries(&root) {
            let game = library.join("steamapps/common/Euro Truck Simulator 2");
            for (directory, executable) in [
                ("bin/win_x64", "eurotrucks2.exe"),
                ("bin/linux_x64", "eurotrucks2"),
            ] {
                let bin = game.join(directory);
                if bin.join(executable).is_file() {
                    bins.push(bin);
                }
            }
        }
    }
    bins.sort();
    bins.dedup();
    bins
}

fn discovery_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for variable in ["USERPROFILE", "HOME", "OneDrive", "XDG_DATA_HOME"] {
        if let Some(value) = env::var_os(variable) {
            roots.push(PathBuf::from(value));
        }
    }
    if let Some(home) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        roots.extend([
            home.join(".steam/steam"),
            home.join(".local/share/Steam"),
            home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
        ]);
    }
    roots.extend([
        PathBuf::from(r"C:\Program Files (x86)\Steam"),
        PathBuf::from(r"C:\Program Files\Steam"),
    ]);
    roots.sort();
    roots.dedup();
    roots
}

fn add_native_profile(out: &mut Vec<GameInstall>, root: &Path) {
    for base in [
        root.join("Documents"),
        root.join("My Documents"),
        root.join(".local/share"),
        root.join(".var/app/com.valvesoftware.Steam/data/documents"),
        root.to_path_buf(),
    ] {
        let radio = base.join("Euro Truck Simulator 2/live_streams.sii");
        if radio.is_file() {
            out.push(GameInstall {
                profile: base.join("Euro Truck Simulator 2"),
                radio_file: radio,
                mode: "native-windows-or-documents".into(),
            });
        }
    }
}
fn add_proton_profile(out: &mut Vec<GameInstall>, documents: PathBuf) {
    let radio = documents.join("Euro Truck Simulator 2/live_streams.sii");
    if radio.is_file() {
        out.push(GameInstall {
            profile: documents.join("Euro Truck Simulator 2"),
            radio_file: radio,
            mode: "proton-windows-profile".into(),
        });
    }
}

fn add_proton_profiles(out: &mut Vec<GameInstall>, library: &Path) {
    let compatdata = library.join("steamapps/compatdata/227300/pfx/drive_c/users");
    let Ok(users) = fs::read_dir(compatdata) else {
        return;
    };
    for user in users.flatten() {
        let home = user.path();
        // ETS2 has used both Windows Documents names depending on the
        // prefix/profile history. Check both without assuming which one the
        // current Proton prefix exposes.
        for documents_name in ["Documents", "My Documents"] {
            add_proton_profile(out, home.join(documents_name));
        }
    }
}
fn steam_libraries(root: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let vdf = root.join("steamapps/libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(vdf) else {
        return result;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        let fields: Vec<_> = trimmed.split('"').collect();
        let Some(index) = fields.iter().position(|field| *field == "path") else {
            continue;
        };
        if let Some(value) = fields.get(index + 2) {
            result.push(PathBuf::from(value.replace("\\\\", "\\")));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vdf_parser_accepts_windows_paths() {
        let root = env::temp_dir().join(format!("roadwaves-steam-{}", std::process::id()));
        let steamapps = root.join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();
        fs::write(
            steamapps.join("libraryfolders.vdf"),
            "\"libraryfolders\" {\n \"0\" { \"path\" \"C:\\\\SteamLibrary\" }\n}\n",
        )
        .unwrap();
        let found = steam_libraries(&root);
        assert_eq!(found, vec![PathBuf::from(r"C:\SteamLibrary")]);
        let _ = fs::remove_file(steamapps.join("libraryfolders.vdf"));
        let _ = fs::remove_dir(steamapps);
        let _ = fs::remove_dir(root);
    }

    #[test]
    fn proton_profile_is_explicitly_classified() {
        let root = env::temp_dir().join(format!("roadwaves-proton-{}", std::process::id()));
        let radio = root.join("Euro Truck Simulator 2/live_streams.sii");
        fs::create_dir_all(radio.parent().unwrap()).unwrap();
        fs::write(&radio, "SiiNunit\n{\n}\n").unwrap();
        let mut found = Vec::new();
        add_proton_profile(&mut found, root.clone());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mode, "proton-windows-profile");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn proton_profiles_support_both_windows_documents_names() {
        let root = env::temp_dir().join(format!("roadwaves-proton-docs-{}", std::process::id()));
        let compat_users = root.join("steamapps/compatdata/227300/pfx/drive_c/users/steamuser");
        for documents_name in ["Documents", "My Documents"] {
            let radio = compat_users
                .join(documents_name)
                .join("Euro Truck Simulator 2/live_streams.sii");
            fs::create_dir_all(radio.parent().unwrap()).unwrap();
            fs::write(radio, "SiiNunit\n{\n}\n").unwrap();
        }
        let mut found = Vec::new();
        add_proton_profiles(&mut found, &root);
        assert_eq!(found.len(), 2);
        assert!(found
            .iter()
            .all(|profile| profile.mode == "proton-windows-profile"));
        let _ = fs::remove_dir_all(root);
    }
}
