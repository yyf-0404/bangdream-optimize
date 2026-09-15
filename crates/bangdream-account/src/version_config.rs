//! Only public client metadata is persisted. No account or session data belongs here.
use crate::{apk::ClientVersion, credentials::fail, ImportError};
use rand::{rngs::OsRng, RngCore};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) const DEFAULT_PATH: &str = "var/bangdream-account/client-version.json";
const EMBEDDED: &str = include_str!("../default-client-version.json");

#[derive(Clone)]
pub(crate) struct VersionConfig {
    path: PathBuf,
}
impl Default for VersionConfig {
    fn default() -> Self {
        Self::new(DEFAULT_PATH)
    }
}
impl VersionConfig {
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub(crate) fn load(&self) -> Result<ClientVersion, ImportError> {
        let mut data = String::new();
        match File::open(&self.path) {
            Ok(file) => {
                file.take(4097)
                    .read_to_string(&mut data)
                    .map_err(|_| fail("无法读取国服客户端版本配置"))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => data = EMBEDDED.to_owned(),
            Err(_) => return Err(fail("无法读取国服客户端版本配置")),
        }
        if data.len() > 4096 {
            return Err(fail("国服客户端版本配置过大"));
        }
        let version: ClientVersion =
            serde_json::from_str(&data).map_err(|_| fail("国服客户端版本配置格式异常"))?;
        version.validate()?;
        Ok(version)
    }
    pub(crate) fn save(&self, version: &ClientVersion) -> Result<(), ImportError> {
        version.validate()?;
        let data = serde_json::to_vec_pretty(version).map_err(|_| fail("国服版本配置生成失败"))?;
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)
            .map_err(|_| fail("无法保存国服默认版本配置，请检查配置目录的写入权限"))?;
        let temp = parent.join(format!(".client-version-{:016x}.tmp", OsRng.next_u64()));
        let result = (|| -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            file.write_all(&data)?;
            file.write_all(b"\n")?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
            return Err(fail("无法保存国服默认版本配置，请检查配置文件的写入权限"));
        }
        Ok(())
    }
    // APK discovery is only a fallback for configuration/preflight failure, before credentials are used.
    pub(crate) fn resolve<T>(
        &self,
        mut validate: impl FnMut(&ClientVersion) -> Result<T, ImportError>,
        discover: impl FnOnce() -> Result<ClientVersion, ImportError>,
    ) -> Result<(ClientVersion, T), ImportError> {
        if let Ok(version) = self.load() {
            if let Ok(application) = validate(&version) {
                return Ok((version, application));
            }
        }
        let version = discover()?;
        version.validate()?;
        let application = validate(&version)?;
        // Commit only verified metadata. An unsuccessful refresh must not replace a working default.
        self.save(&version)?;
        Ok((version, application))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let dir =
                std::env::temp_dir().join(format!("cn-version-test-{:016x}", OsRng.next_u64()));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn config(&self) -> VersionConfig {
            VersionConfig::new(self.0.join("client-version.json"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(self.0.join("client-version.json"));
            let _ = fs::remove_file(self.0.join("not-a-directory"));
            let _ = fs::remove_dir(&self.0);
        }
    }
    fn newer() -> ClientVersion {
        ClientVersion {
            client: "9.4.5".into(),
            code: "106".into(),
            unity: "2022.3.62f3c1".into(),
        }
    }
    #[test]
    fn embedded_config_works_without_apk_or_a_disk_write() {
        let f = Fixture::new();
        let config = f.config();
        let (version, ()) = config
            .resolve(|_| Ok(()), || panic!("APK should not be fetched"))
            .unwrap();
        assert_eq!(version.client, "9.4.4");
        assert_eq!(version.code, "105");
        assert!(!config.path.exists());
    }
    #[test]
    fn stale_config_refreshes_once_and_is_reused_after_restart() {
        let f = Fixture::new();
        let config = f.config();
        config.save(&config.load().unwrap()).unwrap();
        let calls = Cell::new(0);
        let (actual, ()) = config
            .resolve(
                |v| {
                    calls.set(calls.get() + 1);
                    if v == &newer() {
                        Ok(())
                    } else {
                        Err(fail("outdated"))
                    }
                },
                || Ok(newer()),
            )
            .unwrap();
        assert_eq!(actual, newer());
        assert_eq!(calls.get(), 2);
        assert_eq!(f.config().load().unwrap(), newer());
        f.config()
            .resolve(|_| Ok(()), || panic!("updated file must be preferred"))
            .unwrap();
        let saved = fs::read_to_string(&config.path).unwrap();
        let json: serde_json::Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 3);
    }
    #[test]
    fn failed_refresh_keeps_previous_defaults() {
        let f = Fixture::new();
        let config = f.config();
        config.save(&config.load().unwrap()).unwrap();
        let before = fs::read(&config.path).unwrap();
        assert!(config
            .resolve::<()>(|_| Err(fail("maintenance")), || Ok(newer()))
            .is_err());
        assert_eq!(fs::read(&config.path).unwrap(), before);
        assert!(config
            .resolve::<()>(|_| Err(fail("outdated")), || Err(fail("APK unavailable")))
            .is_err());
        assert_eq!(fs::read(&config.path).unwrap(), before);
    }
    #[test]
    fn invalid_local_config_recovers_using_verified_metadata() {
        let f = Fixture::new();
        let config = f.config();
        fs::write(&config.path, b"invalid").unwrap();
        config.resolve(|_| Ok(()), || Ok(newer())).unwrap();
        assert_eq!(config.load().unwrap(), newer());
    }
    #[test]
    fn save_failure_does_not_return_an_unpersisted_default() {
        let f = Fixture::new();
        let parent = f.0.join("not-a-directory");
        fs::write(&parent, b"file").unwrap();
        let config = VersionConfig::new(parent.join("client-version.json"));
        assert!(config
            .resolve(
                |version| {
                    if version == &newer() {
                        Ok(())
                    } else {
                        Err(fail("outdated"))
                    }
                },
                || Ok(newer())
            )
            .is_err());
    }
}
