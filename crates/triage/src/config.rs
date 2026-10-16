//! Environment → [`Config`], and the platform directories in [`Paths`].
//!
//! This is the only module that reads `std::env`.

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use figment::Figment;
use figment::providers::Env;
use serde::Deserialize;
use url::Url;

/// Env vars that must be set before startup. M0 has none.
pub const REQUIRED: &[&str] = &[];

const ENV_PREFIX: &str = "TRIAGE_";
const DATA_DIR_VAR: &str = "TRIAGE_DATA_DIR";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("missing required environment variables: {}", .0.join(", "))]
    Missing(Vec<String>),
    #[error("invalid environment variables: {}", .0.join("; "))]
    Invalid(Vec<String>),
    #[error("cannot read .env: {0}")]
    Dotenv(#[from] dotenvy::Error),
    #[error("cannot find a home directory; pass --data-dir <path>")]
    NoHome,
    #[error("cannot create directory {}: {source}", path.display())]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("data directory {} is not writable: {source}", path.display())]
    NotWritable { path: PathBuf, source: io::Error },
}

impl Error {
    /// 2 for bad configuration, 1 for an unusable filesystem.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Missing(_) | Self::Invalid(_) | Self::Dotenv(_) => 2,
            Self::NoHome | Self::CreateDir { .. } | Self::NotWritable { .. } => 1,
        }
    }
}

// figment's own message drops the `TRIAGE_` prefix from the key, so name the var here.
impl From<figment::Error> for Error {
    fn from(err: figment::Error) -> Self {
        Self::Invalid(
            err.into_iter()
                .map(|e| {
                    format!(
                        "{ENV_PREFIX}{}: {}",
                        e.path.join("_").to_uppercase(),
                        e.kind
                    )
                })
                .collect(),
        )
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Columns,
    Json,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    pub log_level: String,
    pub log_file_level: String,
    pub log_format: LogFormat,
    pub otel_endpoint: Option<Url>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 7311,
            log_level: "info".into(),
            log_file_level: "info,triage=debug".into(),
            log_format: LogFormat::Columns,
            otel_endpoint: None,
        }
    }
}

impl Config {
    /// Reads `.env` in debug builds, checks [`REQUIRED`], then extracts `TRIAGE_*`.
    pub fn load() -> Result<Self> {
        #[cfg(debug_assertions)]
        match dotenvy::dotenv() {
            Err(err) if !err.not_found() => return Err(err.into()),
            _ => {}
        }
        preflight(REQUIRED, |name| {
            std::env::var_os(name).is_some_and(|v| !v.is_empty())
        })?;
        Self::from_env()
    }

    fn from_env() -> Result<Self> {
        Ok(Figment::from(Env::prefixed(ENV_PREFIX)).extract()?)
    }
}

/// Serde stops at the first missing field, so this names every absent var at once.
fn preflight(required: &[&str], is_set: impl Fn(&str) -> bool) -> Result<()> {
    let missing: Vec<String> = required
        .iter()
        .filter(|name| !is_set(name))
        .map(ToString::to_string)
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Error::Missing(missing))
    }
}

/// A value whose `Debug` output never shows its contents.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    pub fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub data: PathBuf,
    pub logs: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    /// `--data-dir` > `TRIAGE_DATA_DIR` > platform dirs. Creates each dir with mode 0700.
    pub fn resolve(flag: Option<PathBuf>) -> Result<Self> {
        let env = std::env::var_os(DATA_DIR_VAR).filter(|v| !v.is_empty());
        let paths = match pick_data_dir(flag, env) {
            Some(dir) => Self::under(dir),
            None => Self::platform(std::env::var_os("HOME").filter(|v| !v.is_empty()))?,
        };
        paths.create()?;
        Ok(paths)
    }

    fn under(dir: PathBuf) -> Self {
        Self {
            logs: dir.join("logs"),
            config: dir.join("config"),
            data: dir,
        }
    }

    // directories falls back to the passwd entry when HOME is unset, which would
    // silently pick a dir the user did not ask for.
    fn platform(home: Option<OsString>) -> Result<Self> {
        let home = PathBuf::from(home.ok_or(Error::NoHome)?);
        let dirs = ProjectDirs::from("studio", "", "triage").ok_or(Error::NoHome)?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            logs: platform_logs(&home, &dirs),
            config: dirs.config_dir().to_path_buf(),
        })
    }

    fn create(&self) -> Result<()> {
        create_dir(&self.data)?;
        check_writable(&self.data)?;
        create_dir(&self.config)?;
        create_dir(&self.logs)
    }
}

#[cfg(target_os = "macos")]
fn platform_logs(home: &Path, _dirs: &ProjectDirs) -> PathBuf {
    home.join("Library/Logs/triage")
}

#[cfg(not(target_os = "macos"))]
fn platform_logs(_home: &Path, dirs: &ProjectDirs) -> PathBuf {
    dirs.state_dir()
        .unwrap_or_else(|| dirs.data_local_dir())
        .join("logs")
}

fn pick_data_dir(flag: Option<PathBuf>, env: Option<OsString>) -> Option<PathBuf> {
    match (flag, env) {
        (Some(flag), Some(env)) => {
            tracing::debug!(
                flag = %flag.display(),
                env = %Path::new(&env).display(),
                "--data-dir overrides {DATA_DIR_VAR}"
            );
            Some(flag)
        }
        (flag, env) => flag.or_else(|| env.map(PathBuf::from)),
    }
}

