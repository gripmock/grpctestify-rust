# BENCH

File-level benchmark configuration for `grpctestify bench`.

`BENCH` is optional, can appear at most once per file, and is recommended as the first section (or right after `META`).

## What BENCH controls

- How load is generated (`mode`, `concurrency`, schedules, limits).
- How benchmark runtime behaves (warmup, stop policy, progress heartbeat).
- Which pass/fail thresholds are applied.

`BENCH` is for runtime mechanics. Use `META.tags` for scenario labeling/grouping.

## Precedence

- `bench` command model: `CLI bench flags > BENCH section > bench defaults`.
- Example: `--concurrency 64` overrides `BENCH.concurrency: 16`.

## Minimal example

```gctf
--- BENCH ---
mode: fixed
concurrency: 16
duration: 60s
max_rps: 200
load_schedule: const
duration_stop: wait
progress_interval: 5s
thresholds.latency_ms.p(95): <120
thresholds.error_rate_pct: <1.0
```

## Key format rules

- Use canonical `snake_case` keys only.
- Hyphen-case keys in `BENCH` are treated as unknown keys.
- Unknown/typo keys get suggestions (for example, `did you mean 'load_schedule'?`).

## Keys by responsibility

- Core: `mode`, `name`
- Stop/load: `requests`, `duration`, `max_duration`, `max_rps`
- Scheduler: `load_schedule`, `load_start`, `load_step`, `load_end`, `load_step_duration`, `load_max_duration`
  - `sine` shape adds: `load_midpoint`, `load_amplitude`, `load_frequency`
  - `spike` shape adds: `load_spike_target`, `load_spike_after`, `load_spike_duration`
- Concurrency sweep: `concurrency_schedule`, `concurrency_start`, `concurrency_end`,
  `concurrency_step`, `concurrency_step_duration`
- Runtime/transport: `concurrency`, `connections`, `connect_timeout`, `request_timeout`, `keepalive`, `cpus`
- Methodology: `ramp_up`, `warmup`, `skip_first`, `count_errors_in_latency`, `duration_stop`, `latency_percentiles`, `progress_interval`
- Validation cost: `assert_mode`, `no_assert`, `sample_rate`
- Cache: `cache`, `cache_ttl`
- Thresholds: `thresholds.<metric>`

## Key reference

- `mode`: load execution strategy (`fixed`, `stepping`, `adaptive`; compat values `closed`, `open` are still accepted).
- `name`: optional run label in benchmark reports.
- `concurrency`: number of parallel workers.
- `connections`: number of transport connections; must be `> 0` and `<= concurrency`. Left unset it
  is `min(concurrency, cores)` capped at 8.

  A gRPC connection is a shared mutable object: every stream on it contends for one per-connection
  state mutex inside the HTTP/2 layer, and a profile of the client puts that mutex at the top of its
  own wait time. Splitting the pool is what removes the contention. Measured against a reference
  tonic server with CPU headroom on both sides (8-core M1 Pro, 12 s runs):

  | concurrency | 1 connection | 2 | 4 | 8 | 16 |
  | --- | --- | --- | --- | --- | --- |
  | 25 | 28 443 rps | 45 453 | 57 714 | **65 912** | — |
  | 50 | 30 758 rps | 49 757 | 63 992 | **75 673** | — |
  | 200 | 35 907 rps | 58 696 | 81 387 | **93 502** | 96 370 |

  Client CPU per request falls with it — 85.3 → 47.6 µs at concurrency 25 — so this is contention
  removed, not work redistributed. The curve flattens past eight, which is where the cap sits.

  Against a target that is *itself* the bottleneck the extra connections cost about 2.6 % (17 340
  rps at one connection versus 16 883 at eight, measured against a heavier mock). Raise
  `connections` when the generator is the constraint and lower it when the target is.
- `connect_timeout`: connection timeout duration.
- `request_timeout`: per-request deadline. Defaults to the run `duration` (30s when only `requests` is
  set), so a server slower than the run window reports timeouts instead of slow successes. Rounded down
  to whole seconds, minimum 1s.
- `keepalive`: keepalive interval.
- `cpus`: tokio worker threads for the run (CLI: `--cpus`, or `TOKIO_WORKER_THREADS`).
  Defaults to `min(cores, 4)` for `bench`. More worker threads cost CPU *and* instructions per
  request — work-stealing, wakeups and atomics — without adding throughput. Measured on an 8-core
  M1 Pro, 40 k unary requests at `--concurrency 50 --connections 8`, all three reaching the same
  ~41 k rps:

  | worker threads | client cores | rps/core | instructions/request |
  | --- | --- | --- | --- |
  | 2 | 1.35 | 30 420 | 210 259 |
  | 4 (the default) | 2.05 | 20 246 | 219 429 |
  | 8 | 2.67 | 15 311 | 242 815 |

  The tax scales with core count, so on a large host leaving this unbounded is much worse than the
  table suggests. Lower it when the generator, not the target, is the constraint.
