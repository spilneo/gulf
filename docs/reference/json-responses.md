# JSON responses

These are representative sanitized shapes, not schemas and not route guarantees. Volatile values were captured as examples on July 10, 2026.

## Quote

```json
{"quotes":[{"provider_id":"odos","provider_name":"Odos","estimated_fee_bps":0.0,"commission_bps":20.0,"protocol_fee_bps":0.0,"owner_fee_bps":20.0,"partner_fee_bps":0.0,"integrator_fee_bps":0.0,"total_configured_bps":20.0,"dynamic_fee":true,"settlement_seconds":20,"docs":["https://docs.odos.xyz/"],"normalized":{"output_amount":"228576588550011","estimated_fee_bps":null,"estimated_fee_usd":null,"gas_fee_usd":null,"settlement_seconds":20,"error":null},"live_response":{}}]}
```

`normalized` is optional. Its fields are `output_amount`, `estimated_fee_bps`, `estimated_fee_usd`, `gas_fee_usd`, `settlement_seconds`, and `error`; values can be null when a provider does not return them.

## Prepared plan and local receipt

```json
{"swap":{"provider_id":"odos","provider_name":"Odos","request":{"from_chain":"base","to_chain":"base","from_token":"USDC","to_token":"WETH","amount":"100000","sender":"0xYourAddress","recipient":null,"live":true,"partner_fee":null},"normalized_transaction":{"to":"0xRouter","data":"0x…","value":"0"},"execution":{"kind":"evm_approval_then_transaction","actions":[{"chain":"eip155:8453","action_type":"approval","description":"Approve token spending","to":"0xUSDC","data":"0x…","value":"0","token":"base-usdc","spender":"0xRouter","amount":"100000","deposit_address":null,"memo":null,"serialized_transaction":null,"raw":{}},{"chain":"eip155:8453","action_type":"transaction","description":"Submit provider transaction","to":"0xRouter","data":"0x…","value":"0","token":null,"spender":null,"amount":null,"deposit_address":null,"memo":null,"serialized_transaction":null,"raw":{}}],"status":null,"expires_at":null,"warnings":[]},"transaction":{},"raw":{},"docs":["https://docs.odos.xyz/"]}}
```

```json
{"receipts":[{"transaction_hash":"0xbb0f400c66a8e2ad44240956c8a6435f119824dbd66d815659c0924d9f37cd79","chain":"eip155:8453","block_number":48460747,"status":"success","action_type":"approval"},{"transaction_hash":"0xa7d787e0ae41d9e5c06a508f65b996f60c2793c28e7b5232cceb5e97fd54aee6","chain":"eip155:8453","block_number":48460747,"status":"success","action_type":"transaction"}]}
```

## Circle settlement and errors

Circle status is external to the Gulf API; a representative pending-to-complete status shape is:

```json
{"status":"pending_confirmations","sourceTxHash":"0xae3fcf09cf07829e4dede98b75515b4511037653adf1300463a0d2eaf9e5a4ad","destinationTxHash":null}
{"status":"complete","sourceTxHash":"0xae3fcf09cf07829e4dede98b75515b4511037653adf1300463a0d2eaf9e5a4ad","destinationTxHash":"0x1ec1c377cba05c9eacd16b3df8b84f41ad77cbf358bc4a44a578a462f0f2fe67"}
```

All Gulf API errors use:

```json
{"error":{"code":"bad_request","message":"request could not be processed"}}
```
