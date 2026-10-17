//! Tracing subscriber: human-readable stderr plus a JSONL file for agents.

use std::collections::HashSet;
use std::fmt::{self, Write as _};
use std::io::{self, IsTerminal};
use std::path::Path;

use serde_json::{Map, Value};
use time::OffsetDateTime;
use time::format_description::BorrowedFormatItem;
use time::macros::format_description;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber, span};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields};
use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer, Registry};

use crate::config::{Config, LogFormat, Paths};

const MAX_LOG_FILES: usize = 7;

// Fixed width so `ts` sorts as text; RFC 3339 trims trailing sub-second zeros.
const TS_FORMAT: &[BorrowedFormatItem<'_>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]Z");

/// Flushes the JSONL file on drop. Hold it until the process exits.
#[derive(Debug)]
#[must_use = "dropping the guard stops file logging"]
pub struct Guard {
    _worker: Option<WorkerGuard>,
}

type BoxedLayer = Box<dyn Layer<Registry> + Send + Sync>;

struct Sinks {
    layers: Vec<BoxedLayer>,
    worker: Option<WorkerGuard>,
    warnings: Vec<String>,
}

/// Installs the global subscriber. An unusable logs dir degrades to stderr only.
pub fn init(config: &Config, paths: &Paths) -> Guard {
    let Sinks {
        layers,
        worker,
        mut warnings,
    } = sinks(config, paths);
    let worker = match tracing_subscriber::registry().with(layers).try_init() {
        Ok(()) => worker,
        Err(err) => {
            warnings.push(format!("tracing subscriber already installed: {err}"));
            None
        }
    };
    for warning in warnings {
        tracing::warn!("{warning}");
    }
    Guard { _worker: worker }
}

fn sinks(config: &Config, paths: &Paths) -> Sinks {
    let mut warnings = Vec::new();
    let stderr_filter = filter(
        "TRIAGE_LOG_LEVEL",
        &config.log_level,
        &Config::default().log_level,
        &mut warnings,
    );
    let file_filter = filter(
        "TRIAGE_LOG_FILE_LEVEL",
        &config.log_file_level,
        &Config::default().log_file_level,
        &mut warnings,
    );

    let mut layers: Vec<BoxedLayer> = vec![stderr_layer(config.log_format, stderr_filter)];
    let worker = match file_appender(&paths.logs) {
        Ok(appender) => {
            // Lossless: the file is the record agents debug from, so block rather than drop.
            let (writer, worker) = NonBlockingBuilder::default().lossy(false).finish(appender);
            layers.push(file_layer(writer).with_filter(file_filter).boxed());
            Some(worker)
        }
        Err(err) => {
            warnings.push(format!(
                "cannot write log files in {}: {err}; logging to stderr only",
                paths.logs.display()
            ));
            None
        }
    };
    Sinks {
        layers,
        worker,
        warnings,
    }
}

fn filter(var: &str, directives: &str, fallback: &str, warnings: &mut Vec<String>) -> EnvFilter {
    EnvFilter::try_new(directives).unwrap_or_else(|err| {
        warnings.push(format!(
            "invalid {var} {directives:?}: {err}; using {fallback:?}"
        ));
        EnvFilter::new(fallback)
    })
}

fn stderr_layer(format: LogFormat, filter: EnvFilter) -> BoxedLayer {
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(io::stderr)
        .with_ansi(io::stderr().is_terminal());
    match format {
        LogFormat::Pretty => layer.pretty().with_filter(filter).boxed(),
        // TODO(#15): Columns gets its own formatter.
        LogFormat::Columns | LogFormat::Compact => layer.compact().with_filter(filter).boxed(),
    }
}

fn file_appender(dir: &Path) -> Result<RollingFileAppender, String> {
    // tracing-appender prints its own stderr line for a non-directory, which would double the warning.
    if !dir.is_dir() {
        return Err("not a directory".into());
    }
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("triage")
        .filename_suffix("jsonl")
        .max_log_files(MAX_LOG_FILES)
        .build(dir)
        .map_err(|err| err.to_string())
}

fn file_layer<S, W>(writer: W) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> tracing_subscriber::fmt::MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .fmt_fields(SkipFields)
        .event_format(JsonLines)
        .with_writer(writer)
        .and_then(RequestIdLayer)
}

