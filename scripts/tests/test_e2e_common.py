import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

from e2e_common import api_request, load_probes, prepare_cli_command, quote_cli_command, sanitize


class ProbeConfigTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.probes = load_probes(Path(__file__).parent / "fixtures" / "probes.toml")

    def test_parses_quote_and_prepare_probes(self):
        self.assertEqual([probe.kind for probe in self.probes], ["quote", "prepare"])
        self.assertEqual(self.probes[0].env_required, ("PROBE_FIXTURE_KEY",))
        self.assertTrue(self.probes[0].request["live"])

    def test_constructs_cli_and_api_requests(self):
        quote = quote_cli_command("/tmp/gulf", "fixture.toml", self.probes[0], False)
        self.assertEqual(quote[:5], ["/tmp/gulf", "--config", "fixture.toml", "--json", "quote"])
        self.assertIn("--live", quote)
        prepare = prepare_cli_command("gulf", "fixture.toml", self.probes[1], True)
        self.assertEqual(prepare[:3], ["cargo", "run", "--quiet"])
        self.assertIn("prepare", prepare)
        self.assertEqual(api_request(self.probes[1])[0], "/v1/swaps/prepare")

    def test_sanitizes_sensitive_response_values_without_losing_amounts(self):
        response = {
            "amount": "1000000",
            "authorization": "Bearer secret",
            "sender": "0x0000000000000000000000000000000000000001",
            "data": "0x" + "a" * 128,
            "transaction_hash": "0x" + "b" * 64,
            "approval_address": None,
            "calldata": None,
            "url": "https://user:password@example.test/path",
            "nested": {"fee": "12.5"},
        }
        sanitized = sanitize(response)
        self.assertEqual(sanitized["amount"], "1000000")
        self.assertEqual(sanitized["nested"]["fee"], "12.5")
        self.assertEqual(sanitized["authorization"], "[REDACTED]")
        self.assertEqual(sanitized["sender"], "[REDACTED]")
        self.assertEqual(sanitized["data"], "[REDACTED]")
        self.assertEqual(sanitized["transaction_hash"], "[REDACTED]")
        self.assertIsNone(sanitized["approval_address"])
        self.assertIsNone(sanitized["calldata"])
        self.assertEqual(sanitized["url"], "https://example.test/path")


if __name__ == "__main__":
    unittest.main()
