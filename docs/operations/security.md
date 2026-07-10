# Security

Blockchain actions are irreversible. Gulf helps structure a route; it cannot make a provider, quote, wallet, or destination safe.

- Keep `GULF_EVM_PRIVATE_KEY` in a protected local environment for the CLI only. Never put it in an HTTP request, server environment, container serving the API, report, issue, or log.
- `gulf swap execute` requires `--yes`; treat it as an acknowledgement, not a safety check. Review a freshly prepared plan before using it.
- The HTTP API never receives or uses server signing keys. It prepares actions and proxies RPC; sign raw transactions externally in your wallet or custody boundary.
- Validate sender, recipient, token contract, chain, raw amount, allowance spender, transaction target, calldata, value, expiry, and every multi-step action. Confirm the selected provider ID.
- Sanitize reports and captured JSON: remove API keys, private endpoints, wallet details, and sensitive provider payloads.
- Use least-value live tests. Confirm a source receipt and then destination settlement; bridges can remain pending after source success.
- Set only needed provider credentials, use private RPC endpoints where appropriate, and keep configuration safety limits restrictive.

Read [errors](/reference/errors) for operational failures and [commissions](/guide/commissions) before enabling partner fees.