/// Span fields never reach the file line, so formatting them is wasted work.
#[derive(Debug, Clone, Copy, Default)]
struct SkipFields;

impl<'writer> FormatFields<'writer> for SkipFields {
    fn format_fields<R: RecordFields>(&self, _: Writer<'writer>, _: R) -> fmt::Result {
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct RequestId(Value);

/// Keeps each span's `request_id` in its extensions so events read it without reparsing fields.
struct RequestIdLayer;

impl<S> Layer<S> for RequestIdLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        store_request_id(attrs, id, &ctx);
    }

    fn on_record(&self, id: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        store_request_id(values, id, &ctx);
    }
}

fn store_request_id<S>(fields: &impl RecordFields, id: &span::Id, ctx: &Context<'_, S>)
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let mut map = Map::new();
    fields.record(&mut JsonVisitor(&mut map));
    if let Some(value) = map.remove("request_id")
        && let Some(span) = ctx.span(id)
    {
        span.extensions_mut().replace(RequestId(value));
    }
}

/// One JSON object per event: `ts level target message`, the event's fields
/// flattened, then `span`, `request_id`, `span_id`, `trace_id`.
#[derive(Debug, Clone, Copy, Default)]
struct JsonLines;

const RESERVED: &[&str] = &["ts", "level", "target", "span", "span_id", "trace_id"];

impl<S> FormatEvent<S, SkipFields> for JsonLines
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, SkipFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let ts = OffsetDateTime::now_utc()
            .format(TS_FORMAT)
            .map_err(|_| fmt::Error)?;
        let mut fields = Map::new();
        event.record(&mut JsonVisitor(&mut fields));
        let message = match fields.remove("message") {
            Some(Value::String(message)) => message,
            Some(other) => other.to_string(),
            None => String::new(),
        };
        let mut request_id = fields.remove("request_id");

        // Vec, not Map: serde_json's Map sorts keys, and these must lead the line.
        let mut line: Vec<(String, Value)> = vec![
            ("ts".into(), ts.into()),
            ("level".into(), meta.level().as_str().into()),
            ("target".into(), meta.target().into()),
            ("message".into(), message.into()),
        ];
        let mut taken: HashSet<String> = fields.keys().cloned().collect();
        for (key, value) in fields {
            line.push((free_key(key, &mut taken), value));
        }

        if let Some(scope) = ctx.event_scope() {
            let spans: Vec<_> = scope.collect();
            if let (Some(leaf), Some(root)) = (spans.first(), spans.last()) {
                line.push(("span".into(), leaf.name().into()));
                request_id = request_id.or_else(|| {
                    spans.iter().find_map(|span| {
                        span.extensions().get::<RequestId>().map(|id| id.0.clone())
                    })
                });
                if let Some(id) = request_id.take() {
                    line.push(("request_id".into(), id));
                }
                // Strings: registry ids carry a generation in the high bits and exceed 2^53.
                line.push(("span_id".into(), leaf.id().into_u64().to_string().into()));
                line.push(("trace_id".into(), root.id().into_u64().to_string().into()));
            }
        }
        if let Some(id) = request_id {
            line.push(("request_id".into(), id));
        }

        let mut out = String::new();
        for (key, value) in line {
            if !out.is_empty() {
                out.push(',');
            }
            write!(out, "{}:{value}", Value::String(key))?;
        }
        writeln!(writer, "{{{out}}}")
    }
}

