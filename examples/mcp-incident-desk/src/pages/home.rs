// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub async fn handler(State(s): State<AppState>, h: HeaderMap) -> Result<Response> {
    if auth::browser_principal(&s, &h).await.is_ok() {
        return Ok(auth::redirect("/issues"));
    }
    page(
        "Welcome",
        None,
        html_sync! {
            <div class="welcome">
                <p class="eyebrow">"INCIDENT RESPONSE, SHARED"</p>
                <h1>
                    "One investigation."
                    <br />
                    "Every interface."
                </h1>
                <p>
                    "Investigate and resolve production incidents with your team and your agents, using the same permissions and the same source of truth."
                </p>
                <a class="button" href="/auth/login">"Sign in to Incident Desk →"</a>
                <p class="muted">"Demo identity provider · Real encrypted sessions"</p>
            </div>
        },
    )
}
