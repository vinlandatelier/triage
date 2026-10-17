//! Default stderr format: one aligned line per event.
//!
//! `HH:MM:SS.mmm LEVEL module    message  key=value…  duration  req=<id>`, then one
//! `└ ` line per `source()` of an error field. Closing spans print a line with their duration.

use std::fmt::{self, Display, Write as _};
use std::time::{Duration, Instant};

use time::{OffsetDateTime, UtcOffset};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::field::RecordFields;
use tracing_subscriber::fmt::format::{FmtSpan, Writer};
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, FormattedFields, MakeWriter};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

const MODULE_WIDTH: usize = 8;
/// `HH:MM:SS.mmm LEVEL ` is 19 columns wide.
const PREFIX_WIDTH: usize = 19;

/// The columnar stderr layer, including the span timer that feeds durations.
pub fn layer<S, W>(columns: Columns, writer: W, ansi: bool) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    SpanTimer.and_then(
        tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::CLOSE)
            .fmt_fields(ColumnFields)
            .event_format(columns)
            .with_ansi(ansi)
            .with_writer(writer),
    )
}

#[derive(Debug, Clone, Copy)]
pub struct Columns {
    offset: fn(OffsetDateTime) -> UtcOffset,
    now: fn() -> OffsetDateTime,
}

impl Columns {
    pub fn local() -> Self {
        Self {
            // Per event, so a long-running daemon follows DST changes.
            offset: |now| UtcOffset::local_offset_at(now).unwrap_or(UtcOffset::UTC),
            now: OffsetDateTime::now_utc,
        }
    }
}

impl<S> FormatEvent<S, ColumnFields> for Columns
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, ColumnFields>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let mut fields = Fields::default();
        let mut elapsed = None;
        // `FmtSpan::CLOSE` events carry the span's own metadata. Describe the span, not "close".
        let closing = meta.is_span().then(|| ctx.event_scope()?.next()).flatten();
        if let Some(span) = &closing {
            let ext = span.extensions();
            span.name().clone_into(&mut fields.message);
            if let Some(formatted) = ext.get::<FormattedFields<ColumnFields>>() {
                fields.pairs.clone_from(&formatted.fields);
            }
            elapsed = ext.get::<Started>().map(|started| started.0.elapsed());
        } else {
            event.record(&mut fields);
        }
        let request_id = fields.request_id.take().or_else(|| {
            ctx.event_scope()?
                .find_map(|span| span.extensions().get::<RequestId>().map(|id| id.0.clone()))
        });

        let now = (self.now)();
        let now = now.to_offset((self.offset)(now));
        write!(
            writer,
            "{:02}:{:02}:{:02}.{:03} ",
            now.hour(),
            now.minute(),
            now.second(),
            now.millisecond()
        )?;
        write_level(&mut writer, *meta.level())?;
        let module = meta.target().rsplit("::").next().unwrap_or_default();
        write!(writer, " {module:<MODULE_WIDTH$}  {}", fields.message)?;
        let indent = PREFIX_WIDTH + module.chars().count().max(MODULE_WIDTH) + 2;
        if !fields.pairs.is_empty() {
            write!(writer, "  {}", fields.pairs)?;
        }
        if let Some(elapsed) = elapsed {
            write!(writer, "  {}", HumanDuration(elapsed))?;
        }
        if let Some(id) = request_id {
            write!(writer, "  req={id}")?;
        }
        writeln!(writer)?;
        for source in fields.sources {
            writeln!(writer, "{:indent$}└ {source}", "")?;
        }
        Ok(())
    }
}

fn write_level(writer: &mut Writer<'_>, level: Level) -> fmt::Result {
    if !writer.has_ansi_escapes() {
        return write!(writer, "{level:<5}");
    }
    let colour = match level {
        Level::ERROR => 31,
        Level::WARN => 33,
        Level::INFO => 32,
        Level::DEBUG => 34,
        Level::TRACE => 35,
    };
    write!(writer, "\x1b[{colour}m{level:<5}\x1b[0m")
}

/// Renders span fields as `key=value` for close lines. `request_id` moves to `req=`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColumnFields;