/// A field that clashes with a line key moves to `field.<name>`, prefixed again until free.
fn free_key(key: String, taken: &mut HashSet<String>) -> String {
    if !RESERVED.contains(&key.as_str()) {
        return key;
    }
    let mut key = format!("field.{key}");
    while taken.contains(&key) {
        key = format!("field.{key}");
    }
    taken.insert(key.clone());
    key
}

struct JsonVisitor<'a>(&'a mut Map<String, Value>);

impl Visit for JsonVisitor<'_> {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.0.insert(field.name().into(), value.to_string().into());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0
            .insert(field.name().into(), format!("{value:?}").into());
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};

    use time::format_description::well_known::Rfc3339;

    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl io::Write for Capture {
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

    fn capture_raw(emit: impl FnOnce()) -> Result<String, Box<dyn std::error::Error>> {
        let capture = Capture::default();
        let writer = capture.clone();
        let subscriber = tracing_subscriber::registry().with(file_layer(move || writer.clone()));
        tracing::subscriber::with_default(subscriber, emit);
        let bytes = capture.0.lock().map_err(|_| "poisoned")?.clone();
        Ok(String::from_utf8(bytes)?)
    }

    fn capture(emit: impl FnOnce()) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
        Ok(capture_raw(emit)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?)
    }

    #[test]
    fn core_keys_lead_the_line() -> TestResult {
        let raw = capture_raw(|| tracing::info!(a = 1, "hi"))?;
        let at = |key: &str| raw.find(&format!("\"{key}\":"));
        let order: Vec<_> = ["ts", "level", "target", "message", "a"].map(at).into();
        assert!(order.iter().all(Option::is_some), "{raw}");
        assert!(order.is_sorted(), "{raw}");
        Ok(())
    }

    #[test]
    fn event_without_span_has_core_keys_and_flat_fields() -> TestResult {
        let lines = capture(|| tracing::info!(count = 3, ok = true, "hello"))?;
        let [line] = lines.as_slice() else {
            return Err(format!("want one line, got {lines:?}").into());
        };
        let ts = line["ts"].as_str().unwrap_or_default();
        assert!(OffsetDateTime::parse(ts, &Rfc3339).is_ok(), "{ts}");
        assert_eq!(ts.len(), "2026-01-01T00:00:00.000000Z".len(), "{ts}");
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["target"], module_path!());

        assert_eq!(line["message"], "hello");
        assert_eq!(line["count"], 3);
        assert_eq!(line["ok"], true);
        for key in ["span", "span_id", "trace_id", "request_id"] {
            assert!(line.get(key).is_none(), "{key} in {line}");
        }
        Ok(())
    }

    #[test]
    fn request_id_and_ids_are_lifted_from_spans() -> TestResult {
        let lines = capture(|| {
            let request = tracing::info_span!("request", request_id = "r1", method = "GET");
            let _request = request.enter();
            let inner = tracing::info_span!("handler");
            let _inner = inner.enter();
            tracing::info!(level = "user", "inside");
        })?;
        let [line] = lines.as_slice() else {
            return Err(format!("want one line, got {lines:?}").into());
        };
        assert_eq!(line["span"], "handler");
        assert_eq!(line["request_id"], "r1");
        assert!(line["span_id"].is_string());
        assert!(line["trace_id"].is_string());
        assert_ne!(line["span_id"], line["trace_id"]);
        assert_eq!(line["level"], "INFO");
        assert_eq!(line["field.level"], "user");
        assert!(
            line.get("method").is_none(),
            "span fields stay off the line"
        );
        Ok(())
    }

    #[test]
    fn request_id_recorded_later_is_seen() -> TestResult {
        let lines = capture(|| {
            let span = tracing::info_span!("request", request_id = tracing::field::Empty);
            span.record("request_id", "r2");
            let _span = span.enter();
            tracing::info!("late");
        })?;
        assert_eq!(lines.first().map(|l| &l["request_id"]), Some(&"r2".into()));
        Ok(())
    }

    #[test]
    fn event_request_id_wins_and_sits_in_the_tail() -> TestResult {
        let raw = capture_raw(|| {
            let span = tracing::info_span!("request", request_id = "span");
            let _span = span.enter();
            tracing::info!(request_id = "event", z = 1, "x");
        })?;
        let line: Value = serde_json::from_str(raw.trim())?;
        assert_eq!(line["request_id"], "event");
        let at = |key: &str| raw.find(&format!("\"{key}\":"));
        assert!(
            at("span") < at("request_id") && at("request_id") < at("span_id"),
            "{raw}"
        );
        Ok(())
    }

    #[test]
    fn renamed_field_never_duplicates_a_key() -> TestResult {
        let raw = capture_raw(|| tracing::info!(ts = 1, field.ts = 2, "x"))?;
        assert_eq!(raw.matches("\"field.ts\":").count(), 1, "{raw}");
        let line: Value = serde_json::from_str(raw.trim())?;
        assert_eq!(line["field.ts"], 2);
        assert_eq!(line["field.field.ts"], 1);
        Ok(())
    }

    #[test]
    fn message_is_always_a_string() -> TestResult {
        let lines = capture(|| {
            tracing::info!(a = 1);
            tracing::info!(message = 5);
        })?;
        assert_eq!(
            lines.iter().map(|l| &l["message"]).collect::<Vec<_>>(),
            [&Value::from(""), &Value::from("5")]
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn unusable_logs_dir_gives_one_warning_naming_it() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        let logs = tmp.path().join("logs");
        fs::create_dir(&logs)?;
        fs::set_permissions(&logs, fs::Permissions::from_mode(0o500))?;
        let paths = Paths {
            data: tmp.path().into(),
            logs: logs.clone(),
            config: tmp.path().into(),
        };
        let sinks = sinks(&Config::default(), &paths);
        fs::set_permissions(&logs, fs::Permissions::from_mode(0o700))?;
        // Root ignores the mode bits, so the appender succeeds.
        if sinks.worker.is_some() {
            return Ok(());
        }
        assert_eq!(sinks.layers.len(), 1);
        let [warning] = sinks.warnings.as_slice() else {
            return Err(format!("want one warning, got {:?}", sinks.warnings).into());
        };
        assert!(warning.contains(&logs.display().to_string()), "{warning}");
        Ok(())
    }

    #[test]
    fn logs_path_that_is_a_file_fails_appender() -> TestResult {
        let tmp = tempfile::tempdir()?;
        let file = tmp.path().join("logs");
        fs::write(&file, "")?;
        assert!(file_appender(&file).is_err());
        Ok(())
    }

    #[test]
    fn root_span_id_is_trace_id() -> TestResult {
        let lines = capture(|| {
            let span = tracing::info_span!("only");
            let _span = span.enter();
            tracing::info!("x");
        })?;
        let line = lines.first().ok_or("no line")?;
        assert_eq!(line["span_id"], line["trace_id"]);
        Ok(())
    }

    #[test]
    fn appender_keeps_seven_files() -> TestResult {
        let tmp = tempfile::tempdir()?;
        for day in 1..=8 {
            fs::write(tmp.path().join(format!("triage.2020-01-0{day}.jsonl")), "")?;
            // Pruning sorts by creation time, so give each file a distinct one.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let _appender = file_appender(tmp.path())?;
        let mut names: Vec<_> = fs::read_dir(tmp.path())?
            .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<Result<_, _>>()?;
        names.sort();
        assert_eq!(names.len(), MAX_LOG_FILES, "{names:?}");
        assert!(
            !names.contains(&"triage.2020-01-01.jsonl".to_owned()),
            "{names:?}"
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn read_only_logs_dir_fails_appender() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir()?;
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o500))?;
        let result = file_appender(tmp.path());
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700))?;
        assert!(result.is_err());
        Ok(())
    }
}
