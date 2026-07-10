# HTTP API

Start the API with `gulf serve --listen 127.0.0.1:8080`. It binds to loopback by default. It prepares transactions and proxies configured chain JSON-RPC; it **never accepts private keys or signs**.

| Method | Path | Purpose |
| --- | --- | --- |
| GET | `/health` | Service health |
| GET | `/v1/providers` | Configured provider summaries |
| GET | `/v1/chains` | Configured chains |
| GET | `/v1/tokens` | Configured tokens |
| POST | `/v1/rpc/{chain}` | Proxy an allowlisted JSON-RPC request to a configured chain endpoint |
| GET, POST | `/v1/quotes` | Get quotes from enabled providers |
| POST | `/v1/swaps/prepare` | Build a provider action plan |

```sh
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:8080/v1/providers
curl http://127.0.0.1:8080/v1/chains
curl http://127.0.0.1:8080/v1/tokens
```

## Quotes

GET query parameters are `from_chain`, `to_chain`, `from_token`, `to_token`, `amount`, and `sender`; optional parameters are `recipient`, `live`, `partner_id`, `partner_fee_bps`, and `partner_recipient`.

```sh
curl -G http://127.0.0.1:8080/v1/quotes \
  --data-urlencode from_chain=base --data-urlencode to_chain=base \
  --data-urlencode from_token=USDC --data-urlencode to_token=WETH \
  --data-urlencode amount=100000 --data-urlencode sender=0xYourAddress \
  --data-urlencode live=true
```

POST accepts a `QuoteRequest`:

```sh
curl http://127.0.0.1:8080/v1/quotes -H 'content-type: application/json' -d '{
  "from_chain":"base", "to_chain":"base", "from_token":"USDC", "to_token":"WETH",
  "amount":"100000", "sender":"0xYourAddress", "recipient":"0xRecipient",
  "live":true, "partner_fee":null
}'
```

## Prepare and RPC

```sh
curl http://127.0.0.1:8080/v1/swaps/prepare -H 'content-type: application/json' -d '{
  "provider_id":"odos", "quote": {
    "from_chain":"base", "to_chain":"base", "from_token":"USDC", "to_token":"WETH",
    "amount":"100000", "sender":"0xYourAddress", "recipient":null,
    "live":true, "partner_fee":null
  }
}'

curl http://127.0.0.1:8080/v1/rpc/base -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}'
```

RPC accepts one JSON-RPC object only; batches are rejected. Allowed methods are `eth_chainId`, `eth_blockNumber`, `eth_getBalance`, `eth_call`, `eth_estimateGas`, `eth_getCode`, `eth_getTransactionCount`, `eth_getTransactionByHash`, `eth_getTransactionReceipt`, and `eth_sendRawTransaction`. The latter permits externally signed transaction submission; the API never accepts keys or signs transactions.

Validate every returned address, chain, amount, calldata, and action before external signing. Response envelopes and errors are documented in [JSON responses](/reference/json-responses) and [errors](/reference/errors).
