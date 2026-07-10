# Providers

Configured support is determined by the active TOML registry, credentials, chain/token mapping, and provider route availability. It is not a promise that a live request will succeed.

## Configured provider IDs

`circle-cctp-standard`, `circle-cctp-fast-v2`, `openocean-v4`, `odos`, `socket-v3`, `mayan-swift-v2`, `across-swap-api`, `debridge-dln`, `thorchain`, `chainflip-broker`, `squid-router`, `rango-main-api`, `zero-x-swap-api`, `symbiosis-api`, `lifi`, `velora-market`, `changenow`, `rubic`, `orbiter`, `rhino-fi`, `meson`, `allbridge-pool`, `allbridge-cctp-oft`, and `eco-routes`.

Use `gulf providers --json` for enabled metadata, fee estimates, and docs; use `gulf quote --live` for the route you intend to use. Generic HTTP providers are configured by endpoint mappings. Circle CCTP, Meson, THORChain, ChangeNOW, Chainflip, Rhino.fi, Squid, and other protocol-specific cases use native handling where required.

## July 10, 2026 probe evidence

The comprehensive coverage probe passed: all 24 provider IDs had probe coverage. Provider operation totals are separate: 19 probes passed, 4 failed, and 2 were skipped in the last run.

Live provider probes passed for Circle CCTP Standard, Circle CCTP Fast V2, Odos, Socket V3, Across, deBridge DLN, THORChain, Squid, 0x, Symbiosis, LI.FI, Velora, ChangeNOW, Orbiter, Allbridge Pool, Allbridge CCTP, Eco Routes, and Rhino.fi. Meson initially failed preparation because its allowance was absent, then completed a live execution after allowance; it is live-execution verified with that precondition.

OpenOcean and Rango were blocked by Cloudflare 403 responses. Mayan quote failed and is not claimed as verified. Rubic was skipped because its API key was missing; Chainflip was skipped because an operator broker endpoint was unavailable. The [live-testing record](/operations/live-testing) includes executed transactions and Docker API coverage.
