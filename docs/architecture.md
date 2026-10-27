<div align="right">

**English** · [中文](architecture.zh.md)

</div>

# Architecture

How Solidway is put together. Behaviour is specified in [`openspec/specs/`](../openspec/specs/); design rationale lives in [`docs/adr/`](adr/).

## Process layout

One process hosts two planes:

```text
                    ┌──────────────────────────────┐
  LLM clients ─────►│ data plane (solidway-core)   │─────► provider endpoints
  (scripts, SDKs)   │ forward · select · health    │
                    │ usage accounting             │
                    └──────────────┬───────────────┘
                                   │ events
                    ┌──────────────▼───────────────┐
  Console ─────────►│ management plane             │
  (browser)         │ API · auth · SQLite · TLS    │
                    └──────────────────────────────┘
```

The data plane holds all live state in memory: credential health, cooldown deadlines, selection metrics and session affinity. It publishes events that the management plane turns into usage records, incidents and alerts.

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `solidway-core` | Data plane. Forwarding, selection scoring, health and probing, usage accounting, affinity. No database, no HTTP management surface. | — |
| `solidway-api` | Management plane. SQLite schema and queries, HTTP API, session auth, provider catalog, TLS/ACME, alerts, console hosting. | core |
| `solidway` | Binary. Parses `data_root` and lifecycle flags, wires the two planes together, handles signals. | api |
| `solidway-egress` | Outbound request shaping for provider dialects that need it. | transcode |
| `solidway-transcode` | Protocol conversion primitives. Present in-tree; **not wired into the forward path** today. | — |

Front end and shell:

| Path | Role |
|---|---|
| `web/` | React console (Vite, shadcn-style components, i18next). Embedded into the binary in release builds. |
| `app/` | Tauri 2 desktop shell. Separate cargo workspace, excluded from the root workspace. |

## Request lifecycle

```text
inbound match ──► resolve route (by API key) ──► select credential
      │                                              │
      │                                    score = f(weight, latency, errors, cooldown)
      │                                              │
      └──────────────► forward upstream ◄─────────────┘
                            │
             first byte?  ──┴──► commit: no further failover
                            │
                    stream frames through
                            │
                 record usage + release slot
```

Failover happens only before the first byte reaches the client; after that the response is committed and an upstream failure is surfaced as an error rather than spliced.

## Data model

SQLite, two databases:

- `solidway.db` — users and sessions, routes, provider credentials, API keys, settings, provider health, incidents, runtime snapshot.
- `usage_detail.db` — per-request detail rows, retention and sampling configured in settings.

Provider credentials are encrypted with AES-256-GCM; the data key is derived from `master.key`. Console sessions are server-side and cookie-based.

## State and restarts

- Health, cooldown and affinity state are in memory, seeded from the runtime snapshot written during shutdown.
- Shutdown persists usage and the snapshot within a fixed budget, then exits.
- Selection metrics are aggregated from recent attempt buckets, so restarting does not reset observed quality.

## Console and desktop shell

- The console is a SPA that talks only to `/api/*`; the data plane (`/v1/*`) is never touched by it.
- The desktop shell supervises the gateway as a child process, reads its machine-readable stdout events (`solidway: …`), and opens the console in a webview.
- `--mode desktop` makes the process exit when its supervisor closes stdin, so a crashed or killed shell leaves no orphan.
