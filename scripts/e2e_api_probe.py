#!/usr/bin/env python3
"""Probe an already-running Gulf API using real configured provider requests."""

from __future__ import annotations

import argparse
import json
import os
import sys
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

from e2e_common import coverage_result, failure_diagnostic, Probe, api_request, load_probes, report_entry, write_report


def request_json(base_url: str, path: str, method: str, timeout: float, payload: object = None) -> object:
    body = None if payload is None else json.dumps(payload).encode("utf-8")
    request = Request(base_url.rstrip("/") + path, data=body, method=method)
    if body is not None:
        request.add_header("Content-Type", "application/json")
    with urlopen(request, timeout=timeout) as response:
        return json.load(response)


def run_probe(args: argparse.Namespace, probe: Probe) -> dict[str, object]:
    if any(not os.environ.get(name) for name in probe.env_required):
        return report_entry(probe.name, probe.kind, "SKIP", error="required environment variables are not set")
    path, payload = api_request(probe)
    try:
        response = request_json(args.base_url, path, "POST", args.timeout, payload)
    except (HTTPError, URLError, TimeoutError, OSError, json.JSONDecodeError) as error:
        body = ""
        if isinstance(error, HTTPError):
            try:
                body = error.read().decode("utf-8", errors="replace")
            except OSError:
                pass
        return report_entry(probe.name, probe.kind, "FAIL", error=failure_diagnostic("API request failed", body, error))
    if probe.kind == "quote":
        quotes = response.get("quotes", []) if isinstance(response, dict) else []
        quote = next((item for item in quotes if isinstance(item, dict) and item.get("provider_id") == probe.expected_provider), None)
        valid = quote is not None
        reason = "expected provider was not returned"
        if valid and probe.request["live"]:
            assert quote is not None
            if quote.get("live_response") is None:
                valid, reason = False, "expected provider has no live response"
            elif not isinstance(quote.get("normalized"), dict):
                valid, reason = False, "expected provider has no normalized quote"
            elif quote["normalized"].get("error") is not None:
                valid, reason = False, f"expected provider reported an error: {quote['normalized']['error']}"
    else:
        swap = response.get("swap") if isinstance(response, dict) else None
        valid = isinstance(swap, dict) and swap.get("provider_id") == probe.expected_provider
        reason = "expected provider was not returned"
    return report_entry(probe.name, probe.kind, "PASS" if valid else "FAIL", response, None if valid else reason)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base_url")
    parser.add_argument("config")
    parser.add_argument("--probes", help="separate TOML file containing [e2e] probe declarations")
    parser.add_argument("--no-coverage-check", action="store_true", help="do not require probes for every enabled runtime provider")
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--report")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    results: list[dict[str, object]] = []
    try:
        probes = load_probes(args.probes or args.config)
        if not args.no_coverage_check:
            results.append(coverage_result(args.config, probes))
        health = request_json(args.base_url, "/health", "GET", args.timeout)
        providers = request_json(args.base_url, "/v1/providers", "GET", args.timeout)
        chains = request_json(args.base_url, "/v1/chains", "GET", args.timeout)
        tokens = request_json(args.base_url, "/v1/tokens", "GET", args.timeout)
        valid = (
            health == {"status": "ok"}
            and isinstance(providers, dict)
            and isinstance(providers.get("providers"), list)
            and isinstance(chains, dict)
            and isinstance(chains.get("chains"), list)
            and isinstance(tokens, dict)
            and isinstance(tokens.get("tokens"), list)
        )
        results.append(report_entry("api-basics", "api", "PASS" if valid else "FAIL", {"health": health, "providers": providers, "chains": chains, "tokens": tokens}, None if valid else "unexpected API response"))
        results.extend(run_probe(args, probe) for probe in probes)
    except (HTTPError, URLError, TimeoutError, OSError, json.JSONDecodeError, ValueError) as error:
        results.append(report_entry("api-basics", "api", "FAIL", error=failure_diagnostic("API basics request failed", stderr=error)))
    write_report(args.report, results)
    for result in results:
        print(f"{result['status']}: {result['kind']} {result['name']}")
    summary = {status: sum(result["status"] == status for result in results) for status in ("PASS", "FAIL", "SKIP")}
    print("Summary: " + " ".join(f"{status}={count}" for status, count in summary.items()))
    return 1 if summary["FAIL"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
