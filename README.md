<div align="right">

**English** · [中文](README.zh.md)

</div>

# Solidway

**A Rust LLM gateway that forwards requests unchanged.**

Solidway sits between your application and your model providers. Request bodies, response bodies and streaming frame boundaries travel through it byte for byte, while failover, account health and usage attribution run alongside the forward path.

It is built for a specific shape of deployment: an organization owns a set of model accounts, puts them in a pool, and lets its own employees and scripts share them. Accounts belong to you; Solidway never sells access to models.

[![CI](https://github.com/zeaphoo/solidway/actions/workflows/ci.yml/badge.svg)](https://github.com/zeaphoo/solidway/actions/workflows/ci.yml)

---

## Contents

- [Features](#features)
- [What Solidway does not do](#what-solidway-does-not-do)
- [Failure classification](#failure-classification)
- [Comparison with other gateways](#comparison-with-other-gateways)
- [Distribution](#distribution)
- [Status](#status)
- [License](#license)
- [Further reading](#further-reading)

---

## Features

### Forwarding

- Three inbound protocol families, each forwarded on its own terms: OpenAI Chat Completions (`/v1/chat/completions`), OpenAI Responses (`/v1/responses`), Anthropic Messages (`/v1/messages`).
- Request and response bodies preserved byte for byte, with one exception: the top-level `model` field is rewritten when a credential maps a client-facing alias to a different upstream id. No prompt rewriting, so provider prompt caches keep working.
- Streaming frames pass through with their original chunk boundaries.
- No cross-protocol translation on the forward path: a client speaks the dialect its provider expects.

### Reliability

- **Pooled accounts** — a route is a pool of provider credentials. Selection is weighted and health-aware (observed latency, error rate, cooldown state) rather than a fixed table.
- **Failover before the first byte** — if a response has not started, the gateway switches to another credential and the client never sees the failed attempt.
- **Failure classification** — a rejected credential (`401`) retires an account until an operator restores it; rate limits, exhausted quota and access errors put it into a timed cooldown. Errors caused by the request itself never remove an account from rotation.
- **Probing** — held-out accounts are checked by a probe and return to rotation on their own once they can serve again.

### Operations

- **Self-hosted, one static binary** — data plane and management plane are one process with the console embedded. No external control plane is contacted at runtime.
- **Console** — routes, provider credentials (masked, encrypted at rest), API keys, per-route and global usage, incidents, users, system settings and built-in TLS. Chinese and English.
- **Usage attribution** — requests, tokens, latency and error rate broken down by route, API key, provider, model and owner, so spend can be attributed to the team that produced it.
- **Incidents and alerts** — provider failures are recorded with duration and attempt counts; alerts are delivered to a webhook with retry and per-provider throttling.
- **Built-in HTTPS** — certificates issued and renewed through ACME (`http-01` or `tls-alpn-01`) for domains you configure, or terminate TLS in front of Solidway.
- **Provider catalog** — presets generated from models.dev, including endpoint templates for Vertex, Azure and Bedrock (SigV4), with model discovery from the upstream.

## What Solidway does not do

- **No schema translation.** If you want to call Claude through the OpenAI SDK, this is the wrong tool.
- **No content moderation, no prompt rewriting, no response rewriting.**
- **No resale of model access.** It is a gateway for accounts you already own.
- **No container images, no Prometheus/OTLP exporter, no email/Slack alerting (webhook only).** See [Status](#status).

## Failure classification

| Class | Retry | Hold |
|---|---|---|
| Transient (network, 5xx) | Same target, with backoff | No |
| Rate limit | Rotate credential | Yes, timed cooldown |
| Credential rejected (401) | Rotate credential, no backoff | Yes, until an operator restores it |
| Quota exhausted | Rotate credential, no backoff | Yes, timed cooldown |
| Model not accessible | Rotate credential, no backoff | Yes, for that model |
| Request rejected by provider | No | No |

## Comparison with other gateways

| | Solidway | LiteLLM | Bifrost |
|---|---|---|---|
| Primary goal | Forward unchanged | Unified schema | Unified schema + governance |
| Language | Rust | Python | Go |
| Calls vendor A with vendor B's SDK | Not supported, by design | Supported | Supported |
| Adding a provider | Configuration change | Adapter code | Adapter code |
| Failover | Built in | Built in | Built in |
| Load balancing | Built in, health-aware | Built in | Built in |
| Circuit breaking | Built in | Commercial edition | Deployment cooldown |
| Deployment shape | One static binary | Service or container | Binary or container |

Sources: Bifrost and LiteLLM project documentation, reviewed October 2026. Products change; check their docs before deciding.

## Distribution

- **Server** — one static binary, distributed as `deb` and `rpm` packages with a systemd unit, or run from the release archive. Linux.
- **Desktop** — a Tauri application for Linux, Windows and macOS that supervises the gateway locally and opens the console in a webview; each package ships the gateway binary.
- **Upgrades** — settings, accounts and usage live in a local SQLite database, so upgrading means replacing the binary or package.

## Status

| | |
|---|---|
| Version | `0.5.0-rc1`, pre-1.0 — interfaces may change between minor releases |
| Source | Complete implementation in this repository |
| Platforms | Linux (server + desktop), macOS (desktop), Windows (desktop) |
| License | AGPL-3.0-only |
| Not yet available | Container images, Prometheus/OTLP, email/Slack alerting, cross-protocol transcode wiring, code signing/notarization |

## License

AGPL-3.0-only. You can run, modify and self-host it freely. If you offer a modified version to others **as a network service**, the AGPL requires you to give those users access to the corresponding source.

Running Solidway internally, or as a service to your own employees, carries no such obligation. Commercial support and licensing for cases the AGPL does not fit are available — open an issue or contact the maintainer.

## Further reading

- [Getting started](docs/getting-started.md) — build, run, deploy, desktop packages, data directory and backups
- [Architecture](docs/architecture.md) — process layout, crates, data model
- [Contributing](CONTRIBUTING.md) — development workflow and required checks
- [Design decisions](docs/adr/), [domain glossary](docs/domain-context.md), [behaviour specs](openspec/specs/)
