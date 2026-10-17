# Copyright Proa Labs, Inc.
# SPDX-License-Identifier: Apache-2.0

"""Failure-path coverage for the release build and Railway cutover."""

import json
import subprocess
import unittest
from unittest.mock import MagicMock, patch

from marketing import release


class BuildPollingTests(unittest.TestCase):
    def setUp(self):
        self.sleep = patch.object(release.time, "sleep").start()
        self.cancel = patch.object(release.subprocess, "run").start()
        self.addCleanup(patch.stopall)
        self.initial = {"id": "build-id", "status": "WORKING"}

    def test_transient_poll_failure_recovers_without_cancelling(self):
        with patch.object(release, "gcloud", side_effect=[
            subprocess.CalledProcessError(1, "gcloud"),
            json.dumps({"id": "build-id", "status": "SUCCESS"}),
        ]):
            self.assertEqual(release.await_build(self.initial)["status"], "SUCCESS")
        self.cancel.assert_not_called()

    def test_repeated_poll_failure_cancels_the_submitted_build(self):
        with patch.object(release, "gcloud", side_effect=subprocess.TimeoutExpired("gcloud", 120)):
            with self.assertRaises(subprocess.TimeoutExpired):
                release.await_build(self.initial)
        self.assertIn("build-id", self.cancel.call_args.args[0])
        self.assertIn("cancel", self.cancel.call_args.args[0])
        self.assertTrue(self.cancel.call_args.kwargs["check"])

    def test_signal_exit_cancels_the_submitted_build(self):
        self.sleep.side_effect = SystemExit(143)
        with self.assertRaises(SystemExit):
            release.await_build(self.initial)
        self.cancel.assert_called_once()

    def test_terminal_failure_does_not_cancel_a_finished_build(self):
        self.assertEqual(release.await_build({"id": "build-id", "status": "FAILURE"})["status"], "FAILURE")
        self.cancel.assert_not_called()


