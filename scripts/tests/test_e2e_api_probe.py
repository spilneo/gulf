import argparse
import io
import sys
import unittest
from email.message import Message
from pathlib import Path
from unittest.mock import patch
from urllib.error import HTTPError

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import e2e_api_probe
from e2e_common import Probe


class ApiProbeTests(unittest.TestCase):
    def test_live_quote_with_endpoint_failure_fails(self):
        args = argparse.Namespace(base_url="http://localhost", timeout=1)
        probe = Probe("live", "quote", "provider-a", {"live": True}, ())
        response = {"quotes": [{"provider_id": "provider-a", "live_response": None, "normalized": {"error": "HTTP 503"}}]}
        with patch("e2e_api_probe.request_json", return_value=response):
            result = e2e_api_probe.run_probe(args, probe)
        self.assertEqual(result["status"], "FAIL")
        self.assertIn("no live response", str(result["error"]))

    def test_prepare_still_requires_matching_provider(self):
        args = argparse.Namespace(base_url="http://localhost", timeout=1)
        probe = Probe("prepare", "prepare", "provider-a", {"live": True}, ())
        with patch("e2e_api_probe.request_json", return_value={"swap": {"provider_id": "other"}}):
            result = e2e_api_probe.run_probe(args, probe)
        self.assertEqual(result["status"], "FAIL")

    def test_api_failure_report_includes_sanitized_response_body(self):
        args = argparse.Namespace(base_url="http://localhost", timeout=1)
        probe = Probe("live", "quote", "provider-a", {"live": True}, ())
        error = HTTPError("http://localhost", 502, "bad gateway", Message(), io.BytesIO(b"provider unavailable"))
        with patch("e2e_api_probe.request_json", side_effect=error):
            result = e2e_api_probe.run_probe(args, probe)
        self.assertEqual(result["status"], "FAIL")
        self.assertIn("provider unavailable", str(result["error"]))


if __name__ == "__main__":
    unittest.main()
