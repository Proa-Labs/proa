// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{render_md_to_string_async, render_to_string};
use proa_macros::{md, md_sync};

#[tokio::test]
async fn table_cell_code_boundaries_are_independent() {
    let value = "Ada";
    let sync = render_to_string(md_sync! { r###"
        | One | Two |
        | --- | --- |
        | `{not_rust` | {value} |
        | ` | {value} ` |
        | ` | {value} |
        | ` | other |
        "###
    });
    let asynchronous = render_md_to_string_async(md! { r###"
        | One | Two |
        | --- | --- |
        | `{not_rust` | {value} |
        | ` | {value} ` |
        | ` | {value} |
        | ` | other |
        "###
    })
    .await;
    let expected = "| One | Two |\n| --- | --- |\n| `{not_rust` | Ada |\n| ` | Ada ` |\n| ` | Ada |\n| ` | other |\n";
    assert_eq!(sync, expected);
    assert_eq!(asynchronous, expected);
}

#[tokio::test]
async fn commonmark_containers_and_paragraphs_control_interpolation() {
    let value = "Ada";
    let sync = render_to_string(md_sync! { r###"
        - Example

            ~~~text
            {{value}} {not_rust
            ~~~

        Before `

        {value}

        After `

            {{value}} {not_rust

        {value}
        "###
    });
    let asynchronous = render_md_to_string_async(md! { r###"
        - Example

            ~~~text
            {{value}} {not_rust
            ~~~

        Before `

        {value}

        After `

            {{value}} {not_rust

        {value}
        "###
    })
    .await;
    let expected = "- Example\n\n    ~~~text\n    {{value}} {not_rust\n    ~~~\n\nBefore `\n\nAda\n\nAfter `\n\n    {{value}} {not_rust\n\nAda\n";
    assert_eq!(sync, expected);
    assert_eq!(asynchronous, expected);
}

#[tokio::test]
async fn escapes_decode_once_in_sync_and_async_templates() {
    let name = "Ada";
    let input = "{{name}}";
    let sync = render_to_string(md_sync! { r###"
        {{name}} | {{{name}}} | \{name} | \{{name}}
        {{ name }} | { { name } }
        {input} | {"{{name}}"} | {format!("{{{name}}}")}
        {md_sync! { "{{{name}}}" }}
        `{{name}}` | `{name}`
        "###
    });
    let asynchronous = render_md_to_string_async(md! { r###"
        {{name}} | {{{name}}} | \{name} | \{{name}}
        {{ name }} | { { name } }
        {input} | {"{{name}}"} | {format!("{{{name}}}")}
        {md! { "{{{name}}}" }}
        `{{name}}` | `{name}`
        "###
    })
    .await;
    let expected = r"{name} | {Ada} | \Ada | \{name}
{ name } | Ada
{{name}} | {{name}} | {Ada}
{Ada}
`{{name}}` | `{name}`
";
    assert_eq!(sync, expected);
    assert_eq!(asynchronous, expected);
}

#[test]
fn cooked_and_raw_literals_share_brace_escaping() {
    let name = "Ada";
    assert_eq!(
        render_to_string(md_sync! { "{{{name}}} \\{name} \\{{name}}" }),
        render_to_string(md_sync! { r"{{{name}}} \{name} \{{name}}" })
    );
}

#[tokio::test]
async fn quoted_fences_are_opaque_in_sync_and_async_templates() {
    let name = "Ada";
    let sync = render_to_string(md_sync! { r###"
        > ~~~text
        > {{name}} {not_rust
        > ~~~
        {name}
        > > ```text
        > > {{name}} {not_rust
        > {name}
        "###
    });
    let asynchronous = render_md_to_string_async(md! { r###"
        > ~~~text
        > {{name}} {not_rust
        > ~~~
        {name}
        > > ```text
        > > {{name}} {not_rust
        > {name}
        "###
    })
    .await;
    let expected =
        "> ~~~text\n> {{name}} {not_rust\n> ~~~\nAda\n> > ```text\n> > {{name}} {not_rust\n> Ada\n";
    assert_eq!(sync, expected);
    assert_eq!(asynchronous, expected);
}
