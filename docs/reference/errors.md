# Errors and status handling

Gulf returns structured API errors as `{ "error": { "code", "message" } }`. A malformed request or provider/configuration processing failure is HTTP **400** with `bad_request`; an unknown route is **404** `not_found`; a method not registered for a route is **405** `method_not_allowed`.

| Condition | Response or action |
| --- | --- |
| Missing environment variable | Set the provider credential, recipient, or RPC variable required by the active config; retry only after validating it. |
| Unsupported route | Choose a configured provider, chain, and token combination; inspect `providers`, `chains`, and `tokens`. |
| Provider minimum | Increase amount or select another route; do not split blindly because each transfer has fees. |
| Missing ERC-20 allowance | Submit and confirm the prepared approval, then prepare again. Meson can reject preparation before allowance. |
| Cloudflare or rate limit | Treat it as provider access failure, back off, and use authorized access or another route. OpenOcean and Rango returned Cloudflare 403 in the recorded probe. |
| Unavailable broker or local Allbridge API | Restore the required Chainflip broker endpoint or local Allbridge Core REST service, then retry. |
| Reverted transaction | Stop, inspect the on-chain receipt and plan; do not automatically resubmit. |
| Pending settlement | Source success is not destination completion. Poll the provider/status handle and wait for final settlement. |

Provider payloads can expose an error in `normalized.error` even when other quote fields are present. Preserve raw provider detail privately for diagnosis, but sanitize it before sharing. See [security](/operations/security) and [live testing](/operations/live-testing).