- `assert_mode`: assertion execution policy (`full`, `sampled`, `off`; compat aliases are accepted).
- `no_assert`: disables assertion checks for transport baseline. It no longer prints response
  bodies — that made the flag meant to strip cost 41 % slower than a normal run. Use `-v` to see
  them.
- `sample_rate`: sampled assertion/detail rate in `[0,1]`.
- `duration_stop`: in-flight policy at duration deadline (`close`, `wait`, `ignore`).
- `skip_first`: exclude first N samples from latency stats.
- `count_errors_in_latency`: include failed calls in latency aggregates (`true/false/1/0`).
- `latency_percentiles`: comma-separated percentile list (for example `p50,p90,p95,p99`).
- `progress_interval`: progress heartbeat interval.
- `cache`: cache mode (`on`, `off`, `refresh`; also `true/false/1/0`).
- `cache_ttl`: cache lifetime duration.

## Value sets

- `mode`: `fixed`, `stepping`, `adaptive` (compat: `closed`, `open`)
- `load_schedule`: `const`, `step`, `line`, `sine`, `spike`, `custom`
- `concurrency_schedule`: `const`, `step`, `line`
- `duration_stop`: `close`, `wait`, `ignore`
- `assert_mode`: `full`, `sampled`, `off` (compat: `fail_fast`, `collect_all`, `skip`)
- `cache`: `on`, `off`, `refresh` (also `true`, `false`, `1`, `0`)

## Thresholds

- Key pattern: `thresholds.<metric>`.
- Expression forms: `<N`, `<=N`, `>N`, `>=N`.
- Dynamic percentile metrics are supported:
  - `thresholds.p(95)`
  - `thresholds.latency_ms.p(99.9)`
- Unknown threshold metric fails deterministically (non-silent failure).

### Metrics

| Metric | Meaning |
| --- | --- |
| `count` | requests issued |
| `ok`, `errors` | transport outcome — requests whose gRPC status was / was not `OK` |
| `passed`, `failed` | document verdict — requests whose RESPONSE/ERROR/ASSERTS held or did not |
| `pass_rate_pct`, `fail_rate_pct` | the verdict counters as a percentage of `count` |
| `error_rate_pct` | `errors` as a percentage of `count` |
| `rps` (`rps_observed`, `throughput`) | observed requests per second |
| `average_ms` / `average_ns` (`avg_*`) | mean latency |
| `fastest_ms` / `slowest_ms` (`min_*` / `max_*`, `_ns` variants) | latency extremes |
| `total_ns` | summed latency |
| `p(N)`, `latency_ms.p(N)`, `latency_ns.p(N)` | latency percentiles |

`passed`/`failed` are not the same as `ok`/`errors`: a document asserting
`--- ERROR partial ---` passes while its request carries a non-`OK` status. Gate a
negative-path benchmark on `pass_rate_pct`, not on `error_rate_pct`.

```gctf
thresholds.rps: > 10000
thresholds.pass_rate_pct: >= 100
thresholds.latency_ms.p(99): < 25
```

## A document must have something to verify

`bench` refuses a document with no `RESPONSE`, `ERROR` or `ASSERTS` section, as `run` already did.

Without one the runner never reads the response, so the run reports requests *sent* rather than
completed — a rate several times higher than the target can actually serve — and each unread call is
left running. Measured on a 3-second run at `--concurrency 200`: 8.6 GB resident and 690 k "rps"
against 55 MB and 75 k rps for the same document with `--- RESPONSE partial --- {}` added.

Add `--- RESPONSE partial ---` with `{}` to accept any reply while still waiting for it. Direct-call
mode (`bench --call`) does this for you.

## Scaling with a data source

A primary source is streamed, never loaded, so memory does not follow the file. Measured with
`--calibrate --concurrency 200 --connections 8`, one placeholder substituted per request:

| rows | file | rps | instructions/request | peak RSS |
| --- | --- | --- | --- | --- |
| 1 000 | 42 KB | 74 565 | 254 187 | 57 MiB |
| 100 000 | 4.2 MB | 75 764 | 254 339 | 54 MiB |
| 1 000 000 | 42 MB | 76 152 | 254 071 | 56 MiB |
| 10 000 000 | 416 MB | 72 021 | 254 907 | 59 MiB |
| 100 000 000 | 4.1 GB | 68 930 | 256 521 | 55 MiB |

