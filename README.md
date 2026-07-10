# Gulf

Config-driven bridge and swap routing for command-line, Rust, and HTTP integrations. Gulf normalizes quotes and executable action plans from generic HTTP providers and native adapters; EVM signing happens only in the local CLI.

- **Docs:** https://spilneo.github.io/gulf/
- **Crate:** https://crates.io/crates/gulf
- **Image:** `ghcr.io/spilneo/gulf`

## Quick start

Requires Rust 1.96. Copy `.env.example` to `.env`, set the credentials and fee recipients for providers you use, then:

```sh
cargo run -- check
cargo run -- providers
cargo run -- quote --from-chain base --to-chain base \
  --from-token USDC --to-token WETH --amount 100000 \
  --sender 0xYourAddress --live
```

To submit a supported EVM plan locally, set `GULF_EVM_PRIVATE_KEY` and use `gulf swap execute ... --yes`. The HTTP API prepares plans and proxies RPC calls; it never accepts or stores private keys.

See the [documentation](https://spilneo.github.io/gulf/) for configuration, API, Docker, operational guidance, and live-test evidence captured July 10, 2026.

## Development

Install the repository pre-commit hook with `./scripts/install-git-hooks.sh`.

The hook runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`.

## License

Apache-2.0 OR MIT.
