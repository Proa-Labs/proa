use proa_core::MdRenderSync;
use proa_macros::md_sync;

pub struct Activity {
    pub user_name: String,
    pub note: String,
}

pub fn report(a: &Activity, rows: &[Activity], person: Option<&Activity>) -> String {
    let client = "editor";
    let mut bytes = Vec::new();
    md_sync! { r###"
        ### Activity

        **{a.user_name.as_str()}** · via {client}

        {a.note.as_str()}

        {for row in rows {
            md_sync! { r#"
                - {row.user_name.as_str()}: {row.note.as_str()}
                Read [docs](https://example.com) — don't forget {row.user_name.as_str()}
                {if row.note.is_empty() {
                    md_sync! { "No note.\n" }
                } else {
                    md_sync! { "Note: {row.note.as_str()}\n" }
                }}
            "# }
        }}

        {if let Some(person) = person {
            md_sync! { "Author: {person.user_name.as_str()}\n" }
        } else {
            md_sync! { "Anonymous.\n" }
        }}

        ```rust
        fn example() { literal_braces }
        ```
        "###
    }
    .render_md(&mut proa_core::WebContext::with_buffer(&mut bytes))
    .unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn renders_raw_templates_on_edition_2024() {
    let a = Activity {
        user_name: "**name**".into(),
        note: "<script>".into(),
    };
    let result = report(&a, &[], Some(&a));
    assert!(result.starts_with("### Activity\n"));
    assert!(result.contains("\\*\\*name\\*\\*"));
    assert!(result.contains("fn example() { literal_braces }"));
}