impl<'writer> FormatFields<'writer> for ColumnFields {
    fn format_fields<R: RecordFields>(
        &self,
        mut writer: Writer<'writer>,
        fields: R,
    ) -> fmt::Result {
        let mut visitor = Fields::default();
        fields.record(&mut visitor);
        writer.write_str(&visitor.pairs)
    }

    fn add_fields(
        &self,
        current: &'writer mut FormattedFields<Self>,
        fields: &Record<'_>,
    ) -> fmt::Result {
        let mut visitor = Fields::default();
        fields.record(&mut visitor);
        if !current.fields.is_empty() && !visitor.pairs.is_empty() {
            current.fields.push(' ');
        }
        current.fields.push_str(&visitor.pairs);
        Ok(())
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    pairs: String,
    sources: Vec<String>,
    request_id: Option<String>,
}

impl Fields {
    fn push(&mut self, field: &Field, value: impl Display) {
        let value = one_line(value);
        match field.name() {
            "message" => self.message = value,
            "request_id" => self.request_id = Some(value),
            name => {
                if !self.pairs.is_empty() {
                    self.pairs.push(' ');
                }
                let _ = write!(self.pairs, "{name}={value}");
            }
        }
    }
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.push(field, value);
    }

    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        self.push(field, value);
        let mut source = value.source();
        while let Some(err) = source {
            self.sources.push(one_line(err));
            source = err.source();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.push(field, format_args!("{value:?}"));
    }
}

/// Also escapes control characters, so field values cannot move the cursor or recolour a TTY.
fn one_line(value: impl Display) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('⏎');
            }
            '\n' => out.push('⏎'),
            c if c.is_control() => {
                let _ = write!(out, "\\x{:02x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

struct HumanDuration(Duration);

impl Display for HumanDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.0;
        if d < Duration::from_millis(1) {
            write!(f, "{}µs", d.as_micros())
        // Below 999.95ms, so rounding never prints `1000.0ms`.
        } else if d < Duration::from_micros(999_950) {
            write!(f, "{:.1}ms", d.as_secs_f64() * 1000.0)
        } else {
            write!(f, "{:.2}s", d.as_secs_f64())
        }
    }
}

struct Started(Instant);
struct RequestId(String);

/// Stamps each span with its start time and `request_id` for [`Columns`] to read back.
struct SpanTimer;

