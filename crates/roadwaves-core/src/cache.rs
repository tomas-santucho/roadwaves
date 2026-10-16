use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const CACHE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEnvelope<T> {
    schema_version: u32,
    fetched_unix_secs: u64,
    expires_unix_secs: u64,
    value: T,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CacheValue<T> {
    pub value: T,
    pub stale: bool,
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn user_cache_dir() -> PathBuf {
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("roadwaves");
    }
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path).join("RoadWaves").join("cache");
    }
    if let Some(path) = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")) {
        return PathBuf::from(path).join(".cache").join("roadwaves");
    }
    PathBuf::from("roadwaves-cache")
}

pub fn load<T: DeserializeOwned>(path: &Path, now: u64) -> io::Result<CacheValue<T>> {
    let bytes = fs::read(path)?;
    let envelope: CacheEnvelope<T> = match serde_json::from_slice(&bytes) {
        Ok(envelope) => envelope,
        Err(error) => {
            preserve_corrupt(path, now);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid cache: {error}"),
            ));
        }
    };
    if envelope.schema_version != CACHE_SCHEMA_VERSION {
        preserve_corrupt(path, now);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported cache schema",
        ));
    }
    Ok(CacheValue {
        stale: now > envelope.expires_unix_secs,
        value: envelope.value,
    })
}

fn preserve_corrupt(path: &Path, now: u64) {
    let corrupt = path.with_extension(format!("corrupt-{now}"));
    let _ = fs::rename(path, corrupt);
}

pub fn store<T: Serialize>(path: &Path, value: &T, ttl_secs: u64, now: u64) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let envelope = CacheEnvelope {
        schema_version: CACHE_SCHEMA_VERSION,
        fetched_unix_secs: now,
        expires_unix_secs: now.saturating_add(ttl_secs),
        value,
    };
    let bytes = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip_reports_expiration() {
        let path = env::temp_dir().join(format!("roadwaves-cache-{}.json", std::process::id()));
        store(&path, &vec!["place"], 10, 100).unwrap();
        assert!(!load::<Vec<String>>(&path, 105).unwrap().stale);
        assert!(load::<Vec<String>>(&path, 111).unwrap().stale);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn corrupt_cache_is_rejected() {
        let path = env::temp_dir().join(format!(
            "roadwaves-cache-corrupt-{}.json",
            std::process::id()
        ));
        fs::write(&path, b"not json").unwrap();
        assert_eq!(
            load::<Vec<String>>(&path, 0).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_file(path);
    }
}
