# Architecture

Gulf is a configuration-driven routing layer. The registry resolves chain and token aliases, then routes a quote or preparation request to either a **generic HTTP provider** described by TOML or a **native adapter** for provider-specific protocol logic.

```text
request → registry → provider mapping/native adapter → provider response
        ← normalized quote / prepared action plan ←
```

Quotes normalize output amount, dynamic and configured fees, gas estimate, settlement estimate, and provider error. Preparation normalizes transaction fields and produces an `ExecutionPlan` with actions, warnings, expiry, and optional status handle. Plans cover EVM transactions, approval-plus-transaction flows, native deposits and memos, serialized transactions, typed signatures, intent orders, and unsupported forms.

The CLI may locally sign supported EVM actions only. The HTTP API has read, quote, prepare, and RPC-proxy routes; it never receives a private key and cannot submit a signed transaction. An API consumer signs the returned raw action externally and broadcasts with its own wallet or infrastructure.

Configuration is data, not an assurance that a provider can quote every route. Consult [providers](/concepts/providers), inspect [JSON responses](/reference/json-responses), and apply [security controls](/operations/security).
