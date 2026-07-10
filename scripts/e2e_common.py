"""Shared stdlib-only support for honest Gulf integration probes."""

from __future__ import annotations

import json
import os
import re
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit, urlunsplit


@dataclass(frozen=True)
class Probe:
    name: str
    kind: str
    expected_provider: str
    request: dict[str, Any]
    env_required: tuple[str, ...]


def load_probes(config_path: str | Path) -> list[Probe]:
    """Load quote and prepare probe declarations from an e2e TOML config."""
    with Path(config_path).open("rb") as config_file:
        document = tomllib.load(config_file)
    e2e = document.get("e2e", {})
    if not isinstance(e2e, dict):
        raise ValueError("[e2e] must be a TOML table")
    probes: list[Probe] = []
    for kind, table_name in (("quote", "quote_probes"), ("prepare", "prepare_probes")):
        entries = e2e.get(table_name, [])
        if not isinstance(entries, list):
            raise ValueError(f"e2e.{table_name} must be an array of tables")
        for index, entry in enumerate(entries, start=1):
            if not isinstance(entry, dict):
                raise ValueError(f"e2e.{table_name}[{index}] must be a table")
            required = ("name", "expected_provider", "from_chain", "to_chain", "from_token", "to_token", "amount", "sender")
            missing = [field for field in required if not isinstance(entry.get(field), str) or not entry[field]]
            if missing:
                raise ValueError(f"e2e.{table_name}[{index}] missing: {', '.join(missing)}")
            env_required = entry.get("env_required", [])
            if not isinstance(env_required, list) or not all(isinstance(name, str) and name for name in env_required):
                raise ValueError(f"e2e.{table_name}[{index}].env_required must be an array of names")
            request = {key: entry[key] for key in ("from_chain", "to_chain", "from_token", "to_token", "amount", "sender")}
            if isinstance(entry.get("recipient"), str):
                request["recipient"] = entry["recipient"]
            request["live"] = bool(entry.get("live", False))
            probes.append(Probe(entry["name"], kind, entry["expected_provider"], request, tuple(env_required)))
    return probes


def uncovered_enabled_providers(config_path: str | Path, probes: list[Probe]) -> list[str]:
    """Return enabled runtime providers that have no declared live probe."""
    with Path(config_path).open("rb") as config_file:
        document = tomllib.load(config_file)
    providers = document.get("providers", [])
    if not isinstance(providers, list):
        raise ValueError("providers must be an array of tables")
    enabled: set[str] = set()
    for index, provider in enumerate(providers, start=1):
        if not isinstance(provider, dict):
            raise ValueError(f"providers[{index}] must be a table")
        if provider.get("enabled") is True:
            provider_id = provider.get("id")
            if not isinstance(provider_id, str) or not provider_id:
                raise ValueError(f"providers[{index}].id must be a non-empty string")
            enabled.add(provider_id)
    covered = {probe.expected_provider for probe in probes}
    return sorted(enabled - covered)


def coverage_result(config_path: str | Path, probes: list[Probe]) -> dict[str, Any]:
    uncovered = uncovered_enabled_providers(config_path, probes)
    if not uncovered:
        return report_entry("provider-coverage", "coverage", "PASS")
    return report_entry(
        "provider-coverage",
        "coverage",
        "FAIL",
        error=f"uncovered enabled providers: {', '.join(uncovered)}",
    )


def quote_cli_command(executable: str, config: str, probe: Probe, cargo_run: bool) -> list[str]:
    command = [executable, "--config", config, "--json", "quote"]
    for field in ("from_chain", "to_chain", "from_token", "to_token", "amount", "sender", "recipient"):
        if field in probe.request:
            command.extend((f"--{field.replace('_', '-')}", str(probe.request[field])))
    if probe.request["live"]:
        command.append("--live")
    return ["cargo", "run", "--quiet", "--"] + command[1:] if cargo_run else command


def prepare_cli_command(executable: str, config: str, probe: Probe, cargo_run: bool) -> list[str]:
    command = quote_cli_command(executable, config, probe, cargo_run)
    quote_index = command.index("quote")
    command[quote_index : quote_index + 1] = ["swap", "prepare"]
    command.extend(("--provider-id", probe.expected_provider))
    return command


def api_request(probe: Probe) -> tuple[str, dict[str, Any]]:
    if probe.kind == "quote":
        return "/v1/quotes", probe.request
    return "/v1/swaps/prepare", {"quote": probe.request, "provider_id": probe.expected_provider}