impl<S> Layer<S> for SpanTimer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut ext = span.extensions_mut();
        ext.replace(Started(Instant::now()));
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(request_id) = fields.request_id {
            ext.replace(RequestId(request_id));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        values.record(&mut fields);
        if let Some(request_id) = fields.request_id {
            span.extensions_mut().replace(RequestId(request_id));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use tracing_subscriber::layer::SubscriberExt as _;

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

    fn fixed_clock() -> OffsetDateTime {
        OffsetDateTime::UNIX_EPOCH + time::Duration::milliseconds(45_296_789)
    }

    fn render(ansi: bool, emit: impl FnOnce()) -> Result<String, Box<dyn std::error::Error>> {
        let capture = Capture::default();
        let writer = capture.clone();
        let columns = Columns {
            offset: |_| UtcOffset::UTC,
            now: fixed_clock,
        };
        let subscriber =
            tracing_subscriber::registry().with(layer(columns, move || writer.clone(), ansi));
        tracing::subscriber::with_default(subscriber, emit);
        let bytes = capture.0.lock().map_err(|_| "poisoned")?.clone();
        Ok(String::from_utf8(bytes)?)
    }

    #[derive(Debug, thiserror::Error)]
    #[error("{0}")]
    struct Chain(&'static str, #[source] Option<Box<Chain>>);

    #[test]
    fn plain_event() -> TestResult {
        let out = render(false, || {
            tracing::info!(target: "triage::config", count = 3, path = "/tmp", "loaded");
        })?;
        insta::assert_snapshot!(out, @"12:34:56.789 INFO  config    loaded  count=3 path=/tmp");
        Ok(())
    }

    #[test]
    fn event_in_request_span_ends_with_req() -> TestResult {
        let out = render(false, || {
            let request = tracing::info_span!("request", request_id = "r1", method = "GET");
            let _request = request.enter();
            let handler = tracing::info_span!("handler");
            let _handler = handler.enter();
            tracing::warn!(target: "triage::http", "slow");
        })?;
        let first = out.lines().next().unwrap_or_default();
        insta::assert_snapshot!(first, @"12:34:56.789 WARN  http      slow  req=r1");
        Ok(())
    }

    #[test]
    fn request_id_recorded_later_is_seen() -> TestResult {
        let out = render(false, || {
            let span = tracing::info_span!("request", request_id = tracing::field::Empty);
            span.record("request_id", "r2");
            let _span = span.enter();
            tracing::info!(target: "triage::http", "late");
        })?;
        assert!(
            out.lines().next().is_some_and(|l| l.ends_with("  req=r2")),
            "{out}"
        );
        Ok(())
    }

    #[test]
    fn closing_span_prints_fields_and_duration() -> TestResult {
        let out = render(false, || {
            let span = tracing::info_span!(target: "triage::http::middleware", "request", request_id = "r3", method = "GET", status = tracing::field::Empty);
            span.record("status", 200);
        })?;
        let line = out.trim_end();
        let (head, tail) = line.split_once("  req=").ok_or(line)?;
        let (head, duration) = head.rsplit_once("  ").ok_or(line)?;
        insta::assert_snapshot!(head, @"12:34:56.789 INFO  middleware  request  method=GET status=200");
        assert!(duration.ends_with('s'), "{line}");
        assert_eq!(tail, "r3");
        Ok(())
    }

    #[test]
    fn error_chain_prints_one_line_per_source() -> TestResult {
        let err = Chain(
            "save failed",
            Some(Box::new(Chain(
                "disk full",
                Some(Box::new(Chain("quota exceeded", None))),
            ))),
        );
        let out = render(false, || {
            tracing::error!(target: "triage::store", error = &err as &dyn std::error::Error, "write");
        })?;
        insta::assert_snapshot!(out, @r"
        12:34:56.789 ERROR store     write  error=save failed
                                     └ disk full
                                     └ quota exceeded
        ");
        Ok(())
    }

    #[test]
    fn newlines_in_values_become_markers() -> TestResult {
        let out = render(false, || {
            tracing::info!(target: "triage::cli", body = "a\nb\r\nc", "two\nlines");
        })?;
        insta::assert_snapshot!(out, @"12:34:56.789 INFO  cli       two⏎lines  body=a⏎b⏎c");
        Ok(())
    }

    #[test]
    fn source_lines_align_under_a_long_module() -> TestResult {
        let err = Chain("save failed", Some(Box::new(Chain("disk full", None))));
        let out = render(false, || {
            tracing::error!(target: "triage::middleware", error = &err as &dyn std::error::Error, "write");
        })?;
        insta::assert_snapshot!(out, @r"
        12:34:56.789 ERROR middleware  write  error=save failed
                                       └ disk full
        ");
        Ok(())
    }

    #[test]
    fn control_characters_are_escaped() -> TestResult {
        let out = render(true, || {
            tracing::info!(target: "triage::mail", subject = "\x1b[2J\x07red\rfake\tx", "got");
        })?;
        assert!(
            out.trim_end()
                .ends_with(r"subject=\x1b[2J\x07red⏎fake\x09x"),
            "{out:?}"
        );
        assert_eq!(out.matches('\x1b').count(), 2, "{out:?}");
        Ok(())
    }

    #[test]
    fn ansi_only_when_enabled() -> TestResult {
        let emit = || tracing::info!("x");
        assert!(!render(false, emit)?.contains('\x1b'));
        assert!(render(true, emit)?.contains("\x1b[32mINFO \x1b[0m"));
        Ok(())
    }

    #[test]
    fn durations_pick_a_readable_unit() {
        let show = |d| HumanDuration(d).to_string();
        assert_eq!(show(Duration::from_micros(250)), "250µs");
        assert_eq!(show(Duration::from_micros(12_345)), "12.3ms");
        assert_eq!(show(Duration::from_micros(999_960)), "1.00s");
        assert_eq!(show(Duration::from_millis(2_500)), "2.50s");
    }
}
