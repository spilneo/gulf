# CLI

All commands load `config/gulf.toml` unless `--config` or `GULF_CONFIG` overrides it. `--json` affects list and execution output; quote and prepare output is JSON.

```sh
gulf check
gulf --json providers
gulf --json chains
gulf --json tokens
```

## Quote and prepare

```sh
gulf quote --from-chain base --to-chain base \
  --from-token USDC --to-token WETH --amount 100000 \
  --sender 0xYourAddress --recipient 0xRecipient --live

gulf swap prepare --provider-id odos \
  --from-chain base --to-chain base --from-token USDC --to-token WETH \
  --amount 100000 --sender 0xYourAddress --recipient 0xRecipient --live
```

`--recipient` defaults to `--sender`. `--live` requests live provider data. Use `--partner-id`, `--partner-fee-bps`, and `--partner-recipient` to request a partner fee within configured limits.

## Execute locally

Only the CLI signs supported EVM action plans. It reads the key from `GULF_EVM_PRIVATE_KEY` by default, or a variable selected with `--private-key-env`; it requires the explicit `--yes` acknowledgement.

```sh
export GULF_EVM_PRIVATE_KEY=0x...
gulf --json swap execute --provider-id odos \
  --from-chain base --to-chain base --from-token USDC --to-token WETH \
  --amount 100000 --sender 0xYourAddress --live --yes
```

Review the prepared `execution.actions` first. An `evm_approval_then_transaction` plan submits approval before its transaction. Execution does not make a bridge settled; use the provider status information where supplied. See [JSON responses](/reference/json-responses) and [security](/operations/security).

## Serve

```sh
gulf serve --listen 127.0.0.1:8080
```

The server has no signing endpoint. See the complete [HTTP API](/reference/http-api).
