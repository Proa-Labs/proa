// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn formatting_is_idempotent() {
    let sources = [
        r#"fn render() {
    html_sync! { <div class={"card"}><span>{self.product.name}</span></div> }
}"#,
        r#"fn render() {
    html_sync! { <main class="mx-auto max-w-4xl px-6 py-16"><p class="mt-4 text-lg leading-8 text-zinc-600">"A long line of copy that will need to break across lines when formatted."</p><br/></main> }
}"#,
        r#"fn render() {
    html! { <section><h1>{title}</h1>{ if cond { a } else { b } }</section> }
}"#,
        r#"fn render() {
    html_sync! {}
}"#,
    ];
    for source in sources {
        let once = format_file(Path::new("sample.rs"), source).unwrap().output;
        let twice = format_file(Path::new("sample.rs"), &once).unwrap().output;
        assert_eq!(once, twice, "formatting is not idempotent for {source}");
    }
}

#[test]
fn verify_rejects_formatted_output_that_no_longer_parses() {
    let original = r#"fn render() {
    html_sync! { <div>"ok"</div> }
}"#;
    let broken = r#"fn render() {
    html_sync! { <div>"ok"</div }
}"#;
    let diagnostics = verify_render_preserving(Path::new("sample.rs"), original, broken);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.code == "format/verify" && diag.severity == Severity::Error),
        "expected a format/verify error, got {diagnostics:?}"
    );
}

#[test]
fn preserves_multiline_raw_string_bytes_while_formatting_siblings() {
    let source = r###"fn render() {
    html_sync! {
        {StaticRaw(r##"<!DOCTYPE html>
<html lang="en">
<head>
"##)}
        < meta name = "description" content = "Proa" / >
    }
}"###;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
        formatted.contains(
            r###"{StaticRaw(r##"<!DOCTYPE html>
<html lang="en">
<head>
"##)}"###
        ),
        "{formatted}"
    );
    assert!(
        formatted.contains(r#"<meta name="description" content="Proa" />"#),
        "{formatted}"
    );
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
}

#[test]
fn write_mode_does_not_partially_update_when_any_file_has_errors() {
    let temp = tempfile::tempdir().unwrap();
    let good = temp.path().join("a_good.rs");
    let bad = temp.path().join("z_bad.rs");
    let good_source = r#"fn render() {
    html_sync! { < div > "ok" </ div > }
}
"#;
    let bad_source = r#"fn render() {
    html_sync! { <div>"broken"</span> }
}
"#;
    std::fs::write(&good, good_source).unwrap();
    std::fs::write(&bad, bad_source).unwrap();

    let summary = format_paths_with_options(&[temp.path().to_path_buf()], false, true).unwrap();

    assert!(
        summary
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        "{summary:#?}"
    );
    assert_eq!(std::fs::read_to_string(&good).unwrap(), good_source);
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), bad_source);
}

#[test]
fn write_mode_updates_files_after_all_inputs_validate() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("a.rs");
    let second = temp.path().join("b.rs");
    std::fs::write(
        &first,
        "fn render() { html_sync! { < div > \"one\" </ div > } }\n",
    )
    .unwrap();
    std::fs::write(
        &second,
        "fn render() { html_sync! { < span > \"two\" </ span > } }\n",
    )
    .unwrap();

    let summary = format_paths_with_options(&[temp.path().to_path_buf()], false, true).unwrap();

    assert!(
        !summary
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error),
        "{summary:#?}"
    );
    assert!(std::fs::read_to_string(&first)
        .unwrap()
        .contains("<div>\"one\"</div>"));
    assert!(std::fs::read_to_string(&second)
        .unwrap()
        .contains("<span>\"two\"</span>"));
}

#[test]
fn formats_nested_markup_and_preserves_expr_source() {
    let source = r#"fn render() {
    html_sync! { <div class={"card"}><span>{self.product.name}</span></div> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains(
            "html_sync! {\n        <div class={\"card\"}>\n            <span>{self.product.name}</span>\n        </div>\n    }"
        ));
    assert!(!formatted.contains("self . product . name"));
}