Resident memory is flat across a 100 000x range in source size, and the per-request cost is flat
too. The ~9 % throughput drop at the largest size is the page cache, not the client: at 4.1 GB the
file no longer fits in it.

Dimension sources are the ones that may be loaded — see [Memory budget](../../bench-sources#memory-budget).

## Calibration

`grpctestify bench --calibrate` runs the document against a built-in no-op gRPC target instead of the
configured address, and reports what that costs — a loopback baseline for this document on this
host.

Read it for what it is. The target runs **inside the same process**, so its work lands in the same
CPU counter and both ends compete for the same cores: `client_cost` in a calibration run covers the
client *and* the target, and the throughput is depressed by the sharing. The baseline is therefore
neither an upper nor a lower bound on a real run — it is a reference point for comparing builds and
documents against each other, measured with the target's cost held constant.

```bash
grpctestify bench service.gctf --calibrate --duration 12s --concurrency 200
```

The built-in target answers every method — unary, client-, server- and bidirectional-streaming — with
default-valued messages, so assertions are switched off for the run. The document still needs a
`PROTO` section: descriptors are never taken from the no-op target.

## Client cost

Every run measures its own CPU and reports it next to the target's numbers, so a result can be
audited rather than taken on trust. No other gRPC load generator does this; k6 documents the concern
but leaves the check to the operator.

```text
   • Requests/sec: 17415.30
   • Client cost:  72.7 µs CPU/request, 1.27/8 cores, 13765 rps/core
```

In the JSON report the same figures appear under `client_cost`:

| Field | Meaning |
| --- | --- |
| `cpu_seconds` | CPU the generator burned during the run |
| `cpu_us_per_request` | that cost divided by requests issued |
| `cores_used` | `cpu_seconds / wall_seconds` — how many cores the generator occupied |
| `host_cores` | cores available |
| `rps_per_core` | observed throughput per core the generator used |
| `generator_limited` | `true` when the run says more about this client than about the target |
| `limits` | why, one string per reason |

`generator_limited` trips at 80 % of the host's cores. Above that the throughput a run reports is the
generator's ceiling, and the target's real capacity is unknown — rerun with more `connections` and
fewer `cpus`, or split the load across machines.

The inverse reading matters too: a low `cores_used` with disappointing throughput means the target,
not the client, is the constraint.

## Source tracking in reports

Resolved benchmark options include source tags in report metadata:

- `cli`
- `bench_section`
- `default`

These are emitted in `options_resolved` so the effective value is explainable.

## Profiles

A profile is a named preset of BENCH keys, applied with `grpctestify bench --profile <name>`. Profiles set
a baseline; anything the `BENCH` section or a CLI flag specifies still wins.

Precedence: `CLI flags > BENCH section > --profile preset > built-in defaults`.

### Built-in profiles

| Profile | Purpose | Key settings |
| --- | --- | --- |
| `functional` | Quick functional check (the default) | `mode: fixed`, `concurrency: 1`, `requests: 100`, `duration: 30s` |
| `load` | Stepped load test 50→200 RPS | `mode: stepping`, `concurrency: 10`, `duration: 60s`, `load_schedule: step`, `load_start: 50`, `load_step: 10`, `load_end: 200`, `load_step_duration: 10s` |
| `stress` | Linear stress test 10→500 RPS | `mode: stepping`, `concurrency: 50`, `duration: 120s`, `load_schedule: line`, `load_start: 10`, `load_step: 5`, `load_end: 500` |
| `spike` | Spike test 10→500→10 RPS | `mode: fixed`, `concurrency: 100`, `duration: 60s`, `load_schedule: spike`, `load_start: 10`, `load_spike_target: 500`, `load_spike_after: 30`, `load_spike_duration: 10` |
| `soak` | Long-duration soak at 50 RPS | `mode: fixed`, `concurrency: 5`, `duration: 3600s`, `load_schedule: const`, `load_start: 50` |

### Flags

- `--profile <name>`: apply a built-in or custom profile.
- `--list-profiles`: print every available profile (built-in + custom) with its description, then exit.
- `--profile-file <path>`: load custom profiles from a YAML file. A custom profile may `extends` another
  to inherit its keys.

### Example

```bash
grpctestify bench service.gctf --profile stress
grpctestify bench --list-profiles
```

## Related

- [Command Line](../api/command-line)
- [Test File Format](../api/test-files)
- [META](./meta)
