//! Environment → [`Config`], and the platform directories in [`Paths`].
//!
//! This is the only module that reads `std::env`.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, DirBuilder, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use figment::Figment;
use figment::error::Kind;
use figment::providers::Env;
use serde::Deserialize;
use tracing_subscriber::EnvFilter;
use url::Url;

/// Env vars that must be set before startup. M0 has none.
pub const REQUIRED: &[&str] = &[];

const ENV_PREFIX: &str = "TRIAGE_";
const DATA_DIR_VAR: &str = "TRIAGE_DATA_DIR";
const OTEL_ENDPOINT_VAR: &str = "TRIAGE_OTEL_ENDPOINT";
const OTEL_TRACES_VAR: &str = "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT";
const OTEL_STANDARD_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

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
    #[error("data dir {}: `~` is not expanded; use an absolute path", .0.display())]
    TildeDataDir(PathBuf),
    #[error("cannot create directory {}: {source}", path.display())]
    CreateDir { path: PathBuf, source: io::Error },
    #[error("directory {} is not writable: {source}", path.display())]
    NotWritable { path: PathBuf, source: io::Error },
}

impl Error {
    /// 2 for bad configuration, 1 for an unusable filesystem.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Missing(_) | Self::Invalid(_) | Self::Dotenv(_) | Self::TildeDataDir(_) => 2,
            Self::NoHome | Self::CreateDir { .. } | Self::NotWritable { .. } => 1,
        }
    }
}

// figment's own message drops the `TRIAGE_` prefix from the key, so name the var here.
// It also echoes the raw value, which would leak a malformed secret to stderr.
impl From<figment::Error> for Error {
    fn from(err: figment::Error) -> Self {
        Self::Invalid(
            err.into_iter()
                .map(|e| {
                    let var = format!("{ENV_PREFIX}{}", e.path.join("_").to_uppercase());
                    match e.kind {
                        Kind::InvalidType(_, expected) | Kind::InvalidValue(_, expected) => {
                            format!("{var}: expected {expected}")
                        }
                        kind => format!("{var}: {kind}"),
                    }
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
    Pretty,
    Compact,
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Columns => "columns",
            Self::Pretty => "pretty",
            Self::Compact => "compact",
        })
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    pub log_level: String,
    pub log_file_level: String,
    pub log_format: LogFormat,
    // Three vars feed it, so `from_env` reads it instead of figment.
    #[serde(skip)]
    pub otel_endpoint: Option<OtelEndpoint>,
    /// Bad settings that fell back instead of failing; telemetry logs them once it starts.
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            port: 7311,
            log_level: "info".into(),
            log_file_level: "info,triage=debug".into(),
            log_format: LogFormat::Columns,
            otel_endpoint: None,
            warnings: Vec::new(),
        }
    }
}

impl Config {
    /// Reads `.env` in debug builds, checks [`REQUIRED`], then extracts `TRIAGE_*`.
    pub fn load() -> Result<Self> {
        #[cfg(debug_assertions)]
        // `dotenv()` walks up parent dirs and could load an unrelated `.env`.
        match dotenvy::from_path(".env") {
            Err(err) if !err.not_found() => return Err(err.into()),
            _ => {}
        }
        preflight(REQUIRED, |name| {
            std::env::var_os(name).is_some_and(|v| !v.is_empty())
        })?;
        Self::from_env()
    }

    fn from_env() -> Result<Self> {
        // `VAR=` means unset, matching preflight and TRIAGE_DATA_DIR.
        let env = Env::prefixed(ENV_PREFIX).filter(|key| {
            std::env::var_os(format!("{ENV_PREFIX}{}", key.as_str().to_uppercase()))
                .is_none_or(|v| !v.is_empty())
        });
        let mut config: Self = Figment::from(env).extract()?;
        (config.otel_endpoint, config.warnings) = otel_endpoint()?;
        config.validate()?;
        Ok(config)
    }

    /// Checks what serde cannot: the log filters must parse as `EnvFilter` directives.
    fn validate(&self) -> Result<()> {
        let invalid: Vec<String> = [
            ("TRIAGE_LOG_LEVEL", &self.log_level),
            ("TRIAGE_LOG_FILE_LEVEL", &self.log_file_level),
        ]
        .into_iter()
        .filter_map(|(var, directives)| {
            EnvFilter::try_new(directives)
                .err()
                .map(|err| format!("{var}: {err}"))
        })
        .collect();
        if invalid.is_empty() {
            Ok(())
        } else {
            Err(Error::Invalid(invalid))
        }
    }