_HEX_PAYLOAD = re.compile(r"^0x[0-9a-fA-F]{64,}$")
_ADDRESS = re.compile(r"^0x[0-9a-fA-F]{40}$")
_SENSITIVE_KEY = re.compile(r"(?:authorization|api[_-]?key|secret|password|private[_-]?key|signature|calldata|serialized|typed[_-]?data|^data$|^input$)", re.I)
_HEADER_KEY = re.compile(r"(?:headers?|auth)", re.I)
_ADDRESS_KEY = re.compile(r"(?:address|sender|recipient|spender|deposit|\bto\b|\bfrom\b)", re.I)
_TRANSACTION_ID_KEY = re.compile(r"(?:transaction|tx)[_-]?(?:hash|id)|(?:status|exchange|order)[_-]?id", re.I)
_DIAGNOSTIC_SECRET = re.compile(r"(?i)(?:authorization|api[_-]?key|secret|password|private[_-]?key|signature|calldata)\s*[:=]\s*[^\s,;]+|\bbearer\s+[^\s,;]+")
_DIAGNOSTIC_HEX = re.compile(r"0x[0-9a-fA-F]{64,}")
_LOCAL_PATH = re.compile(r"(?:/Users|/home|/tmp|/var/folders)(?:/[^\s:]+)+")
_SENSITIVE_ENV_MARKER = re.compile(r"KEY|TOKEN|SECRET|AFFILIATE|RECIPIENT|PRIVATE|MNEMONIC|JWT", re.I)
_DIAGNOSTIC_LIMIT = 600


def sanitize(value: Any, key: str = "") -> Any:
    """Keep response shape and useful values while removing sensitive artifacts."""
    if value is None:
        return None
    if _SENSITIVE_KEY.search(key) or _HEADER_KEY.fullmatch(key) or _ADDRESS_KEY.search(key) or _TRANSACTION_ID_KEY.search(key):
        return "[REDACTED]"
    if isinstance(value, dict):
        return {str(child_key): sanitize(child_value, str(child_key)) for child_key, child_value in value.items()}
    if isinstance(value, list):
        return [sanitize(item, key) for item in value]
    if isinstance(value, str):
        if _ADDRESS.fullmatch(value) or _HEX_PAYLOAD.fullmatch(value):
            return "[REDACTED]"
        parsed = urlsplit(value)
        if parsed.scheme and parsed.netloc and (parsed.username is not None or parsed.password is not None):
            host = parsed.hostname or ""
            if parsed.port is not None:
                host = f"{host}:{parsed.port}"
            return urlunsplit((parsed.scheme, host, parsed.path, parsed.query, parsed.fragment))
    return value


def sanitize_diagnostic(value: object) -> str:
    """Return compact failure text without secrets, calldata, environment values, or local paths."""
    text = str(value)
    for name, secret in os.environ.items():
        if len(secret) >= 8 and _SENSITIVE_ENV_MARKER.search(name):
            text = text.replace(secret, f"<redacted:{name}>")
            text = text.replace(secret.lower(), f"<redacted:{name}>")
    text = _DIAGNOSTIC_SECRET.sub("[REDACTED]", text)
    text = _DIAGNOSTIC_HEX.sub("[REDACTED]", text)
    text = _LOCAL_PATH.sub("[PATH]", text)
    return " ".join(text.split())[:_DIAGNOSTIC_LIMIT]


def failure_diagnostic(prefix: str, stdout: object = "", stderr: object = "") -> str:
    """Attach the most useful subprocess/API failure output to a safe concise reason."""
    details = []
    if stderr:
        details.append(f"stderr: {sanitize_diagnostic(stderr)}")
    if stdout:
        details.append(f"stdout: {sanitize_diagnostic(stdout)}")
    detail = " ".join(details)
    return sanitize_diagnostic(f"{prefix}: {detail}") if detail else prefix


def report_entry(name: str, kind: str, status: str, response: Any = None, error: str | None = None) -> dict[str, Any]:
    entry: dict[str, Any] = {"name": name, "kind": kind, "status": status}
    if response is not None:
        entry["response"] = sanitize(response)
    if error is not None:
        entry["error"] = sanitize_diagnostic(error)
    return entry


def write_report(path: str | None, results: list[dict[str, Any]]) -> None:
    if path is None:
        return
    summary = {status: sum(item["status"] == status for item in results) for status in ("PASS", "FAIL", "SKIP")}
    Path(path).write_text(json.dumps({"summary": summary, "results": results}, indent=2) + "\n", encoding="utf-8")
