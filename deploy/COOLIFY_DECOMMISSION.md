# Retired deployment infrastructure

All Proa applications moved from Coolify to Railway on 22 October 2026.
Railway PR environments replaced the VPS preview archive. The retired gateway,
archive services, provisioning scripts, and their tests have been removed from
this repository; their previous configuration remains available in Git history.

Current operations are documented in the [marketing deployment guide](../proa_marketing/README.md#deployment)
and the [release-image runbook](gcp-runners/README.md#marketing-release-images).
The CLI's optional Coolify preset remains available for users deploying to their
own Coolify instances.

Repository cleanup does not verify deletion of former external resources.
Any remaining Coolify containers, preview services, GitHub Apps, Cloudflare
Worker routes, DNS records, and obsolete credentials can be retired separately.
Preserve required backups and unrelated services on the former VPS.
