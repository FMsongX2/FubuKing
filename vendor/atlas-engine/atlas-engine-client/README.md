<!-- Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md. -->
# atlas-engine-client

Higher-level request policy layered on `atlas-engine-http-client` without any Atlas Agent/OpenAI API awareness.

- Provides retry utilities (`RetryPolicy`, `RetryOn`, `run_with_retry`, `backoff`) that callers plug into for unary and streaming calls.
- Supplies the `sse_stream` helper to turn byte streams into raw SSE `data:` frames with idle timeouts and surfaced stream errors.
- Defines the request telemetry callback used by higher-level clients.
- Re-exports the low-level HTTP types temporarily so consumers can migrate to `atlas-engine-http-client` incrementally.
