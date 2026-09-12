// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Differential-expansion gate for the consolidation core.
//!
//! The runtime-template compiler refactor consolidates the static walk and
//! relocates its semantic core out of this proc-macro crate. These tests
//! pin the exact `TokenStream` the emitters produce for a representative
//! template corpus, so any relocation or refactor that changes macro
//! expansion, even by a token, fails here.
//!
//! Bless new goldens with `PROA_UPDATE_GOLDEN=1 cargo test -p proa_macros
//! --lib` after an intentional emitter change; never bless to paper over
//! an unintended diff.
//!
//! Everything runs inside ONE `#[test]` in a fixed order: the raw-text
//! emitter allocates process-global guard identifiers, so case order must
//! be deterministic for the goldens to be stable.

use super::{
    generate_optimized, generate_optimized_async, generate_optimized_async_xml,
    generate_optimized_xml, validate_security,
};
use crate::parse::HtmlRoot;
use std::fmt::Write as _;
use std::path::PathBuf;

struct Case {
    name: &'static str,
    template: &'static str,
    /// XML templates run the XML generators instead of the HTML ones.
    xml: bool,
}

const CASES: &[Case] = &[
    Case {
        name: "static_only",
        template: r#"
            <header class="hero">
                <h1>"Proa Pages"</h1>
                <p class="tagline">"Templates as <data> & bytes"</p>
            </header>
        "#,
        xml: false,
    },
    Case {
        name: "doctype_page",
        template: r#"
            <!DOCTYPE html>
            <html lang="en">
                <body>
                    <main>"Hello"</main>
                </body>
            </html>
        "#,
        xml: false,
    },
    Case {
        name: "dynamic_text_and_attrs",
        template: r#"
            <article class="profile" data-theme={theme} hidden={muted}>
                <img class="avatar" src={avatar_url} alt={name}>
                <h2>{name}</h2>
            </article>
        "#,
        xml: false,
    },
    Case {
        name: "control_flow",
        template: r#"
            <ul class="items">
                {for (i, item) in items.iter().enumerate() {
                    html_sync! { <li>{item.as_str()} " #" {i}</li> }.render(cx)?;
                }}
                {if let Some(note) = note {
                    html_sync! { <li class="note">{note}</li> }.render(cx)?;
                } else {
                    html_sync! { <li class="note-empty">"none"</li> }.render(cx)?;
                }}
            </ul>
        "#,
        xml: false,
    },
    Case {
        name: "raw_text_script_style",
        template: r#"
            <div class="analytics">
                <style>".accent { color: " {accent} "; }"</style>
                <script>"window.__x = \"" {payload} "\";"</script>
            </div>
        "#,
        xml: false,
    },
    Case {
        name: "noopener_merge",
        template: r#"
            <a href="https://example.com" target="_blank" rel="me">"Docs"</a>
        "#,
        xml: false,
    },
    Case {
        name: "a11y_ok_strip",
        template: r#"
            <div _a11y_ok role="button" tabindex="0" onclick={handler}>"Press"</div>
        "#,
        xml: false,
    },
    Case {
        name: "fragment_mixed",
        template: r#"
            <>
                <span>"a"</span>
                {value}
                <span>"b"</span>
            </>
        "#,
        xml: false,
    },
    Case {
        name: "xml_feed",
        template: r#"
            <feed xmlns="http://www.w3.org/2005/Atom">
                <title>{title}</title>
                <entry_count>"3"</entry_count>
            </feed>
        "#,
        xml: true,
    },
];

/// Template whose `validate_security` result must stay stable: a hard
/// error (dangerous URL scheme), which expands to deterministic
/// `compile_error!` tokens.
const ERROR_CASE: (&str, &str) = (
    "validate_dangerous_url_error",
    r#"<a href="javascript:alert(1)">"x"</a>"#,
);

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("golden")
        .join("consolidation")
}

fn check(name: &str, actual: &str) -> Result<(), String> {
    let path = golden_dir().join(format!("{name}.golden.txt"));
    if std::env::var_os("PROA_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden_dir()).expect("create golden dir");
        std::fs::write(&path, actual).expect("write golden");
        return Ok(());
    }
    let expected = std::fs::read_to_string(&path)
        .map_err(|err| format!("missing golden {}: {err}", path.display()))?;
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "expansion for `{name}` diverged from {}.\n\
             Re-bless with PROA_UPDATE_GOLDEN=1 only if the change is intentional.\n\
             --- expected ---\n{expected}\n--- actual ---\n{actual}",
            path.display()
        ))
    }
}

#[test]
fn expansion_matches_goldens() {
    let mut failures = Vec::new();
    for case in CASES {
        let root: HtmlRoot = syn::parse_str(case.template)
            .unwrap_or_else(|err| panic!("case `{}` failed to parse: {err}", case.name));
        let mut out = String::new();
        if case.xml {
            writeln!(out, "--- sync-xml ---").unwrap();
            writeln!(out, "{}", generate_optimized_xml(&root)).unwrap();
            writeln!(out, "--- async-xml ---").unwrap();
            writeln!(out, "{}", generate_optimized_async_xml(&root)).unwrap();
        } else {
            writeln!(out, "--- sync ---").unwrap();
            writeln!(out, "{}", generate_optimized(&root)).unwrap();
            writeln!(out, "--- async ---").unwrap();
            writeln!(out, "{}", generate_optimized_async(&root)).unwrap();
        }
        if let Err(failure) = check(case.name, &out) {
            failures.push(failure);
        }
    }

    // Hard-error output from the validation pass.
    let (name, template) = ERROR_CASE;
    let root: HtmlRoot = syn::parse_str(template).expect("error case parses");
    let rendered = match validate_security(&root) {
        Err(errors) => format!("--- errors ---\n{errors}\n"),
        Ok(_) => panic!("dangerous URL template unexpectedly validated"),
    };
    if let Err(failure) = check(name, &rendered) {
        failures.push(failure);
    }

    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
