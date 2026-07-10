#!/usr/bin/env python3
"""Run configured Gulf CLI probes against real providers; never fabricates responses."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

from e2e_common import coverage_result, failure_diagnostic, Probe, load_probes, prepare_cli_command, quote_cli_command, report_entry, write_report


def run_probe(args: argparse.Namespace, probe: Probe) -> dict[str, object]:
    missing = [name for name in probe.env_required if not os.environ.get(name)]
    if missing:
        return report_entry(probe.name, probe.kind, "SKIP", error="required environment variables are not set")
    command = (prepare_cli_command if probe.kind == "prepare" else quote_cli_command)(args.gulf, args.config, probe, args.cargo_run)
    try:
        completed = subprocess.run(command, capture_output=True, text=True, timeout=args.timeout, check=False)
    except subprocess.TimeoutExpired as error:
        return report_entry(probe.name, probe.kind, "FAIL", error=failure_diagnostic("subprocess timed out", error.stdout, error.stderr))
    except OSError:
        return report_entry(probe.name, probe.kind, "FAIL", error="CLI could not be executed")
    if completed.returncode:
        return report_entry(probe.name, probe.kind, "FAIL", error=failure_diagnostic(f"CLI exited with status {completed.returncode}", completed.stdout, completed.stderr))
    try:
        response = json.loads(completed.stdout)
    except json.JSONDecodeError:
        return report_entry(probe.name, probe.kind, "FAIL", error=failure_diagnostic("CLI did not return JSON", completed.stdout, completed.stderr))
    if probe.kind == "quote":
        quote = next((item for item in response if isinstance(item, dict) and item.get("provider_id") == probe.expected_provider), None) if isinstance(response, list) else None
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
        valid = isinstance(response, dict) and response.get("provider_id") == probe.expected_provider
        reason = "expected provider was not returned"
    return report_entry(probe.name, probe.kind, "PASS" if valid else "FAIL", response, None if valid else reason)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("config")
    parser.add_argument("--probes", help="separate TOML file containing [e2e] probe declarations")
    parser.add_argument("--no-coverage-check", action="store_true", help="do not require probes for every enabled runtime provider")
    parser.add_argument("--gulf", default="gulf", help="built Gulf executable")
    parser.add_argument("--cargo-run", action="store_true", help="invoke cargo run instead of --gulf")
    parser.add_argument("--timeout", type=float, default=45.0)
    parser.add_argument("--report")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    try:
        probes = load_probes(args.probes or args.config)
        results = [] if args.no_coverage_check else [coverage_result(args.config, probes)]
        results.extend(run_probe(args, probe) for probe in probes)
    except (OSError, ValueError) as error:
        print(f"FAIL: configuration could not be loaded ({type(error).__name__})", file=sys.stderr)
        return 2
    write_report(args.report, results)
    for result in results:
        print(f"{result['status']}: {result['kind']} {result['name']}")
    summary = {status: sum(result["status"] == status for result in results) for status in ("PASS", "FAIL", "SKIP")}
    print("Summary: " + " ".join(f"{status}={count}" for status, count in summary.items()))
    return 1 if summary["FAIL"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
