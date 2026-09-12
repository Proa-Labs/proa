// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "..", "..");
const sharedSrc = resolve(__dirname, "..", "react-ssr-shared", "src");
const reactSsrSrc = resolve(__dirname, "..", "react-ssr");
const libSrc = resolve(__dirname, "lib");

/** @type {import('next').NextConfig} */
const nextConfig = {
  poweredByHeader: false,
  reactStrictMode: false,
  typescript: {
    ignoreBuildErrors: true,
  },
  turbopack: {
    root: repoRoot,
    resolveAlias: {
      "@shared": sharedSrc,
      "@react-ssr": reactSsrSrc,
      "@lib": libSrc,
    },
    resolveExtensions: [".tsx", ".ts", ".jsx", ".js", ".mjs", ".json"],
  },
  experimental: {
    externalDir: true,
  },
  webpack(config) {
    config.resolve.alias["@shared"] = sharedSrc;
    config.resolve.alias["@react-ssr"] = reactSsrSrc;
    config.resolve.alias["@lib"] = libSrc;
    return config;
  },
};

export default nextConfig;
