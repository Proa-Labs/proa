// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export function GET() {
  return Response.json({ status: "ok", engine: "next" });
}