    /// Each env var with its value as it would be set, `-` when unset. Format a [`Secret`] field
    /// through `Debug` so it prints redacted.
    pub fn entries(&self) -> Vec<(String, String)> {
        // Exhaustive so a new field fails to compile until it is listed here.
        let Self {
            port,
            log_level,
            log_file_level,
            log_format,
            otel_endpoint,
            warnings: _,
        } = self;
        let otel_endpoint = otel_endpoint
            .as_ref()
            .map_or_else(|| "-".to_owned(), ToString::to_string);
        [
            ("port", port.to_string()),
            ("log_level", log_level.clone()),
            ("log_file_level", log_file_level.clone()),
            ("log_format", log_format.to_string()),
            ("otel_endpoint", otel_endpoint),
        ]
        .into_iter()
        .map(|(key, value)| (format!("{ENV_PREFIX}{}", key.to_uppercase()), value))
        .collect()
    }
}

// Manual so the startup log cannot leak `otel_endpoint` credentials through `Url`'s `Debug`.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            port,
            log_level,
            log_file_level,
            log_format,
            otel_endpoint,
            warnings,
        } = self;
        f.debug_struct("Config")
            .field("port", port)
            .field("log_level", log_level)
            .field("log_file_level", log_file_level)
            .field("log_format", log_format)
            .field("otel_endpoint", otel_endpoint)
            .field("warnings", warnings)
            .finish()
    }
}

/// Where OTLP traces go, and which env var set it.
#[derive(Clone, PartialEq, Eq)]
pub struct OtelEndpoint {
    url: Url,
    traces: Url,
    var: &'static str,
}

impl OtelEndpoint {
    /// The URL the exporter posts spans to.
    pub fn traces(&self) -> &Url {
        &self.traces
    }

    // Errors never echo the value, which may carry credentials.
    fn parse(var: &'static str, value: &OsStr) -> Result<Self, String> {
        let invalid = || format!("{var}: expected an http URL with a host");
        let url = value
            .to_str()
            .and_then(|v| Url::parse(v).ok())
            .ok_or_else(invalid)?;
        // The exporter is built without TLS, so every https export would fail.
        if url.scheme() == "https" {
            return Err(format!(
                "{var}: https is not supported; use an http collector"
            ));
        }
        // `localhost:4317` parses as a URL with scheme `localhost`.
        if url.scheme() != "http" || !url.has_host() {
            return Err(invalid());
        }
        // The OTel spec takes the signal-specific var verbatim and appends the path to the base.
        let traces = if var == OTEL_TRACES_VAR {
            url.clone()
        } else {
            traces_url(&url).map_err(|_| invalid())?
        };
        Ok(Self { url, traces, var })
    }
}

// A `Url`'s string form and `Debug` keep userinfo credentials.
impl fmt::Display for OtelEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&redact_userinfo(&self.url))?;
        if self.var != OTEL_ENDPOINT_VAR {
            write!(f, " (from {})", self.var)?;
        }
        Ok(())
    }
}

impl fmt::Debug for OtelEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

pub(crate) fn redact_userinfo(url: &Url) -> String {
    let mut url = url.clone();
    // Both setters only fail on URLs that cannot have userinfo, which also have none to hide.
    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }
    if url.password().is_some() {
        let _ = url.set_password(Some("redacted"));
    }
    url.into()
}

/// `TRIAGE_OTEL_ENDPOINT` wins, and set but empty turns OTLP off. A bad value there exits 2. The
/// standard vars may be set shell-wide for other tools, so a bad one only warns and leaves OTLP off.
fn otel_endpoint() -> Result<(Option<OtelEndpoint>, Vec<String>)> {
    let var = std::env::var_os;
    if let Some(value) = var(OTEL_ENDPOINT_VAR) {
        if value.is_empty() {
            return Ok((None, Vec::new()));
        }
        let endpoint = OtelEndpoint::parse(OTEL_ENDPOINT_VAR, &value)
            .map_err(|err| Error::Invalid(vec![err]))?;
        return Ok((Some(endpoint), Vec::new()));
    }
    let Some((name, value)) = [OTEL_TRACES_VAR, OTEL_STANDARD_VAR]
        .into_iter()
        .find_map(|name| var(name).filter(|v| !v.is_empty()).map(|v| (name, v)))
    else {
        return Ok((None, Vec::new()));
    };
    Ok(match OtelEndpoint::parse(name, &value) {
        Ok(endpoint) => (Some(endpoint), Vec::new()),
        Err(err) => (None, vec![format!("{err}; OTLP export is off")]),
    })
}