fn create_dir(path: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path).map_err(|source| Error::CreateDir {
        path: path.to_path_buf(),
        source,
    })
}

// An existing read-only dir passes create_dir, so probe with a real write.
fn check_writable(dir: &Path) -> Result<()> {
    let probe = dir.join(".triage-write-check");
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&probe)
        .and_then(|_| fs::remove_file(&probe))
        .map_err(|source| Error::NotWritable {
            path: dir.to_path_buf(),
            source,
        })
}

#[cfg(test)]
#[allow(clippy::result_large_err)] // figment::Jail fixes the closure error type.
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use figment::Jail;

    use super::*;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn defaults_without_env() -> figment::Result<()> {
        Jail::try_with(|_| {
            assert_eq!(
                Config::from_env().map_err(|e| e.to_string())?,
                Config::default()
            );
            Ok(())
        })
    }

    #[test]
    fn reads_prefixed_env() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.set_env("TRIAGE_PORT", "8000");
            jail.set_env("TRIAGE_LOG_FORMAT", "json");
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "http://localhost:4317");
            let config = Config::from_env().map_err(|e| e.to_string())?;
            assert_eq!(config.port, 8000);
            assert_eq!(config.log_format, LogFormat::Json);
            assert_eq!(
                config.otel_endpoint.map(String::from).as_deref(),
                Some("http://localhost:4317/")
            );
            Ok(())
        })
    }

    #[test]
    fn malformed_port_names_the_var() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.set_env("TRIAGE_PORT", "abc");
            let Err(err) = Config::from_env() else {
                return Err("TRIAGE_PORT=abc must fail".into());
            };
            assert_eq!(err.exit_code(), 2);
            let message = err.to_string();
            assert!(message.contains("TRIAGE_PORT"), "{message}");
            assert!(message.contains("u16"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn malformed_url_fails() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "not a url");
            let message = Config::from_env()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(message.contains("TRIAGE_OTEL_ENDPOINT"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn preflight_lists_every_missing_var() {
        let result = preflight(&["TRIAGE_A", "TRIAGE_SET", "TRIAGE_B"], |name| {
            name == "TRIAGE_SET"
        });
        let Err(err) = result else {
            panic!("preflight must fail");
        };
        assert_eq!(err.exit_code(), 2);
        assert_eq!(
            err.to_string(),
            "missing required environment variables: TRIAGE_A, TRIAGE_B"
        );
    }

    #[test]
    fn preflight_passes_when_all_set() {
        assert!(preflight(REQUIRED, |_| false).is_ok());
        assert!(preflight(&["TRIAGE_A"], |_| true).is_ok());
    }

    #[test]
    fn secret_debug_is_redacted() {
        let secret = Secret("hunter2");
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(*secret.expose(), "hunter2");
    }

    #[test]
    fn data_dir_puts_everything_under_it() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let root = tmp.path().join("t");
        let paths = Paths::under(root.clone());
        paths.create()?;
        assert_eq!(paths.data, root);
        for dir in [&paths.data, &paths.logs, &paths.config] {
            assert!(dir.starts_with(&root) && dir.is_dir(), "{}", dir.display());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&paths.logs)?.permissions().mode() & 0o777,
                0o700
            );
        }
        Ok(())
    }

    #[test]
    fn precedence_flag_then_env() {
        let flag = PathBuf::from("/flag");
        let env = OsString::from("/env");
        assert_eq!(
            pick_data_dir(Some(flag.clone()), Some(env.clone())),
            Some(flag.clone())
        );
        assert_eq!(pick_data_dir(Some(flag.clone()), None), Some(flag));
        assert_eq!(pick_data_dir(None, Some(env)), Some(PathBuf::from("/env")));
        assert_eq!(pick_data_dir(None, None), None);
    }

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .map_err(|_| io::Error::other("poisoned"))?
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn flag_override_logs_debug_line() -> TestResult {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        tracing::subscriber::with_default(subscriber, || {
            pick_data_dir(Some("/flag".into()), Some("/env".into()));
        });
        let output = String::from_utf8(capture.0.lock().map_err(|_| "poisoned")?.clone())?;
        assert!(output.contains("DEBUG"), "{output}");
        assert!(
            output.contains("--data-dir overrides TRIAGE_DATA_DIR"),
            "{output}"
        );
        assert!(output.contains("/env"), "{output}");
        Ok(())
    }

    #[test]
    fn no_home_asks_for_data_dir() {
        let err = Paths::platform(None).err();
        assert!(matches!(err, Some(Error::NoHome)));
        let message = err.map(|e| e.to_string()).unwrap_or_default();
        assert!(message.contains("--data-dir"), "{message}");
    }

    #[cfg(unix)]
    #[test]
    fn read_only_data_dir_names_path_and_os_error() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let data = tmp.path().join("ro");
        fs::create_dir(&data)?;
        fs::set_permissions(&data, fs::Permissions::from_mode(0o500))?;
        let result = Paths::under(data.clone()).create();
        fs::set_permissions(&data, fs::Permissions::from_mode(0o700))?;

        let Err(err) = result else {
            return Err("read-only data dir must fail".into());
        };
        assert_eq!(err.exit_code(), 1);
        let message = err.to_string();
        assert!(message.contains(&data.display().to_string()), "{message}");
        assert!(message.contains("ermission denied"), "{message}");
        Ok(())
    }
}
