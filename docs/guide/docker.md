# Docker

Pull the published image:

```sh
docker pull ghcr.io/spilneo/gulf:latest
docker run --rm ghcr.io/spilneo/gulf:latest check
```

Run the API with a mounted configuration and only the environment values needed by that configuration:

```sh
docker run --rm -p 127.0.0.1:8080:8080 \
  -v "$PWD/config/gulf.toml:/etc/gulf/gulf.toml:ro" \
  -e GULF_ODOS_API_KEY \
  -e GULF_EVM_FEE_RECIPIENT=0xFeeRecipient \
  ghcr.io/spilneo/gulf:latest serve --listen 0.0.0.0:8080
curl http://127.0.0.1:8080/health
```

> **Warning:** the API has no authentication. The loopback-only port binding above is intentional. Do not expose it publicly; if public access is required, place it behind a trusted reverse proxy that enforces authentication and rate limits.

The image runs as a non-root `gulf` user and includes a `gulf check` health check. Do **not** pass `GULF_EVM_PRIVATE_KEY` to the API container: the server never signs. Build locally with `docker build -t gulf .`.
