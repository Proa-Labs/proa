// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { loadDefaultProps } from "@lib/props";
import { nowUs, renderPageWithTimings } from "@lib/render";

export const dynamic = "force-dynamic";
export const runtime = "nodejs";

export async function POST(req: Request) {
  const e2eStart = nowUs();

  const rawBody = await req.text();
  const deserStart = nowUs();
  let payload: { page?: string; props?: unknown };
  try {
    payload = JSON.parse(rawBody);
  } catch {
    return Response.json({ error: "Invalid JSON" }, { status: 400 });
  }
  const deserUs = nowUs() - deserStart;

  if (!payload.page) {
    return Response.json({ error: "Missing page" }, { status: 400 });
  }

  try {
    const props = payload.props ?? loadDefaultProps()[payload.page] ?? {};
    const { html, renderUs, docUs } = await renderPageWithTimings(payload.page, props);
    const e2eUs = nowUs() - e2eStart;
    const htmlSize = Buffer.byteLength(html, "utf8");

    return new Response(html, {
      headers: {
        "content-type": "text/html; charset=utf-8",
        "x-render-us": renderUs.toFixed(3),
        "x-doc-us": docUs.toFixed(3),
        "x-deser-us": deserUs.toFixed(3),
        "x-e2e-us": e2eUs.toFixed(3),
        "x-html-size": String(htmlSize),
      },
    });
  } catch (e) {
    return Response.json({ error: (e as Error).message }, { status: 400 });
  }
}
