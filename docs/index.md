---
layout: home

hero:
  name: Gulf
  text: One routing surface for bridges and swaps
  tagline: Config-driven providers, normalized quote and execution plans, local-only EVM signing.
  actions:
    - theme: brand
      text: Get started
      link: /guide/installation
    - theme: alt
      text: HTTP API
      link: /reference/http-api

features:
  - title: Provider-neutral
    details: Generic HTTP mappings and native adapters produce a common quote and action-plan model.
  - title: Key-safe server
    details: The HTTP API prepares transactions and proxies RPC calls. It never receives signing keys.
  - title: Operationally explicit
    details: Inspect configured routes, obtain a live quote, review every action, then submit locally.
---

## Start with intent, inspect the plan

Gulf resolves configured chains, tokens, providers, fees, and recipient data from TOML. Use the [CLI](/guide/cli) for a local EVM execution, the [Rust library](/guide/rust-library) in an application, or the [HTTP API](/reference/http-api) to prepare an externally signed transaction.

> Quotes, liquidity, gas, fees, and settlement timing are volatile. Treat every live result as route-specific rather than a guarantee.

Read the [security guidance](/operations/security) before submitting an irreversible transaction, and the [live-test record](/operations/live-testing) for evidence captured July 10, 2026.