#[test]
fn fixes_rustfmt_spaced_elements_and_attributes() {
    let source = r#"fn render() {
    html_sync! {
        < html lang = "en" > < head > < title > "Large Page Benchmark" </ title > <
        meta name = "viewport" content = "width=device-width, initial-scale=1" /> </
        head > < body > < main class = "main-content" > { LargeSections } </ main > </
        body > </ html >
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("<html lang=\"en\">"));
    assert!(formatted
        .contains("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />"));
    assert!(formatted.contains("<main class=\"main-content\">{LargeSections}</main>"));
    assert!(!formatted.contains("< html"));
    assert!(!formatted.contains("lang ="));
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn formats_framework_control_attrs_as_parseable_source() {
    let source = r#"fn render() {
    html_sync! { <div _a11y_ok role="progressbar"></div> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("<div _a11y_ok role=\"progressbar\"></div>"));
    assert!(!formatted.contains("<div -a11y-ok"));
    let reparsed = format_file(Path::new("sample.rs"), &formatted).unwrap();
    assert!(reparsed.diagnostics.is_empty());
}

#[test]
fn formats_nested_macro_inside_expression_slot() {
    let source = r#"fn render() {
    html_sync! { <div>{if show { html_sync! { < span data - item > "x" </ span > } .render(cx) ?; }}</div> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("<span data-item>\"x\"</span>"));
    assert!(!formatted.contains("< span data - item"));
}

#[test]
fn preserves_rsjs_authoring_attr_names() {
    let source = r#"#[rsjs::rsjs(component, client)]
fn render() {
    html_sync! { <button data - item on - click={handler} bind - value={value}></button> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("data_item"));
    assert!(formatted.contains("on_click={handler}"));
    assert!(formatted.contains("bind_value={value}"));
    assert!(!formatted.contains("on-click"));
}

#[test]
fn preserves_multiline_component_literals_in_qualified_rsjs_macros() {
    let source = r#"#[rsjs(component, client)]
impl<B: WriteBuf> WebRenderSync<::proa_core::NoLoader, B> for Demo {
    fn render(self, cx: &mut WebContext<::proa_core::NoLoader, B>) -> Result<(), WriteError> {
        proa_macros::html_sync! {
            <section data-rsjs-component-pilot>
                {app_ui::ControlledCheckbox {
                    class: None::<&'static str>,
                    options: &PLAN_OPTIONS,
                    on_change,
                }}
            </section>
        }
        .render(cx)
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("<section data-rsjs-component-pilot>"));
    assert!(!formatted.contains("data_rsjs_component_pilot"));
    assert!(formatted.contains(
        r#"{app_ui::ControlledCheckbox {
                    class: None::<&'static str>,
                    options: &PLAN_OPTIONS,
                    on_change,
                }}"#
    ));
    assert!(!formatted.contains("None::< & 'static str >"));
    assert!(!formatted.contains("& PLAN_OPTIONS"));
    assert!(formatted.contains("\n        .render(cx)\n    }\n}"));

    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn uses_line_indentation_for_nested_macro_calls_and_keeps_handler_blocks() {
    let source = r#"#[rsjs(component, client)]
fn render() {
    wrapper(proa_macros::html_sync! {
        <button on_click={|_| {
            if enabled.get() {
                enabled.set(false);
            }
        }}>
            "Disable"
        </button>
    })
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains(
        r#"    wrapper(proa_macros::html_sync! {
        <button on_click={|_| {
            if enabled.get() {
                enabled.set(false);
            }
        }}>
            "Disable"
        </button>
    })"#
    ));
    assert!(!formatted.contains("| _|"));
    assert!(!formatted.contains("enabled.get() ;"));
}

