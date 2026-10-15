# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Failure-path coverage for the release build and Railway cutover."""

import json
import subprocess
import unittest
from unittest.mock import MagicMock, patch

from marketing import deploy, submit


class BuildPollingTests(unittest.TestCase):
    def setUp(self):
        self.sleep = patch.object(submit.time, "sleep").start()
        self.cancel = patch.object(submit.subprocess, "run").start()
        self.addCleanup(patch.stopall)
        self.initial = {"id": "build-id", "status": "WORKING"}

    def test_transient_poll_failure_recovers_without_cancelling(self):
        with patch.object(submit, "gcloud", side_effect=[
            subprocess.CalledProcessError(1, "gcloud"),
            json.dumps({"id": "build-id", "status": "SUCCESS"}),
        ]):
            self.assertEqual(submit.await_build(self.initial)["status"], "SUCCESS")
        self.cancel.assert_not_called()

    def test_repeated_poll_failure_cancels_the_submitted_build(self):
        with patch.object(submit, "gcloud", side_effect=subprocess.TimeoutExpired("gcloud", 120)):
            with self.assertRaises(subprocess.TimeoutExpired):
                submit.await_build(self.initial)
        self.assertIn("build-id", self.cancel.call_args.args[0])
        self.assertIn("cancel", self.cancel.call_args.args[0])
        self.assertTrue(self.cancel.call_args.kwargs["check"])

    def test_signal_exit_cancels_the_submitted_build(self):
        self.sleep.side_effect = SystemExit(143)
        with self.assertRaises(SystemExit):
            submit.await_build(self.initial)
        self.cancel.assert_called_once()

    def test_terminal_failure_does_not_cancel_a_finished_build(self):
        self.assertEqual(submit.await_build({"id": "build-id", "status": "FAILURE"})["status"], "FAILURE")
        self.cancel.assert_not_called()


class RailwayCutoverTests(unittest.TestCase):
    def setUp(self):
        self.image = "registry/image@sha256:" + "a" * 64
        self.variables = {"service": "service", "environment": "production"}
        self.previous = {"source": {"image": None, "repo": "Proa-Labs/proa"},
                         "railwayConfigFile": "railway.json"}
        self.current = {"source": {"image": self.image, "repo": None}, "railwayConfigFile": None}

    def test_expiring_registry_credentials_block_cutover_before_any_api_call(self):
        image = "us-west1-docker.pkg.dev/airy-cycle-504821-k9/releases/proa-marketing@sha256:" + "a" * 64
        with patch.dict(deploy.os.environ, {"RAILWAY_REGISTRY_USERNAME": "oauth2accesstoken",
                                           "RAILWAY_REGISTRY_PASSWORD": "expired-token"}):
            with patch.object(deploy, "api") as api:
                with self.assertRaisesRegex(RuntimeError, "durable private-registry"):
                    deploy.deploy(image, "service", "production", "a" * 40, "https://example.com/readyz")
        api.assert_not_called()

    def test_cancellation_after_staging_restores_the_previous_source(self):
        with patch.object(deploy, "api", side_effect=[
            {"serviceInstance": self.previous}, True, SystemExit(143),
            {"serviceInstance": self.current}, True,
        ]) as api:
            with self.assertRaises(SystemExit):
                deploy.stage_and_deploy(self.image, self.variables, {"username": "reader", "password": "credential"})
        self.assertEqual(api.call_args.args[1]["input"], self.previous)

    def test_failed_deploy_request_restores_staged_source(self):
        with patch.object(deploy, "api", side_effect=[
            {"serviceInstance": self.previous}, True, {"serviceInstance": self.current},
            OSError("API unavailable"), {"serviceInstance": self.current}, True,
        ]) as api:
            with self.assertRaises(OSError):
                deploy.stage_and_deploy(self.image, self.variables, {"username": "reader", "password": "credential"})
        self.assertEqual(api.call_args.args[1]["input"], self.previous)

    def test_cleanup_does_not_replace_another_operators_source(self):
        other = {"source": {"image": "another-image", "repo": None}}
        with patch.object(deploy, "api", side_effect=[
            {"serviceInstance": self.previous}, True, SystemExit(143), {"serviceInstance": other},
        ]) as api:
            with self.assertRaises(SystemExit):
                deploy.stage_and_deploy(self.image, self.variables, {"username": "reader", "password": "credential"})
        self.assertEqual(api.call_count, 4)

    def test_requested_deployment_keeps_its_image_source(self):
        with patch.object(deploy, "api", side_effect=[
            {"serviceInstance": self.previous}, True, {"serviceInstance": self.current},
            {"serviceInstanceDeployV2": "deployment-id"},
        ]) as api:
            self.assertEqual(deploy.stage_and_deploy(self.image, self.variables, {"username": "reader", "password": "credential"}), "deployment-id")
        self.assertEqual(api.call_count, 4)

    def test_readiness_waits_for_the_expected_commit(self):
        revision = "b" * 40
        old, new = MagicMock(), MagicMock()
        old.status = new.status = 200
        old.headers = {"x-proa-revision": "a" * 40}
        new.headers = {"x-proa-revision": revision}
        old.__enter__.return_value = old
        new.__enter__.return_value = new
        with patch.object(deploy.urllib.request, "urlopen", side_effect=[old, new]) as request:
            with patch.object(deploy.time, "sleep"):
                deploy.verify_revision("https://example.com/readyz", revision)
        self.assertEqual(request.call_count, 2)

    def test_readiness_fails_if_commit_header_is_absent(self):
        response = MagicMock(status=200, headers={})
        response.__enter__.return_value = response
        with patch.object(deploy.urllib.request, "urlopen", return_value=response):
            with patch.object(deploy.time, "sleep"):
                with self.assertRaisesRegex(RuntimeError, "did not serve commit"):
                    deploy.verify_revision("https://example.com/readyz", "a" * 40)


if __name__ == "__main__":
    unittest.main()
