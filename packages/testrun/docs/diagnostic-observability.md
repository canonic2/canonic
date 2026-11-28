# Remote observability and deployed incidents

Use the existing investigation, collector, analysis and query tools for remote
logs, metrics, traces and field reports. Start with `diagnostics capabilities`:
`observability` describes supported exports and `observability_http` describes
configured live readers. Discovery reads no provider files or credentials.

## Choose the evidence route

| Source | Direct configured GET | Explicit retained import |
| --- | --- | --- |
| Prometheus | Fixed metric range, literal scope labels, step and series cap | `prometheus-json`: matrix/vector JSON |
| Loki | Literal scope labels and finite log window | `loki-json`: streams JSON |
| Sentry | Exact account/project/event ID, matching release/environment/window | `sentry-json`: selected project event JSON |
| OpenTelemetry / Tempo | Unregistered | `otlp-json`: resource spans, logs and metrics |
| Jaeger | Unregistered | `jaeger-json`: trace/span JSON export |
| CloudWatch | Unregistered | `cloudwatch-json`: log events, Logs Insights results or GetMetricData pages |
| Firebase Crashlytics | Unregistered | `crashlytics-json`: selected BigQuery JSON event rows |
| Android Vitals / Play reporting | Unregistered | `play-vitals-json`: MetricsRow response |
| Apple field diagnostics | Existing exported-report owner | `apple-ips`, `apple-metrickit` |
| Grafana Cloud | Compatible configured Prometheus/Loki endpoint | OTLP, Prometheus, Loki or Jaeger exports |
| Datadog / New Relic / Honeycomb | Native REST readers unregistered | Explicit OTLP JSON export |
| Deployment/configuration/audit owner | Unregistered | `deployment-evidence-v1` declarations |

Imports use `source:{kind:"import",path:"/absolute/export.json",format:"otlp-json"}`.
Explicit exports can contain private customer data. Testrun retains their original
bytes privately; inspect or share only the selected evidence. No binary OTLP,
automatic SDK installation, collector listener, fleet agent, cloud login or
operational remediation is registered. Existing tools own exporting and access.

## Configure one bounded source

The existing credential owner supplies an absolute regular configuration file
owned by the current Unix user, with no group/other permission bits (usually
mode 0600), and optionally a separate private bearer file. Final file symlinks
are refused. Bearer values must contain at least 16 characters without controls.
Testrun reads only the selected paths; it never discovers, refreshes or upgrades
credentials. Configured reads currently require Unix; other hosts use imports.

Example private Loki configuration (replace paths and selectors explicitly):

```json
{
  "schemaVersion": 1,
  "sourceId": "api-incident",
  "scope": {"account":"acme","project":"api","environment":"staging","region":"eu","resource":"api","release":"v1"},
  "endpoint": "https://logs.example.com",
  "authorizationOwner": "existing-observability-owner",
  "authorization": {"kind":"bearer_file","path":"/private/acme/provider-token"},
  "read": {"kind":"loki","scopeLabels":{"account":"account","project":"project","environment":"environment","region":"region","resource":"resource","release":"release"}},
  "timeoutMs": 3000,
  "maxWindowSeconds": 600,
  "maxRecords": 100,
  "maxBytes": 65536
}
```

`authorization:{kind:"none"}` is available for an explicitly configured owner
proxy. HTTPS endpoints can include a prefix path; plain HTTP is restricted to
numeric loopback with an explicit port. Redirects, ambient proxies, endpoint
scanning, arbitrary methods/queries, polling and retries are disabled.

Open an investigation, then collect with the same exact scope:

```sh
testrun diagnostics open --submission-id incident --question 'Which retained traces accompany the Acme failure?'
testrun diagnostics start INVESTIGATION --submission-id remote --source '{"kind":"observability_http","configFile":"/private/acme/provider.json","sourceId":"api-incident","scope":{"account":"acme","project":"api","environment":"staging","region":"eu","resource":"api","release":"v1"},"window":{"from":"2026-01-01T00:00:00Z","to":"2026-01-01T00:05:00Z"}}'
```

All six scope fields and explicit UTC `Z` endpoints are required. There is no
production default. Config/source mismatch refuses before reading credentials or
sending a request. Prometheus/Loki inject all six exact escaped label matches
and reject returned series with contradictory labels or out-of-window timestamps.
The backend must expose those labels; missing labels require an export route.