#[test]
fn preserves_readable_multiline_attribute_layout() {
    let source = r#"fn render() {
    html_sync! {
        <button
            type="button"
            aria-label="Confirm"
            on_click={handler}
        >
            "Submit"
        </button>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn multiline_handler_attributes_are_idempotent() {
    let source = r#"#[rsjs(component, client)]
fn render() {
    html_sync! {
        <button type="button" on_click={|_| {
            if enabled.get() {
                enabled.set(false);
            }
        }}>
            "Disable"
        </button>
    }
}"#;

    let once = format_file(Path::new("sample.rs"), source).unwrap().output;
    let twice = format_file(Path::new("sample.rs"), &once).unwrap().output;
    assert_eq!(once, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &once).is_empty());
}

#[test]
fn preserves_legacy_names_only_inside_rsjs_items() {
    let source = r#"fn normal() {
    html_sync! { <button data - item on - click={handler}></button> }
}

#[rsjs::rsjs(component, client)]
fn reactive() {
    html_sync! { <button data - item on - click={handler}></button> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("<button data-item on-click={handler}></button>"));
    assert!(formatted.contains("<button data_item on_click={handler}></button>"));
}

#[test]
fn compacts_multiline_simple_expression_slots() {
    let source = r#"fn render() {
    html_sync! {
        <div data-value={self
            .value.as_str()}>
            {self
                .children}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("data-value={self.value.as_str()}"));
    assert!(formatted.contains("{self.children}"));
}

#[test]
fn keeps_struct_update_spacing_when_compacting() {
    let source = r#"fn render() {
    html_sync! {
        <button>
            {is_loading
                .then(|| Spinner { size: Some(SpinnerSize::Sm), ..Spinner::default() })}
        </button>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("size: Some(SpinnerSize::Sm), ..Spinner::default()"));
    assert!(!formatted.contains("),.."));
}

#[test]
fn compacts_multiline_unary_not_without_token_gap() {
    let source = r#"fn render() {
    html_sync! {
        <button disabled={(!
            enabled).then_some("true")}>
            "Save"
        </button>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("disabled={(!enabled).then_some(\"true\")}"));
    assert!(!formatted.contains("! enabled"));
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn preserves_multiline_expression_slots_with_embedded_macros() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {self
                .show_icon.then(|| html_sync! {
                    <div class={icon_classes}></div>
                })}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains(
        r#"{self
                .show_icon.then(|| html_sync! {"#
    ));
    assert!(formatted.contains(
        r#"html_sync! {
                    <div class={icon_classes}></div>
                }"#
    ));
    assert!(!formatted.contains("| |"));
    assert!(!formatted.contains("html_sync !"));
    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn compacts_embedded_macros_without_rewriting_text_literals() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Tabs {
                children: Some(html_sync! {
                    <p>
                        "Production build. Server render p99"
                    </p>
                }),
            }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("html_sync! {"));
    assert!(formatted.contains("\"Production build. Server render p99\""));
    assert!(!formatted.contains("Production build.Server"));
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn preserves_multiline_control_flow_expression_slots() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {for d in 1u8..=dim {
                let day = d;
                html_sync! { <span>{day}</span> }
            }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("{for d in 1u8..=dim {\n"));
    assert!(formatted.contains("let day = d;"));
    assert!(!formatted.contains("{for d in 1u8..= dim { let day"));
}

