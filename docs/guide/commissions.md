# Commissions and partner fees

Gulf calculates configured owner and partner fees in basis points (bps), subject to `[safety].max_commission_bps`, provider maximums, and `[developer.b2b]` limits. A provider can render the normalized amount into its own field: for example, Across uses a decimal `appFee`, while deBridge uses `affiliateFeePercent`.

```sh
gulf quote --from-chain base --to-chain arbitrum \
  --from-token USDC --to-token USDC --amount 1000000 \
  --sender 0xYourAddress --recipient 0xRecipient --live \
  --partner-id reseller-42 --partner-fee-bps 15 \
  --partner-recipient 0xPartnerRecipient
```

The quote exposes `commission_bps`, `owner_fee_bps`, `partner_fee_bps`, `integrator_fee_bps`, and `total_configured_bps`; provider and network charges remain dynamic. A partner ID enables the partner request. `partner_fee_bps` and `partner_recipient` are optional in the request but configuration can require a recipient.

Native commission support is provider-specific: Socket receives `feeBps` and `feeTakerAddress`; Across receives `appFee`, `appFeeRecipient`, and `integratorId`; deBridge receives `affiliateFeePercent` and `affiliateFeeRecipient`; THORChain encodes affiliate fields in its memo. Circle CCTP has no native affiliate parameter; its configured strategy is a wrapper contract. Verify the prepared request and commercial eligibility before treating a fee as collected.
