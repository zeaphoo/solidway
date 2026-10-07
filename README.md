<div align="right">

**English** · [中文](README.zh.md)

</div>

# Solidway

**A Rust LLM gateway that forwards requests unchanged.**

Solidway sits between your application and your model providers. Request bodies, response bodies, header order and streaming frame boundaries travel through it byte for byte, while failover, load balancing, circuit breaking and telemetry run alongside the forward path.

---

> ## Status
>
> The open-source gateway and the enterprise edition are being split out of a single deployment. **No code has been published yet.**
>
> Watch or star this repository to be notified when the first release lands.

---

## Contents

- [Why this exists](#why-this-exists)
- [Features](#features)
- [What Solidway does not do](#what-solidway-does-not-do)
- [Comparison with other gateways](#comparison-with-other-gateways)
- [Design notes](#design-notes)
- [Status](#status)

---

## Why this exists

Most gateways earn their place by changing your traffic. They take what your SDK produced, translate it into a unified schema, and render it into whichever provider dialect you are calling. That buys vendor independence, and it costs predictability.

| What gets given up | Why it matters |
|---|---|
| Prompt cache hits | Rewriting a request prefix invalidates the provider's cache, which raises both cost and latency |
| New parameters | A parameter a provider shipped this week may be dropped by the gateway until its adapter catches up |
| Faithful debugging | What you observe is the gateway's rendering of your request, not your request |
| Byte-level guarantees | Content passes through gateway memory, where a middleware layer can alter it |

Solidway is built around the opposite constraint: **the forward path does not touch the payload.**

Routing, health and accounting are decided per request, outside the bytes you send.

---

## Features

### Forwarding

- Byte-for-byte request and response bodies
- Streaming frames passed through with their original chunk boundaries
- No schema translation, no dropped fields, no rewritten prompts
- Header order preserved

### Reliability

- **Failover** — ordered fallback chains across providers, models and keys. Each attempt carries its own timeout budget, so a slow primary cannot consume the whole request deadline.
- **Load balancing** — weighted distribution across keys and endpoints, with health-aware scoring that adjusts to observed latency and error rates rather than a fixed table.
- **Circuit breaking** — a target that reports a credential, quota, access or rate-limit failure is held out of rotation, and returns to service after a single probe confirms it can serve again.

### Operations

- **Self-hosted** — one static binary. Docker, Kubernetes or bare metal, inside your own network. No external control plane is contacted at runtime.
- **Analytics** — requests, tokens, cost and latency broken down by provider, model, key and workspace, so spend can be attributed to the team that produced it.
- **Capacity alerts** — threshold and trend alerts for quota usage, error rate and p99 latency, delivered to email, Slack or a webhook before a limit is reached.
- **Observability** — OpenTelemetry (OTLP), Prometheus scraping, structured JSON logs and webhooks.

### Runtime

- No garbage collector, so the tail of the latency distribution does not carry collection pauses
- Throughput scales with available cores; steady-state memory is bounded by the configured connection pools rather than by a heap
- One file to ship, for x86-64 and arm64

---

## What Solidway does not do

Stated plainly, because it decides whether this is the right tool for you:

- **It does not translate between provider dialects.** Calling Claude through the OpenAI SDK, or any model through another vendor's SDK, is out of scope by design.
- **It does not ship a prebuilt adapter for every provider.** A provider is reached by configuration, not by an adapter module.

If either of those is a requirement, one of the projects in the next section is a better fit.

---

## Comparison with other gateways

All of these are LLM gateways, but they answer a different question: *what should the gateway do to a request on its way through?* The table lists what could be verified against each project itself.

| | **Solidway** | **Bifrost** | **LiteLLM** |
|---|---|---|---|
| What the gateway does to a request | Forwards it unchanged | Converts to a unified schema, then to the provider format | Converts to a unified schema, then to the provider format |
| Implementation language | Rust | Go | Python |
| Calling vendor A with vendor B's SDK | Not supported, by design | Supported | Supported |
| Adding a provider | Configuration change | Adapter code | Adapter code |
| Failover | Built in | Built in | Built in |
| Load balancing | Built in | Built in; adaptive balancing is a commercial-edition feature | Built in, with several published strategies |
| Circuit breaking | Built in | A commercial-edition feature | Deployment cooldown and retry budgets |
| Deployment shape | One static binary | One static binary or a container | A Python service or a container |

Where a capability is offered only in a commercial edition, the table says so rather than marking it absent.

### When one of these is the better choice

- You want to call Claude through the OpenAI SDK, or switch dialects without touching application code → **Bifrost** or **LiteLLM**
- You need ready-made adapters across a long list of providers → **LiteLLM**
- You want to change gateway behaviour in Python, in the same language as the rest of your stack → **LiteLLM**
- You want a unified schema plus governance, budgets and a plugin system around it → **Bifrost**

### Sources

Each row was checked against the projects themselves. Products change; check their documentation before deciding.

- **Bifrost** — source and documentation reviewed in October 2026 · <https://github.com/maximhq/bifrost>
- **LiteLLM** — version 1.82.0, MIT licensed · <https://github.com/BerriAI/litellm>

---

## Design notes

### Transparent by design

The request path has three stages, and only the middle one is ours.

```
Client  ──►  Solidway  ──►  Provider
              routing
              accounting
              health
```

- **No schema translation.** What your SDK sends is what the provider receives. A model parameter a provider ships works the day it ships, without waiting for a gateway release.
- **Streaming stays streaming.** Server-sent event frames are forwarded as they arrive, so client-side parsing and incremental rendering behave the same with and without the gateway in the path.
- **Routing decisions are explicit.** Which target served a request, why a fallback was taken, and how long each attempt took are recorded in the routing trail attached to that request.

### How a failure is classified

The class decides whether a target is retried, rotated or held. Errors caused by the request itself never hold a target out of rotation.

| Class | Retry | Hold |
|---|---|---|
| Transient (network, 5xx) | Same target, with backoff | No |
| Rate limit | Rotate key | Yes, short |
| Credential rejected | Rotate key, no backoff | Yes |
| Quota exhausted | Rotate key, no backoff | Yes |
| Model not accessible | Rotate key, no backoff | Yes, that model |
| Request rejected by provider | No | No |

---

## Status

|  | Status |
|---|---|
| Open-source gateway | Being split out |
| Enterprise edition | Being split out |
| First release | Not yet scheduled |
| License | To be determined |
| Documentation | Not yet published |

The open-source gateway and the enterprise edition are being separated from a single deployment. Capabilities that end up in the enterprise edition will be listed explicitly rather than left implied, and the comparison table above already marks commercial-edition features as such for the other projects.

**敬请稍后 — please check back shortly.**

---

## Contact

If you would like to talk through where a forward path fits in your architecture, open an issue on this repository.