#[test]
fn tailwind_bang_inside_string_does_not_compact_component_literal() {
    let source = r#"fn render() {
    html_sync! {
        <section>
            {Tabs {
                class: Some("w-full !rounded-[14px] border"),
                children: Some(html_sync! {
                    <div class="grid gap-2">
                        <span>"First"</span>
                        <span>"Second"</span>
                    </div>
                }),
            }}
        </section>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains(
        r#"{Tabs {
                class: Some("w-full !rounded-[14px] border"),
                children: Some(html_sync! {"#
    ));
    assert!(!formatted.contains("Tabs { class:"));
    assert!(!formatted.contains("html_sync !"));
    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn preserves_showcase_control_flow_and_expression_spacing() {
    let source = r#"fn render() {
    html_sync! {
        <main>
            {for (index, listing) in data.listings.iter().enumerate() {
                let visible = !listing.hidden;
                html_sync! {
                    <article data-visible={visible}>
                        {data.listings[index].title}
                    </article>
                }.render(cx)?;
            }}
            {if !data.listings.is_empty() {
                html_sync! {
                    <button>{data.listings[0].price_per_night}</button>
                }.render(cx);
            }}
        </main>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("{for (index, listing) in data.listings.iter().enumerate() {\n"));
    assert!(formatted.contains("let visible = !listing.hidden;"));
    assert!(formatted.contains("{if !data.listings.is_empty() {\n"));
    assert!(formatted.contains("data.listings[index]"));
    assert!(formatted.contains("data.listings[0]"));
    assert!(!formatted.contains("{for("));
    assert!(!formatted.contains("listings ["));
    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn multiline_layout_sensitive_expr_no_longer_freezes_the_whole_macro_body() {
    // A single multi-line interpolation used to force the entire body to be
    // emitted byte-for-byte, so even the malformed `< main … >` spacing
    // survived and nothing in the file was ever re-anchored. The block is
    // now formatted, and the interpolation's interior is re-anchored under
    // its own opening instead of keeping columns from wherever it last sat.
    let source = r#"fn render() {
    html_sync! { < main class = "page" > {for listing in listings {
        html_sync! {
            <article>{listing.title}</article>
        }.render(cx)?;
    }} </ main > }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(formatted.contains("<main class=\"page\">"), "{formatted}");
    assert!(
        !formatted.contains("< main class = \"page\" >"),
        "{formatted}"
    );
    assert!(
        formatted.contains("            {for listing in listings {\n"),
        "{formatted}"
    );
    // Interior indented under its opening, not stranded at the old columns.
    assert!(
        formatted.contains("                html_sync! {\n"),
        "{formatted}"
    );
    assert!(formatted.contains("            }}\n"), "{formatted}");

    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice, "formatting must be idempotent");
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn stranded_interpolation_is_re_anchored_under_its_opening() {
    // The shape this whole change exists for: continuation lines left at an
    // absolute column from an earlier edit, hundreds of characters right of
    // the node they belong to.
    let stranded = format!(
        "fn render() {{\n    html_sync! {{\n        <li>\n            \
             {{Card {{ class: Some(\"x\"), children: Some(html_sync! {{\n{}<div \
             class=\"a\"></div>\n{}}}), }}}}\n        </li>\n    }}\n}}",
        " ".repeat(200),
        " ".repeat(196)
    );

    let formatted = format_file(Path::new("sample.rs"), &stranded)
        .unwrap()
        .output;

    assert!(!formatted.contains(&" ".repeat(60)), "{formatted}");
    assert!(
        formatted.contains("                <div class=\"a\"></div>\n"),
        "{formatted}"
    );
    assert!(verify_render_preserving(Path::new("sample.rs"), &stranded, &formatted).is_empty());
}

#[test]
fn nested_component_macros_are_formatted_recursively() {
    // Regression from the Proa UI account settings block. The outer Card
    // was expanded, but its `Some(html_sync! { ... })` field kept the old
    // absolute columns, leaving descendants more than 100 spaces deep.
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Card { class: Some("shadow-none"), children: Some(html_sync! {
                                                          {CardHeader { children: Some(html_sync! {
                                    {CardTitle { class: None, children: Some("Public profile") }}
                                    {CardDescription { class: None, children: Some("This information appears across your team workspace.") }}
                                }), ..Default::default() }}
                                                          {CardContent { class: Some("space-y-6"), children: Some(html_sync! {
                                                               <div class="flex flex-col gap-5 sm:flex-row sm:items-center">
                                                                   <p>"Profile"</p>
                                                               </div>
                                                           }), }}
                                                      }), }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(formatted.contains("            {Card {\n"), "{formatted}");
    assert!(formatted.contains("{CardHeader {\n"), "{formatted}");
    assert!(formatted.contains("{CardDescription {\n"), "{formatted}");
    let deepest_indent = formatted
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .max()
        .unwrap_or_default();
    assert!(
        deepest_indent <= 60,
        "nested component indentation drifted to column {deepest_indent}:\n{formatted}"
    );

    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice, "formatting must be idempotent");
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn overlong_inline_component_literal_expands_like_rustfmt() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Button { variant: Some(ButtonVariant::Ghost), class: Some("text-muted-foreground transition-colors hover:text-foreground"), children: Some("Remove"), ..Default::default() }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
        formatted.contains(
            r#"{Button {
                variant: Some(ButtonVariant::Ghost),
                class: Some("text-muted-foreground transition-colors hover:text-foreground"),
                children: Some("Remove"),
                ..Default::default()
            }}"#
        ),
        "{formatted}"
    );
    assert!(!formatted.contains("{Button { variant:"), "{formatted}");
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn project_explorer_button_expands_and_reindents_its_svg_children() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Button { size: Some(ButtonSize::Icon), color: None, variant: Some(ButtonVariant::Ghost), radius: None, disabled: false.into(), loading: false.into(), button_type: ButtonType::Button, class: None, id: None, children: Some(html_sync! {
                                                                                                                                                            <svg aria-hidden="true" viewBox="0 0 24 24" class="size-4" fill="none" stroke="currentColor" stroke-width="2">
                                                                                                                                                                <circle cx="12" cy="5" r="1" />
                                                                                                                                                                <circle cx="12" cy="12" r="1" />
                                                                                                                                                                <circle cx="12" cy="19" r="1" />
                                                                                                                                                            </svg>
                                                                                                                                                            <span class="sr-only">
                                                                                                                                                                "Project actions"
                                                                                                                                                            </span>
                                                                                                                                                        }), onclick: None, onblur: None, onfocus: None, }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
        formatted.contains(
            r#"{Button {
                size: Some(ButtonSize::Icon),
                color: None,
                variant: Some(ButtonVariant::Ghost),"#
        ),
        "{formatted}"
    );
    assert!(
        formatted.contains(
            r#"children: Some(html_sync! {
                    <svg"#
        ),
        "{formatted}"
    );
    assert!(formatted.contains("onclick: None,\n"), "{formatted}");
    assert!(!formatted.contains(&" ".repeat(80)), "{formatted}");

    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice, "formatting must be idempotent");
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn nested_struct_inside_some_obeys_the_width_limit() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Breadcrumb { class: None, children: Some(BreadcrumbList { class: None, children: Some("A very long breadcrumb child value that pushes the field beyond the formatter width"), }), }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
            formatted.contains(
                r#"children: Some(BreadcrumbList {
                    class: None,
                    children: Some("A very long breadcrumb child value that pushes the field beyond the formatter width"),
                }),"#
            ),
            "{formatted}"
        );
    assert!(
        !formatted.contains("Some(BreadcrumbList { class:"),
        "{formatted}"
    );
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn verify_allows_formatter_trailing_comma_before_compound_closer() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {EmailText { margin_bottom: Some(0), children: Some(CopyrightText { year, company_with_a_long_name_that_pushes_the_nested_literal_past_the_formatter_width }), ..Default::default() }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;

    assert!(
            formatted.contains(
                r#"children: Some(CopyrightText {
                    year,
                    company_with_a_long_name_that_pushes_the_nested_literal_past_the_formatter_width,
                }),"#
            ),
            "{formatted}"
        );
    assert!(
        verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty(),
        "the formatter's trailing comma must not change expression meaning"
    );
}

