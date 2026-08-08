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
- Runtime/transport: `concurrency`, `connections`, `connect_timeout`, `request_timeout`, `keepalive`, `cpus`
- Methodology: `ramp_up`, `warmup`, `skip_first`, `count_errors_in_latency`, `duration_stop`, `latency_percentiles`, `progress_interval`
- Validation cost: `assert_mode`, `no_assert`, `sample_rate`
- Cache: `cache`, `cache_ttl`
- Thresholds: `thresholds.<metric>`

## Key reference

- `mode`: load execution strategy (`fixed`, `stepping`, `adaptive`; compat values `closed`, `open` are still accepted).
- `name`: optional run label in benchmark reports.
- `concurrency`: number of parallel workers.
- `connections`: number of transport connections; must be `> 0` and `<= concurrency`.
- `requests`: stop after N requests (request-count mode).
- `duration`: stop after duration (time mode).
- `max_duration`: hard cap in request-count mode.
- `max_rps`: global requests-per-second cap.
- `ramp_up`: gradual load ramp before steady phase.
- `warmup`: warmup window excluded from final metrics.
- `load_schedule`: schedule shape (`const`, `step`, `line`, `sine`, `spike`, `custom`).
- `load_start`: starting RPS for schedule.
- `load_step`: RPS increment/slope for schedule.
- `load_end`: optional end RPS for schedule.
- `load_step_duration`: duration per step for `step` schedule.
- `load_max_duration`: max time window for schedule adjustments.
- `load_midpoint`, `load_amplitude`, `load_frequency`: baseline RPS, swing, and frequency for the `sine` schedule.
- `load_spike_target`, `load_spike_after`, `load_spike_duration`: peak RPS, delay before the spike, and how
  long it lasts for the `spike` schedule.
- `connect_timeout`: connection timeout duration.
- `request_timeout`: per-request deadline. Defaults to the run `duration` (30s when only `requests` is
  set), so a server slower than the run window reports timeouts instead of slow successes. Rounded down
  to whole seconds, minimum 1s.
- `keepalive`: keepalive interval.
- `cpus`: optional CPU pinning hint.
- `assert_mode`: assertion execution policy (`full`, `sampled`, `off`; compat aliases are accepted).
- `no_assert`: disables assertion checks for transport baseline.
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
