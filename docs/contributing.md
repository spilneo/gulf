# Contributing

Contributions should preserve Gulf's config-driven provider model, normalized response types, and local-only signing boundary.

Install the repository pre-commit hook with `./scripts/install-git-hooks.sh`.

The hook runs `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, and `python3 -m unittest discover -s scripts/tests -p 'test_*.py'`.

```sh
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
cd docs && npm ci && npm run build
```

Run `cargo run -- check` after changing configuration. Add focused tests for parsing, provider mappings, response normalization, and error paths. Test fixtures represent controlled inputs only; do not claim a fixture is live verification.

When adding or changing a provider, document its exact current ID, required environment variables, route constraints, commission behavior, execution shape, and status behavior. Keep secrets out of commits and reports. Update the relevant guide, reference, and live-testing evidence only when it is verified.

Open a pull request with a concise explanation, tests run, and any operational implications. The docs workflow builds VitePress and deploys the main branch to GitHub Pages.
