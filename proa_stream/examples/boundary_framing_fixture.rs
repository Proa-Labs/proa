// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::io::{self, Write as _};

use proa_core::CspNonce;
use proa_stream::driver::out_of_order::format_boundary_chunk_with_nonce;
use proa_stream::write_inline_runtime_tag;

fn main() {
    let nonce = env::args()
        .nth(1)
        .expect("usage: boundary_framing_fixture <csp-nonce>");
    let nonce = CspNonce::new(nonce).expect("fixture nonce must be valid base64");

    let script_and_island = format!(
        r#"<section id="scripts-and-island"><script nonce="{nonce}">globalThis.__payloadScriptExecutions+=1;</script><rsjs-island data-rsjs-island-root="fixture_island"><button id="island-button">island ready</button></rsjs-island><script type="application/json" data-rsjs-island="fixture_island" nonce="{nonce}">{{"message":"streamed seed","emoji":"🧭"}}</script></section>"#,
        nonce = nonce.as_str(),
    );
    let cases: [(u64, &[u8]); 5] = [
        (
            1,
            r#"<section id="balanced-nested"><p id="unicode-copy">Grüße, 東京 🌊</p><template id="inner-template"><span>nested template content</span></template><p id="after-balanced-template">after balanced template</p></section>"#
                .as_bytes(),
        ),
        (
            2,
            concat!(
                r#"<article id="raw-cms"><p>raw before</p>"#,
                r#"</template><div id="raw-after-lower">after lowercase close</div>"#,
                r#"</TeMpLaTe ><div id="raw-after-mixed">after mixed-case close</div>"#,
                "</template\t><div id=\"raw-after-tab\">after tab close</div>",
                "</template\n><div id=\"raw-after-newline\">after newline close</div>",
                r#"</article>"#
            )
            .as_bytes(),
        ),
        (
            3,
            r#"<article id="sanitized-cms"><p>Sanitized &amp; stable</p><strong>Allowed markup</strong></article>"#
                .as_bytes(),
        ),
        (4, script_and_island.as_bytes()),
        (
            5,
            concat!(
                r#"<section id="adversarial-close"><p>before injection</p>"#,
                r#"</template><div id="escape-attempt">must stay in boundary five</div>"#,
                r#"</TEMPLATE   ><p id="after-escape-attempt">after injection</p>"#,
                r#"</section>"#
            )
            .as_bytes(),
        ),
    ];

    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut runtime = Vec::new();
    write_inline_runtime_tag(&mut runtime, Some(&nonce)).expect("write runtime");
    writeln!(
        stdout,
        "R\t{}",
        String::from_utf8(runtime).expect("runtime is UTF-8")
    )
    .expect("write runtime record");

    for (id, html) in cases {
        let chunk = format_boundary_chunk_with_nonce(id, html, Some(&nonce));
        writeln!(
            stdout,
            "C\t{id}\t{}",
            String::from_utf8(chunk).expect("framed chunk is UTF-8")
        )
        .expect("write chunk record");
    }
}
