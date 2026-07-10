import argparse
import os
import subprocess
import sys
import unittest
from pathlib import Path
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import e2e_probe
from e2e_common import Probe, coverage_result, sanitize_diagnostic


class CliProbeTests(unittest.TestCase):
    def setUp(self):
        self.args = argparse.Namespace(gulf="gulf", config="fixture.toml", cargo_run=False, timeout=1)
        self.probe = Probe("live", "quote", "provider-a", {"live": True}, ())

    def run_with_output(self, output, returncode=0, stderr=""):
        completed = subprocess.CompletedProcess(["gulf"], returncode, output, stderr)
        with patch("e2e_probe.subprocess.run", return_value=completed):
            return e2e_probe.run_probe(self.args, self.probe)

    def test_live_quote_requires_live_response_normalized_quote_and_no_error(self):
        base = {"provider_id": "provider-a", "live_response": {"quote": "ok"}, "normalized": {"error": None}}
        self.assertEqual(self.run_with_output(__import__("json").dumps([base]))["status"], "PASS")
        for field, value in (("live_response", None), ("normalized", None)):
            quote = dict(base)
            quote[field] = value
            result = self.run_with_output(__import__("json").dumps([quote]))
            self.assertEqual(result["status"], "FAIL")
        quote = dict(base)
        quote["normalized"] = {"error": "provider unavailable"}
        result = self.run_with_output(__import__("json").dumps([quote]))
        self.assertEqual(result["status"], "FAIL")
        self.assertIn("provider unavailable", str(result["error"]))

    def test_failure_output_is_reported_without_secrets_or_paths(self):
        secret = "probe_test_secret_value"
        with patch.dict(os.environ, {"PROBE_API_KEY": secret}):
            result = self.run_with_output("", 1, f"Authorization: Bearer {secret} /Users/test/app.py 0x{'a' * 64}")
        self.assertEqual(result["status"], "FAIL")
        self.assertNotIn(secret, str(result["error"]))
        self.assertNotIn("/Users/test/app.py", str(result["error"]))
        self.assertNotIn("0x" + "a" * 64, str(result["error"]))

    def test_failure_report_includes_stdout_when_stderr_is_present(self):
        result = self.run_with_output("provider response detail", 1, "provider stderr detail")
        self.assertIn("stderr: provider stderr detail", str(result["error"]))
        self.assertIn("stdout: provider response detail", str(result["error"]))

    def test_diagnostic_is_capped(self):
        self.assertLessEqual(len(sanitize_diagnostic("word " * 1000)), 600)

    def test_coverage_reports_enabled_provider_without_probe(self):
        fixture = Path(__file__).with_name("fixtures") / "runtime-providers.toml"
        result = coverage_result(fixture, [Probe("other", "quote", "other-provider", {"live": True}, ())])
        self.assertEqual(result["status"], "FAIL")
        self.assertIn("fixture-provider", str(result["error"]))

    def test_coverage_passes_when_all_enabled_providers_are_probed(self):
        fixture = Path(__file__).with_name("fixtures") / "runtime-providers.toml"
        result = coverage_result(fixture, [Probe("covered", "quote", "fixture-provider", {"live": True}, ())])
        self.assertEqual(result["status"], "PASS")


if __name__ == "__main__":
    unittest.main()