fn traces_url(base: &Url) -> Result<Url, url::ParseError> {
    let mut base = base.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    base.join("v1/traces")
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
    /// `--data-dir` > `TRIAGE_DATA_DIR` > platform dirs. Touches nothing on disk; see [`Self::create`].
    pub fn resolve(flag: Option<PathBuf>) -> Result<Self> {
        let home = std::env::var_os("HOME").filter(|v| Path::new(v).is_absolute());
        let paths = match pick_data_dir(flag, data_dir_env()) {
            Some(dir) => Self::under(absolute_data_dir(dir)?),
            None => Self::platform(home)?,
        };
        Ok(paths)
    }

    /// Held by `triage serve` so only one instance uses this data dir.
    pub fn lock_file(&self) -> PathBuf {
        self.data.join("triage.lock")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data.join("triage.db")
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

    /// Creates each dir with mode 0700 and checks the data and config dirs are writable.
    pub fn create(&self) -> Result<()> {
        for dir in [&self.data, &self.config] {
            create_dir(dir)?;
            check_writable(dir)?;
        }
        // Telemetry reports an unusable logs dir and falls back to stderr.
        let _ = create_dir(&self.logs);
        Ok(())
    }
}

/// Logs at debug when `--data-dir` wins over `TRIAGE_DATA_DIR`.
/// Separate from [`Paths::resolve`] because the subscriber needs the resolved logs dir first.
pub fn log_data_dir_override(flag: Option<&Path>) {
    log_override(flag, data_dir_env().as_deref());
}

fn data_dir_env() -> Option<OsString> {
    std::env::var_os(DATA_DIR_VAR).filter(|v| !v.is_empty())
}

fn log_override(flag: Option<&Path>, env: Option<&OsStr>) {
    if let (Some(flag), Some(env)) = (flag, env) {
        tracing::debug!(
            flag = %flag.display(),
            env = %Path::new(env).display(),
            "--data-dir overrides {DATA_DIR_VAR}"
        );
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
    flag.or_else(|| env.map(PathBuf::from))
}

// No shell expands `~` in `.env` or a quoted flag, and a relative dir would
// move with any later chdir.
fn absolute_data_dir(dir: PathBuf) -> Result<PathBuf> {
    if dir.as_os_str().as_encoded_bytes().starts_with(b"~") {
        return Err(Error::TildeDataDir(dir));
    }
    std::path::absolute(&dir).map_err(|source| Error::CreateDir { path: dir, source })
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
// The pid keeps concurrent starts apart; `create_new` refuses a planted symlink.
fn check_writable(dir: &Path) -> Result<()> {
    probe_writable(dir).map_err(|source| Error::NotWritable {
        path: dir.to_path_buf(),
        source,
    })
}

/// Also tightens `dir` to 0700 when the current user owns it.
pub(crate) fn probe_writable(dir: &Path) -> io::Result<()> {
    let probe = dir.join(format!(".triage-write-check-{}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    #[cfg(unix)]
    let owner = file.metadata();
    fs::remove_file(&probe)?;
    #[cfg(unix)]
    if let Ok(owner) = owner {
        restrict(dir, std::os::unix::fs::MetadataExt::uid(&owner));
    }
    Ok(())
}

// `create_dir` leaves an existing dir's mode alone, so one made by hand or an older build may
// still be 0755. The probe file's owner is the effective uid, without a libc dependency.
#[cfg(unix)]
fn restrict(dir: &Path, uid: u32) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    if let Ok(meta) = fs::metadata(dir)
        && meta.uid() == uid
        && meta.mode() & 0o077 != 0
    {
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
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
        Jail::try_with(|jail| {
            jail.clear_env();
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
            jail.clear_env();
            jail.set_env("TRIAGE_PORT", "8000");
            jail.set_env("TRIAGE_LOG_FORMAT", "pretty");
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "http://localhost:4317");
            let config = Config::from_env().map_err(|e| e.to_string())?;
            assert_eq!(config.port, 8000);
            assert_eq!(config.log_format, LogFormat::Pretty);
            assert_eq!(
                config
                    .otel_endpoint
                    .map(|e| e.traces.to_string())
                    .as_deref(),
                Some("http://localhost:4317/v1/traces")
            );
            Ok(())
        })
    }

    #[test]
    fn malformed_port_names_the_var() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
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
    fn invalid_log_levels_exit_2_naming_each_var() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env("TRIAGE_LOG_LEVEL", "info,=[");
            jail.set_env("TRIAGE_LOG_FILE_LEVEL", "triage=loud");
            let Err(err) = Config::from_env() else {
                return Err("invalid log levels must fail".into());
            };
            assert_eq!(err.exit_code(), 2);
            let message = err.to_string();
            assert!(message.contains("TRIAGE_LOG_LEVEL:"), "{message}");
            assert!(message.contains("TRIAGE_LOG_FILE_LEVEL:"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn malformed_url_fails() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
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
    fn empty_values_mean_unset() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env("TRIAGE_PORT", "");
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "");
            assert_eq!(
                Config::from_env().map_err(|e| e.to_string())?,
                Config::default()
            );
            Ok(())
        })
    }

    #[test]
    fn otel_endpoint_needs_http_scheme() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "otlp://user:secret@host");
            let message = Config::from_env()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(message.contains("TRIAGE_OTEL_ENDPOINT"), "{message}");
            assert!(!message.contains("secret"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn invalid_value_is_not_echoed() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env("TRIAGE_PORT", "hunter2");
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "hunter3");
            let message = Config::from_env()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(message.contains("TRIAGE_"), "{message}");
            assert!(!message.contains("hunter"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn tilde_data_dir_is_rejected() {
        for dir in ["~/x", "~user/x"] {
            let err = absolute_data_dir(PathBuf::from(dir)).err();
            assert_eq!(err.map(|e| e.exit_code()), Some(2), "{dir}");
        }
    }

    #[test]
    fn relative_data_dir_becomes_absolute() -> TestResult {
        assert!(absolute_data_dir(PathBuf::from("rel"))?.is_absolute());
        Ok(())
    }

    #[test]
    fn otel_endpoint_falls_back_to_standard_var() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env(OTEL_STANDARD_VAR, "http://collector:4318");
            let config = Config::from_env().map_err(|e| e.to_string())?;
            assert_eq!(
                config
                    .otel_endpoint
                    .map(|e| e.traces.to_string())
                    .as_deref(),
                Some("http://collector:4318/v1/traces")
            );
            jail.set_env("TRIAGE_OTEL_ENDPOINT", "http://triage:4318");
            let config = Config::from_env().map_err(|e| e.to_string())?;
            assert_eq!(
                config
                    .otel_endpoint
                    .map(|e| e.traces.to_string())
                    .as_deref(),
                Some("http://triage:4318/v1/traces")
            );
            Ok(())
        })
    }

    #[test]
    fn malformed_standard_otel_vars_warn_and_leave_otlp_off() -> figment::Result<()> {
        for (var, value) in [
            (OTEL_STANDARD_VAR, "not a url"),
            (OTEL_TRACES_VAR, "https://user:hunter2@collector:4318"),
        ] {
            Jail::try_with(|jail| {
                jail.clear_env();
                jail.set_env(var, value);
                let config = Config::from_env().map_err(|e| e.to_string())?;
                assert_eq!(config.otel_endpoint, None);
                let [warning] = config.warnings.as_slice() else {
                    return Err(format!("want one warning, got {:?}", config.warnings).into());
                };
                assert!(warning.contains(var), "{warning}");
                assert!(!warning.contains("hunter2"), "{warning}");
                Ok(())
            })?;
        }
        Ok(())
    }

    #[test]
    fn https_triage_endpoint_exits_2() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env(OTEL_ENDPOINT_VAR, "https://collector:4318");
            let Err(err) = Config::from_env() else {
                return Err("https must fail".into());
            };
            assert_eq!(err.exit_code(), 2);
            let message = err.to_string();
            assert!(message.contains("https is not supported"), "{message}");
            Ok(())
        })
    }

    #[test]
    fn empty_triage_endpoint_disables_otlp() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env(OTEL_ENDPOINT_VAR, "");
            jail.set_env(OTEL_STANDARD_VAR, "http://collector:4318");
            assert_eq!(
                Config::from_env().map_err(|e| e.to_string())?,
                Config::default()
            );
            Ok(())
        })
    }

    #[test]
    fn traces_var_wins_over_base_and_is_used_verbatim() -> figment::Result<()> {
        Jail::try_with(|jail| {
            jail.clear_env();
            jail.set_env(OTEL_STANDARD_VAR, "http://base:4318");
            jail.set_env(OTEL_TRACES_VAR, "http://traces:4318/custom");
            let config = Config::from_env().map_err(|e| e.to_string())?;
            assert_eq!(
                config
                    .otel_endpoint
                    .map(|e| e.traces.to_string())
                    .as_deref(),
                Some("http://traces:4318/custom")
            );
            Ok(())
        })
    }

    #[test]
    fn traces_path_is_appended_to_base() -> TestResult {
        for (base, want) in [
            ("http://h:4318", "http://h:4318/v1/traces"),
            ("http://h:4318/otlp", "http://h:4318/otlp/v1/traces"),
            ("http://h:4318/otlp/", "http://h:4318/otlp/v1/traces"),
        ] {
            assert_eq!(traces_url(&Url::parse(base)?)?.as_str(), want);
        }
        Ok(())
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
    fn entries_name_every_var() -> TestResult {
        let config = Config {
            otel_endpoint: Some(OtelEndpoint::parse(
                OTEL_ENDPOINT_VAR,
                OsStr::new("http://localhost:4317"),
            )?),
            ..Config::default()
        };
        assert_eq!(
            config.entries(),
            [
                ("TRIAGE_PORT", "7311"),
                ("TRIAGE_LOG_LEVEL", "info"),
                ("TRIAGE_LOG_FILE_LEVEL", "info,triage=debug"),
                ("TRIAGE_LOG_FORMAT", "columns"),
                ("TRIAGE_OTEL_ENDPOINT", "http://localhost:4317/"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        Ok(())
    }

    #[test]
    fn entries_redact_endpoint_credentials() -> TestResult {
        let config = Config {
            otel_endpoint: Some(OtelEndpoint::parse(
                OTEL_STANDARD_VAR,
                OsStr::new("http://user:hunter2@host:4317/v1?x=1"),
            )?),
            ..Config::default()
        };
        let entries = config.entries();
        let endpoint = entries
            .iter()
            .find(|(key, _)| key == OTEL_ENDPOINT_VAR)
            .map(|(_, value)| value.as_str());
        assert_eq!(
            endpoint,
            Some("http://redacted:redacted@host:4317/v1?x=1 (from OTEL_EXPORTER_OTLP_ENDPOINT)")
        );
        Ok(())
    }

    #[test]
    fn debug_redacts_endpoint_credentials() -> TestResult {
        let config = Config {
            otel_endpoint: Some(OtelEndpoint::parse(
                OTEL_ENDPOINT_VAR,
                OsStr::new("http://user:hunter2@host:4317"),
            )?),
            ..Config::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(!debug.contains("user"), "{debug}");
        Ok(())
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

    #[cfg(unix)]
    #[test]
    fn existing_open_dirs_are_tightened_to_0700() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let paths = Paths::under(tmp.path().join("t"));
        for dir in [&paths.data, &paths.config] {
            fs::create_dir_all(dir)?;
            fs::set_permissions(dir, fs::Permissions::from_mode(0o755))?;
        }
        paths.create()?;
        for dir in [&paths.data, &paths.config] {
            assert_eq!(fs::metadata(dir)?.permissions().mode() & 0o777, 0o700);
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
            log_override(Some(Path::new("/flag")), Some(OsStr::new("/env")));
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
        // Root ignores the mode bits, so there is nothing to test.
        if fs::write(data.join("root-check"), b"").is_ok() {
            return Ok(());
        }
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

    #[cfg(unix)]
    #[test]
    fn read_only_logs_dir_is_left_to_telemetry() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let paths = Paths::under(tmp.path().join("t"));
        fs::create_dir_all(&paths.logs)?;
        fs::set_permissions(&paths.logs, fs::Permissions::from_mode(0o500))?;
        let result = paths.create();
        fs::set_permissions(&paths.logs, fs::Permissions::from_mode(0o700))?;
        result?;
        Ok(())
    }
}
