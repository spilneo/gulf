# Live testing

This page records environment-specific evidence captured **July 10, 2026**. It is not a performance, availability, or price guarantee. Use low-value tests and re-quote immediately before an action.

## Coverage and Docker API probe

The comprehensive 24-provider coverage probe passed; coverage is distinct from provider operation totals. The last provider run recorded 19 passed, 4 failed, and 2 skipped operations. Passed live probes: Circle CCTP Standard, Circle CCTP Fast V2, Odos, Socket V3, Across, deBridge DLN, THORChain, Squid, 0x, Symbiosis, LI.FI, Velora, ChangeNOW, Orbiter, Allbridge Pool, Allbridge CCTP, Eco Routes, and Rhino.fi. Meson was subsequently executed after an allowance precondition.

OpenOcean and Rango failed with Cloudflare 403. Mayan quote failed. Rubic was skipped for a missing API key; Chainflip was skipped because no operator broker endpoint was available. All 24 IDs still had probe coverage.

The Docker image built locally. Docker API comprehensive testing passed health/basic operations plus 17 provider operations; OpenOcean and Rango again failed Cloudflare access, and missing-key/infrastructure probes were skipped. The Docker API did not sign transactions.

## Executed transactions

**Base Odos swap.** Gulf approved then swapped 0.1 USDC to `228576588550011` WETH wei. Approval: `0xbb0f400c66a8e2ad44240956c8a6435f119824dbd66d815659c0924d9f37cd79` in Base block `48460743`. Swap: `0xa7d787e0ae41d9e5c06a508f65b996f60c2793c28e7b5232cceb5e97fd54aee6` in Base block `48460747`.

**Base → Arbitrum Meson bridge.** 1.0 USDC left Base and 0.9 USDC arrived on Arbitrum. Gulf approval: `0x3a52ec0d6954fa84f996191287de22d73d9514cf5c80b43152bb886c970f118b`; bridge: `0x575b170500c9b0b8ec2a53318a868104f35e464e783cf78c0a46a33d3c6bbe34`, source block `48462311`. Meson rejected prepare before a preliminary allowance transaction; the allowance had to be confirmed before its route could be prepared and executed.

**Circle CCTP sandbox Sepolia → Base Sepolia.** 1 USDC was burned; `795488` units arrived after a `204512` forwarding fee. Approval: `0x2650e9ceb2b6a99b8e07492b55467ef0e6f6b785b20db9146729dbf7dcfe4e7d`; burn: `0xae3fcf09cf07829e4dede98b75515b4511037653adf1300463a0d2eaf9e5a4ad`; forwarding destination transaction: `0x1ec1c377cba05c9eacd16b3df8b84f41ad77cbf358bc4a44a578a462f0f2fe67`. Circle reported complete.

For repeatable probe inputs, use repository test fixtures only as fixtures—not as live-route guarantees.
