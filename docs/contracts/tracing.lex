Cross-Tool Tracing Contract

    Civitas Forge commands use W3C Trace Context and W3C Baggage to keep one
    live trace across tool and process boundaries. Binaries create and export
    spans. Libraries describe their work through the Rust `tracing` facade and
    use the subscriber installed by their caller.

1. Identifiers

    Tools emit W3C `traceparent` version `00`. The encoded value is
    `00-<trace-id>-<parent-id>-<trace-flags>`: the trace ID is 32 lowercase hex
    digits, the parent ID is 16 lowercase hex digits, and the flags are two
    lowercase hex digits. An all-zero trace ID or parent ID is invalid.

    Receivers validate the complete value according to W3C Trace Context
    [https://www.w3.org/TR/trace-context/]. A malformed or unsupported
    `traceparent` supplies no parent and its `tracestate` is discarded. A tool
    starts a new trace when it has no valid parent.

2. Process Propagation

    A process reads `TRACEPARENT`, `TRACESTATE`, and `BAGGAGE` once at startup.
    Their values use the W3C Trace Context and W3C Baggage encodings
    [https://www.w3.org/TR/trace-context/] [https://www.w3.org/TR/baggage/].
    Environment variable names are uppercase because they cross process
    boundaries; HTTP propagation uses the lowercase header names the W3C
    specifications define.

    A binary with tracing enabled makes its command span a child of the valid
    inbound `TRACEPARENT`. Before it starts a child process, it writes a new
    `TRACEPARENT` for the span that starts that child and passes the current
    `TRACESTATE` and `BAGGAGE`. The child therefore descends from the operation
    that started it.

    A binary with tracing disabled emits no span and passes all three incoming
    variables unchanged. Every tool preserves baggage entries it does not own,
    including their properties. A tool may add or replace a key it owns, but it
    does not reinterpret, rename, or delete another owner's key. Invalid
    baggage members may be dropped as W3C Baggage permits; a member is never
    truncated into a different value.

3. Baggage Namespace

    Registered Civitas Forge keys:
        | Key | Owner | Value |
        | `comitia.change_request` | Comitia | The repository and change-request number as `owner/repository#number`. |
        | `comitia.round` | Comitia | The review round as a non-zero decimal integer. |
        | `oratio.session_id` | Oratio | The Oratio session ID exactly as Oratio records it. |

    A repository owns keys beginning with its lowercase tool name and a dot.
    Adding a shared key requires an additive change to this table that names
    one owner and one stable value encoding. An owner may change its key's
    value during a request when the named fact changes. Keys do not carry
    credentials, personal data, prompts, source content, or other unbounded
    values.

4. Span Emission

    Binaries install the OpenTelemetry subscriber and exporter. They export
    spans with OTLP and read exporter endpoint, protocol, headers, timeout,
    sampling, and resource settings from the standard `OTEL_*` environment
    variables. An unset OTLP traces endpoint leaves export off. Setting
    `OTEL_SDK_DISABLED=true` also leaves export off.

    Each command invocation creates one root command span. Its
    `service.name` is the executable's tool name, such as `comitia` or
    `oratio`, and does not vary by subcommand. Libraries depend only on the
    `tracing` facade: they do not install a subscriber, select an exporter, or
    read `OTEL_*` configuration.

    Span names identify operations rather than arguments. Spans do not record
    credentials, request bodies, source content, or command arguments by
    default. A tool may add bounded identifiers needed to locate an operation,
    subject to the same restrictions as baggage values.

    Interprex GitHub provider spans use
    `interprex.provider.<domain>.<operation>` and set
    `interprex.provider.name=github`. Each public provider-trait method creates
    one of these spans before it checks credentials or contacts GitHub.

5. Deterministic Re-export

    A tool may derive and export a trace after the live operation has ended.
    The derived export keeps the deterministic trace and span IDs its stored
    format defines, so retrying the export addresses the same observations.

    When stored data retains the inbound live context, the derived root span
    links to that context and records it as attributes. The exporter does not
    copy the live trace ID into the derived trace and does not name a span in a
    different trace as its parent. Oratio tape exports follow this rule: each
    segment keeps its derived identity and links its root to the inbound
    `traceparent` recorded for that run.
