// Loaded only when an OTLP endpoint is set. OpenTelemetry is imported only here and in the test,
// so a version bump touches one place.
import { ROOT_CONTEXT, type Span, SpanStatusCode, trace } from "@opentelemetry/api";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-proto";
import { resourceFromAttributes } from "@opentelemetry/resources";
import {
  BatchSpanProcessor,
  type SpanExporter,
  type SpanProcessor,
  TracerProvider,
} from "@opentelemetry/sdk-trace";
import packageJson from "../../package.json" with { type: "json" };
import { redactUserinfo } from "../config";

// Also bounds retries. Under the shutdown budget, so a blackholed collector still fails, and warns,
// before shutdown gives up on the final flush.
const EXPORT_TIMEOUT_MS = 2000;
const SHUTDOWN_TIMEOUT_MS = 5000;
// `ExportResultCode.SUCCESS`; `@opentelemetry/core` is not a direct dependency.
const EXPORT_SUCCESS = 0;

export type OtelAttributes = Readonly<Record<string, string | number | boolean>>;

export type OtelSpan = {
  readonly span: Span;
};

export type Otel = {
  readonly startSpan: (name: string, attributes: OtelAttributes, parent?: OtelSpan) => OtelSpan;
  readonly record: (span: OtelSpan, attributes: OtelAttributes) => void;
  readonly endSpan: (span: OtelSpan, error?: unknown) => void;
  readonly shutdown: () => Promise<void>;
};

type Warn = (message: string, fields: Readonly<Record<string, unknown>>) => void;

/** A batch exporter posting to `traces` as given. Building never connects. */
export function startOtel(
  traces: URL,
  warn: Warn,
  processor: (exporter: SpanExporter) => SpanProcessor = batch,
): Otel {
  const exporter = new OTLPTraceExporter({ url: traces.href, timeoutMillis: EXPORT_TIMEOUT_MS });
  const provider = new TracerProvider({
    resource: resourceFromAttributes({
      "service.name": "triage",
      "service.version": packageJson.version,
    }),
    spanProcessors: [processor(warnOnce(exporter, redactUserinfo(traces), warn))],
  });
  const tracer = provider.getTracer("triage");
  return {
    startSpan(name, attributes, parent) {
      const context =
        parent === undefined ? ROOT_CONTEXT : trace.setSpan(ROOT_CONTEXT, parent.span);
      return { span: tracer.startSpan(name, { attributes }, context) };
    },
    record({ span }, attributes) {
      span.setAttributes(attributes);
    },
    endSpan({ span }, error) {
      if (error !== undefined) {
        span.setStatus({
          code: SpanStatusCode.ERROR,
          message: error instanceof Error ? error.message : String(error),
        });
      }
      span.end();
    },
    async shutdown() {
      let timer: Timer | undefined;
      const timeout = new Promise<void>((resolve) => {
        timer = setTimeout(resolve, SHUTDOWN_TIMEOUT_MS);
      });
      try {
        // A refused final export already warned once; shutdown must still finish.
        await Promise.race([provider.shutdown().catch(() => undefined), timeout]);
      } finally {
        clearTimeout(timer);
      }
    },
  };
}

function batch(exporter: SpanExporter): SpanProcessor {
  return new BatchSpanProcessor({ exporter });
}

/** Logs the first failed export and stays quiet after, so a missing collector costs one line. */
function warnOnce(inner: SpanExporter, endpoint: string, warn: Warn): SpanExporter {
  let warned = false;
  return {
    export(spans, done) {
      inner.export(spans, (result) => {
        if (result.code !== EXPORT_SUCCESS && !warned) {
          warned = true;
          warn("cannot export traces; further OTLP errors are silent", {
            endpoint,
            error: result.error ?? "export failed",
          });
        }
        done(result);
      });
    },
    shutdown: () => inner.shutdown(),
    forceFlush: () => inner.forceFlush?.() ?? Promise.resolve(),
  };
}