Prometheus `read` uses `kind:"prometheus"`, literal `metric`, declared `unit` and
`definition`, `stepSeconds` (1–3600), `maxSeries` (1–32), and the same `scopeLabels`.
The worst selected sample/series count must fit `maxRecords`. There is no free
PromQL expression. Sentry `read:{kind:"sentry_event"}` uses account/project slugs
and a 32-hex event ID in `scope.resource`; release, environment and event time
must agree with the response. Region remains a configured routing declaration,
not authenticated residency evidence.

Limits are at most one day, five seconds, 1 MiB and 2,000 result records per
read. One GET retains one page. HTTP 401/403 stops with denial; 429 reports a
numeric Retry-After when supplied, without retrying. Other errors retain no
provider body or redirect location. Response/time limits do not bound backend
scanned bytes or billing when those are unknown. Cancellation can leave a
bounded HTTP worker pending briefly and cannot prove server-side query cancellation.

Live reads omit Loki text (retaining its digest), extra labels/unknown response
fields and Sentry customer payloads by default. `includeText:true` explicitly
retains text/customer fields. Named credential fields and the selected bearer
literal are scrubbed from the retained packet, including keys. This is a bounded
projection, not universal secret detection. Scope labels remain private evidence.

## Analyze and join retained observations

After collection settles, use the existing analysis workflow:

```sh
testrun diagnostics analyze INVESTIGATION COLLECTOR --submission-id parsed
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"view":"observability"}'
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"correlationId":"trace:11111111111111111111111111111111"}'
```

The view returns exact reported trace IDs, span counts, duplicate span IDs,
missing parents and witness ordinals. Use the same analysis with `view:"rows"`
and `cursor` to inspect witnesses; the response identifies collector and raw
digest. Logs/errors and metric exemplars can share trace IDs. An exemplar links
one sample, not the whole aggregate. Exact IDs establish association, not cause;
duplicates can reflect repeated exports and missing parents sampling/retention.
Different collectors/analyses require explicit retained composition; this view
does not search every investigation or contact providers.

OTLP keeps resource/service/version/image/environment, instrumentation scope,
schema URLs, attributes, severity/status, span events/links, original nanosecond
strings, metric kind, unit, temporality, monotonicity and raw datapoints. Numeric
seconds are approximate ordering values; clocks are not proven synchronized.
Histograms/quantiles remain raw, without invented percentiles or rate conversion.
Large integer samples that cannot be represented exactly have no numeric value.
Multi-service traces preserve each native resource version; an outer query scope
does not relabel every client/server span as the same service/release.

## Retain provenance and deployment observations

`observability-evidence-v1` wraps 1–8 explicitly exported pages with:
`schemaVersion:1`, `provider`, `sourceId`, the six-field `scope`, `window`,
`authorizationOwner`, `request`, native `format`, `coverage` (`complete`, `partial`
or `unknown`), `limits:{maxRecords,maxBytes,timeoutMs}`, and
`scan:{bytes,cost,meaning}`. Optional `configSha256`, per-page `responseSha256`,
declared Prometheus `unit`/`definition`, and up to 16 HTTPS `links` retain source
references. Each page is `{payload:...}` with optional response digest. Known
`scan.maxBytes`/`maxCost` exceeding their budgets refuses analysis. Reported costs,
coverage and digests remain producer claims; parsing cannot authenticate them.
Links have query/fragment removed and are never followed. Query request metadata
in explicit imports must already be sanitized by its owner.

`deployment-evidence-v1` contains `schemaVersion:1` and `records`, each with
`kind` (`deployment`, `feature`, `configuration`, `resource_state` or `audit`),
`id`, `resource`, `release`, RFC3339 `time`, and optional `imageId`,
`artifactSha256`, `configSha256`, `details`. Digest fields use 64 hex characters.
Rows expose `deployment:ID` and `release:RESOURCE:RELEASE` correlations. Resource
names must agree with OTLP service names to share release correlations. Imported
declarations and current local source do not prove what ran at incident time.

CloudWatch pages retain status/statistics/continuation evidence in raw storage;
Play rows retain dimensions, DateTime timezone and aggregate values without
inventing an exact UTC instant. Crashlytics accepts a JSON array or `rows` array
of `event_id`, optional RFC3339 `event_timestamp`, `event_type`, `issue` and
`application`. Broader provider schemas require a compatible selected export or
an explicit unsupported result.

Zero results, a full/truncated page, expired retention, denied access, sampling,
late arrivals and aggregate suppression never establish incident-wide absence
or app health. Keep these gaps beside the evidence. No provider/account live
acceptance, billing calibration or cross-host compatibility campaign is implied
by the owned fixture checks.