#[test]
fn well_anchored_interpolations_are_left_alone() {
    // Both hand-written shapes must be byte-identical after formatting:
    // a component literal closing level with its opening, and an expression
    // continuation sitting one level in.
    let source = r#"fn render() {
    html_sync! {
        <div>
            {Tabs {
                class: None,
            }}
            {self
                .show_icon.then(|| html_sync! {
                    <div class={icon_classes}></div>
                })}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source, "well-anchored blocks must not move");
}

#[test]
fn token_aware_macro_opening_repair_formats_only_the_macro() {
    let source = r#"fn render() {
    html_sync! {
        <div>
            {html_sync ! { < span > {item.name} </ span > }}
        </div>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert!(formatted.contains("html_sync! {"));
    assert!(formatted.contains("<span>{item.name}</span>"));
    assert!(!formatted.contains("html_sync !"));
    let twice = format_file(Path::new("sample.rs"), &formatted)
        .unwrap()
        .output;
    assert_eq!(formatted, twice);
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn comments_force_a_lossless_macro_body_fallback() {
    let source = r#"fn render() {
    html_sync! {
        <section>
            // Guest favorite badge
            <a href="https://proa.so/docs">"Docs"</a>
            /* Listings column */
            <div>{listing.title}</div>
        </section>
    }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_eq!(formatted, source);
    assert!(formatted.contains("// Guest favorite badge"));
    assert!(formatted.contains("/* Listings column */"));
    assert!(formatted.contains("https://proa.so/docs"));
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}

#[test]
fn comment_markers_inside_strings_do_not_disable_formatting() {
    let source = r#"fn render() {
    html_sync! { <a href="https://proa.so/*docs*/"><span>"Docs // now"</span></a> }
}"#;

    let formatted = format_file(Path::new("sample.rs"), source).unwrap().output;
    assert_ne!(formatted, source);
    assert!(formatted.contains("https://proa.so/*docs*/"));
    assert!(formatted.contains("\"Docs // now\""));
    assert!(verify_render_preserving(Path::new("sample.rs"), source, &formatted).is_empty());
}