class RailwayCutoverTests(unittest.TestCase):
    def setUp(self):
        self.image = "registry/image@sha256:" + "a" * 64
        self.variables = {"service": "service", "environment": "production"}
        self.previous = {"source": {"image": None, "repo": "Proa-Labs/proa"},
                         "railwayConfigFile": "railway.json", "healthcheckPath": "/old-health",
                         "healthcheckTimeout": 12, "restartPolicyType": "ALWAYS",
                         "restartPolicyMaxRetries": 4}
        self.current = {"source": {"image": self.image, "repo": None}, "railwayConfigFile": None,
                        "latestDeployment": {"id": "previous-deployment", "meta": {}}}
        self.accepted = self.current | {"latestDeployment": {
            "id": "new-deployment", "meta": {"imageDigest": "sha256:" + "a" * 64},
        }}
        self.sleep = patch.object(release.time, "sleep").start()
        self.addCleanup(patch.stopall)

    def config(self, credentials):
        return {"environment": {"config": {"services": {
            "service": {"deploy": {"registryCredentials": credentials}},
        }}}}

    def test_missing_registry_credentials_block_cutover_before_any_mutation(self):
        image = "us-west1-docker.pkg.dev/airy-cycle-504821-k9/releases/proa-marketing@sha256:" + "a" * 64
        for credentials in (None, {}, {"username": "reader"}):
            with self.subTest(credentials=credentials), patch.object(release, "api", return_value=self.config(credentials)) as api:
                with self.assertRaisesRegex(RuntimeError, "durable private-registry"):
                    release.deploy(image, "service", "production", "a" * 40, "https://example.com/readyz")
                api.assert_called_once()
                self.assertNotIn("mutation", api.call_args.args[0])

    def test_expiring_registry_credentials_are_rejected_when_visible(self):
        with patch.object(release, "api", return_value=self.config({
            "username": "oauth2accesstoken", "password": "*****",
        })):
            with self.assertRaisesRegex(RuntimeError, "expiring"):
                release.require_registry_credentials(self.variables)

    def test_masked_registry_credentials_do_not_need_to_be_read_or_replaced(self):
        with patch.object(release, "api", return_value=self.config({
            "username": "*****", "password": "*****",
        })) as api:
            release.require_registry_credentials(self.variables)
        api.assert_called_once()

    def test_cancellation_after_staging_restores_the_previous_source(self):
        with patch.object(release, "api", side_effect=[
            {"serviceInstance": self.previous}, True, SystemExit(143),
            {"serviceInstance": self.current}, True,
        ]) as api:
            with self.assertRaises(SystemExit):
                release.stage_and_deploy(self.image, self.variables)
        self.assertEqual(api.call_args.args[1]["input"], self.previous)
        for call in api.call_args_list:
            self.assertNotIn("registryCredentials", call.args[1].get("input", {}))

    def test_lost_staging_response_restores_settings_without_changing_credentials(self):
        previous = self.previous | {"source": {"image": "registry/old-image", "repo": None}}
        # A mistaken GitHub secret cannot overwrite the credentials needed to
        # pull the previous image, even when the staging response is lost.
        with patch.dict(release.os.environ, {"RAILWAY_REGISTRY_USERNAME": "wrong-user",
                                           "RAILWAY_REGISTRY_PASSWORD": "wrong-password"}):
            with patch.object(release, "api", side_effect=[
                {"serviceInstance": previous}, TimeoutError("staging response lost"),
                {"serviceInstance": self.current}, True,
            ]) as api:
                with self.assertRaises(TimeoutError):
                    release.stage_and_deploy(self.image, self.variables)
        self.assertEqual(api.call_args.args[1]["input"], previous)
        for call in api.call_args_list:
            self.assertNotIn("registryCredentials", call.args[1].get("input", {}))

    def test_lost_response_adopts_the_accepted_deployment_without_restoring_source(self):
        with patch.object(release, "api", side_effect=[
            {"serviceInstance": self.previous}, True, {"serviceInstance": self.current},
            TimeoutError("response lost"), {"serviceInstance": self.accepted},
        ]) as api:
            self.assertEqual(release.stage_and_deploy(self.image, self.variables), "new-deployment")
        self.assertEqual(sum(call.args[0] == release.UPDATE for call in api.call_args_list), 1)
        self.assertEqual(sum("serviceInstanceDeployV2" in call.args[0] for call in api.call_args_list), 1)

    def test_recovery_waits_for_deployment_visibility_and_digest(self):
        pending = self.accepted | {"latestDeployment": {"id": "new-deployment", "meta": None}}
        with patch.object(release, "api", side_effect=[
            OSError("unavailable"), {"serviceInstance": self.current},
            {"serviceInstance": pending}, {"serviceInstance": self.accepted},
        ]):
            self.assertEqual(release.recover_deployment(self.image, self.variables, "previous-deployment"), "new-deployment")
        self.assertEqual(self.sleep.call_count, 3)

    def test_uncertain_request_never_restores_or_retries_the_submission(self):
        for observed in (self.current, self.accepted | {"latestDeployment": {
                "id": "previous-deployment", "meta": self.accepted["latestDeployment"]["meta"],
        }}, self.accepted | {"latestDeployment": {
                "id": "unrelated-deployment", "meta": {"imageDigest": "sha256:" + "b" * 64},
        }}):
            with self.subTest(observed=observed):
                with patch.object(release, "api", side_effect=[
                    {"serviceInstance": self.previous}, True, {"serviceInstance": self.current},
                    TimeoutError("response lost"), *[{"serviceInstance": observed}] * 12,
                ]) as api:
                    with self.assertRaisesRegex(RuntimeError, "uncertain outcome; source was left untouched"):
                        release.stage_and_deploy(self.image, self.variables)
                self.assertEqual(sum("mutation" in call.args[0] for call in api.call_args_list), 2)

    def test_recovery_api_failure_leaves_source_untouched(self):
        with patch.object(release, "api", side_effect=OSError("API unavailable")) as api:
            with self.assertRaisesRegex(RuntimeError, "uncertain outcome"):
                release.recover_deployment(self.image, self.variables, "previous-deployment")
        self.assertEqual(api.call_count, 12)

    def test_signal_during_submission_leaves_source_untouched(self):
        with patch.object(release, "api", side_effect=[
            {"serviceInstance": self.previous}, True, {"serviceInstance": self.current}, SystemExit(143),
        ]) as api:
            with self.assertRaises(SystemExit):
                release.stage_and_deploy(self.image, self.variables)
        self.assertEqual(api.call_count, 4)

    def test_cleanup_does_not_replace_another_operators_source(self):
        other = {"source": {"image": "another-image", "repo": None}}
        with patch.object(release, "api", side_effect=[
            {"serviceInstance": self.previous}, True, SystemExit(143), {"serviceInstance": other},
        ]) as api:
            with self.assertRaises(SystemExit):
                release.stage_and_deploy(self.image, self.variables)
        self.assertEqual(api.call_count, 4)

    def test_recovery_does_not_adopt_another_operators_source(self):
        other = self.accepted | {"source": {"image": "another-image", "repo": None}}
        with patch.object(release, "api", return_value={"serviceInstance": other}) as api:
            with self.assertRaisesRegex(RuntimeError, "uncertain outcome"):
                release.recover_deployment(self.image, self.variables, "previous-deployment")
        api.assert_called_once()

    def test_requested_deployment_keeps_its_image_source(self):
        with patch.object(release, "api", side_effect=[
            {"serviceInstance": self.previous}, True, {"serviceInstance": self.current},
            {"serviceInstanceDeployV2": "deployment-id"},
        ]) as api:
            self.assertEqual(release.stage_and_deploy(self.image, self.variables), "deployment-id")
        self.assertEqual(api.call_count, 4)
        self.assertNotIn("registryCredentials", api.call_args_list[1].args[1]["input"])

    def test_lost_deploy_response_still_removes_triggers_and_verifies_the_release(self):
        image = "us-west1-docker.pkg.dev/airy-cycle-504821-k9/releases/proa-marketing@sha256:" + "a" * 64
        revision = "b" * 40
        current = self.current | {"source": {"image": image, "repo": None}}
        accepted = self.accepted | {"source": current["source"]}
        triggers = {"deploymentTriggers": {"edges": [{"node": {
            "id": "git-trigger", "environmentId": "production",
            "repository": "Proa-Labs/proa", "branch": "main",
        }}]}}
        with patch.object(release, "api", side_effect=[
            self.config({"username": "*****", "password": "*****"}), triggers,
            {"serviceInstance": self.previous}, True, {"serviceInstance": current},
            TimeoutError("response lost"), {"serviceInstance": accepted}, triggers, True,
            {"deployment": {"status": "SUCCESS", "meta": accepted["latestDeployment"]["meta"]}},
        ]) as api, patch.object(release, "verify_revision") as verify:
            result = release.deploy(image, "service", "production", revision, "https://example.com/readyz")
        self.assertEqual(result["deployment"], "new-deployment")
        self.assertEqual(api.call_args_list[8].args[1], {"id": "git-trigger"})
        self.assertIn("deploymentTriggerDelete", api.call_args_list[8].args[0])
        self.assertEqual(api.call_args_list[9].args[1], {"id": "new-deployment"})
        verify.assert_called_once_with("https://example.com/readyz", revision)

    def test_readiness_waits_for_the_expected_commit(self):
        revision = "b" * 40
        old, new = MagicMock(), MagicMock()
        old.status = new.status = 200
        old.headers = {"x-proa-revision": "a" * 40}
        new.headers = {"x-proa-revision": revision}
        old.__enter__.return_value = old
        new.__enter__.return_value = new
        with patch.object(release.urllib.request, "urlopen", side_effect=[old, new]) as request:
            with patch.object(release.time, "sleep"):
                release.verify_revision("https://example.com/readyz", revision)
        self.assertEqual(request.call_count, 2)

    def test_readiness_fails_if_commit_header_is_absent(self):
        response = MagicMock(status=200, headers={})
        response.__enter__.return_value = response
        with patch.object(release.urllib.request, "urlopen", return_value=response):
            with patch.object(release.time, "sleep"):
                with self.assertRaisesRegex(RuntimeError, "did not serve commit"):
                    release.verify_revision("https://example.com/readyz", "a" * 40)


if __name__ == "__main__":
    unittest.main()
