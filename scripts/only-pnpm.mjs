// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Refuse installs from any package manager other than pnpm.
//
// Dependency-free and offline on purpose: this runs as `preinstall`, before
// anything is fetched, so it cannot rely on a helper package. The lockfile in
// this repository is `pnpm-lock.yaml`; npm or yarn would write a second,
// divergent lockfile and resolve a different dependency graph than CI.
const agent = process.env.npm_config_user_agent ?? "";

if (!agent.startsWith("pnpm/")) {
  const named = agent.split("/")[0] || "this package manager";
  process.stderr.write(
    `\nThis repository uses pnpm. Installing with ${named} would write a second\n` +
      `lockfile and resolve a different dependency graph than CI.\n\n` +
      `  corepack enable && pnpm install\n\n`,
  );
  process.exit(1);
}
