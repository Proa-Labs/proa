// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

import { hydrateRoot } from "react-dom/client";
import { EspnGamecastDocument, type EspnGamecastData } from "../react-ssr-shared/src/espn-gamecast";

const css = document.querySelector("#espn-gamecast-css")?.textContent ?? "";
const seed = document.querySelector("#espn-gamecast-props")?.textContent ?? "{}";
const data = JSON.parse(seed) as EspnGamecastData;
hydrateRoot(document, <EspnGamecastDocument
  css={css}
  data={data}
/>);
