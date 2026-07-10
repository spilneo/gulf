# Configuration

`config/gulf.toml` is the routing registry. It defines safety limits, chains and RPC priorities, token aliases, providers, HTTP request mappings, and commission strategies. Gulf resolves `${env.NAME}` from the process environment.

```toml
[developer]
default_commission_bps = 20

[developer.recipients]
evm = "${env.GULF_EVM_FEE_RECIPIENT}"
solana = "${env.GULF_SOLANA_FEE_RECIPIENT}"

[developer.b2b]
enabled = true
default_partner_fee_bps = 0
max_partner_fee_bps = 100
require_partner_recipient = false

[safety]
max_commission_bps = 300
require_live_quote_for_swap = true
```

Chains use CAIP-2 IDs and aliases. Tokens have a chain, decimals, address where applicable, aliases, and provider-specific symbols. RPC endpoints are tried by priority; an environment endpoint can take precedence over configured public fallbacks.

```toml
[[chains]]
caip2_id = "eip155:8453"
display_name = "Base"
vm = "evm"
aliases = ["base", "8453"]
numeric_chain_id = 8453
rpc_endpoints = [
  { name = "env-primary", url_env = "GULF_BASE_RPC_URL", priority = 0, timeout_ms = 4000 },
  { name = "official", url = "https://mainnet.base.org", priority = 20, timeout_ms = 5000 }
]

[[tokens]]
id = "base-usdc"
chain_caip2_id = "eip155:8453"
symbol = "USDC"
decimals = 6
address = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
```

`require_live_quote_for_swap` rejects swap preparation and local execution unless `quote.live` is true. Use raw base-unit amounts (`1000000` for 1 USDC) in requests. Run `gulf check`, `gulf chains --json`, `gulf tokens --json`, and `gulf providers --json` after an edit. Provider-specific variables are enumerated in `.env.example`; missing credentials or required infrastructure fail the affected route. [Commissions](/guide/commissions) explains fee inputs.
