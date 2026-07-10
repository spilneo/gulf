# Rust library

Add Gulf from the crate registry:

```toml
[dependencies]
gulf = "0.1.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Load configuration and request a live quote:

```rust
use gulf::{Gulf, QuoteRequest};

let gulf = Gulf::from_source("config/gulf.toml").await?;
let request = QuoteRequest {
    from_chain: "base".into(), to_chain: "base".into(),
    from_token: "USDC".into(), to_token: "WETH".into(),
    amount: "100000".into(), sender: "0xYourAddress".into(),
    recipient: Some("0xRecipient".into()), live: false, partner_fee: None,
};
let quotes = gulf.live_quote(request.clone()).await?;
```

Prepare a selected provider plan for external signing:

```rust
use gulf::PrepareSwapRequest;
let prepared = gulf.prepare_swap(PrepareSwapRequest {
    quote: request, provider_id: Some("odos".into()),
}).await?;
```

`Gulf::execute` performs local EVM signing from the named environment variable; do not use it in a service that accepts untrusted signing requests. `providers`, `chains`, `tokens`, `rpc_endpoints`, and `call_rpc` expose the same registry capabilities as the API.
