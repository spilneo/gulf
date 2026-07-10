# Installation

Gulf requires Rust **1.96**. The documentation site requires Node **22 or newer**.

```sh
git clone https://github.com/spilneo/gulf.git
cd gulf
cp .env.example .env
cargo run -- check
```

The default configuration is `config/gulf.toml`. Override it with `GULF_CONFIG` or the global `--config` option:

```sh
gulf --config /etc/gulf/gulf.toml check
```

Set only the environment variables needed by your selected providers. `GULF_EVM_FEE_RECIPIENT` and other recipient variables are public on-chain addresses; provider credentials and `GULF_EVM_PRIVATE_KEY` are secrets. See [configuration](/guide/configuration) and [security](/operations/security).

For a published container, see [Docker](/guide/docker). For Rust dependency use, see the [library guide](/guide/rust-library).
