// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{Ctx, WebRenderSync};
use proa_macros::html_sync;

#[test]
fn test_form_with_comprehensive_attributes() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <form
            action="/submit"
            method="post"
            enctype="multipart/form-data"
            autocomplete="on"
            novalidate
        >
            <fieldset>
                <legend>"User Information"</legend>
                <label for="name">"Name:"</label>
                <input
                    type="text"
                    id="name"
                    name="username"
                    placeholder="Enter your name"
                    required
                    minlength="3"
                    maxlength="50"
                    pattern="[A-Za-z ]+"
                    autocomplete="name"
                />
                <label for="email">"Email:"</label>
                <input
                    type="email"
                    id="email"
                    name="email"
                    placeholder="you@example.com"
                    required
                    autocomplete="email"
                />
                <label for="age">"Age:"</label>
                <input
                    type="number"
                    id="age"
                    name="age"
                    min="18"
                    max="120"
                    step="1"
                />
                <label for="bio">"Bio:"</label>
                <textarea
                    id="bio"
                    name="bio"
                    rows="4"
                    cols="50"
                    placeholder="Tell us about yourself"
                    maxlength="500"
                ></textarea>
                <label for="country">"Country:"</label>
                <select id="country" name="country" required>
                    <option value="">"Select a country"</option>
                    <option value="us">"United States"</option>
                    <option value="uk" selected>"United Kingdom"</option>
                    <option value="ca">"Canada"</option>
                </select>
                <label>
                    <input type="checkbox" name="newsletter" checked />
                    " Subscribe to newsletter"
                </label>
                <button type="submit" disabled>"Submit"</button>
            </fieldset>
        </form>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<form"));
    assert!(html.contains("action=\"/submit\""));
    assert!(html.contains("method=\"post\""));
    assert!(html.contains("enctype=\"multipart/form-data\""));
    assert!(html.contains("autocomplete=\"on\""));
    assert!(html.contains("novalidate"));
    assert!(html.contains("<fieldset>"));
    assert!(html.contains("<legend>"));
    assert!(html.contains("placeholder=\"Enter your name\""));
    assert!(html.contains("required"));
    assert!(html.contains("minlength=\"3\""));
    assert!(html.contains("maxlength=\"50\""));
    assert!(html.contains("pattern=\"[A-Za-z ]+\""));
    assert!(html.contains("min=\"18\""));
    assert!(html.contains("max=\"120\""));
    assert!(html.contains("rows=\"4\""));
    assert!(html.contains("cols=\"50\""));
    assert!(html.contains("checked"));
    assert!(html.contains("disabled"));
    assert!(html.contains("selected"));
}

#[test]
fn test_table_with_attributes() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <table>
            <caption>"Sales Report"</caption>
            <colgroup>
                <col />
                <col />
            </colgroup>
            <thead>
                <tr>
                    <th scope="col" colspan="2">"Q1 Sales"</th>
                </tr>
            </thead>
            <tbody>
                <tr>
                    <td headers="h1">"Product A"</td>
                    <td rowspan="2">"$1000"</td>
                </tr>
            </tbody>
            <tfoot>
                <tr>
                    <td>"Total"</td>
                    <td>"$2000"</td>
                </tr>
            </tfoot>
        </table>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<table>"));
    assert!(html.contains("<caption>"));
    assert!(html.contains("<colgroup>"));
    assert!(html.contains("<col"));
    assert!(html.contains("<thead>"));
    assert!(html.contains("<tbody>"));
    assert!(html.contains("<tfoot>"));
    assert!(html.contains("<th"));
    assert!(html.contains("scope=\"col\""));
    assert!(html.contains("colspan=\"2\""));
    assert!(html.contains("rowspan=\"2\""));
    assert!(html.contains("headers=\"h1\""));
}

#[test]
fn test_media_elements() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div>
            <img
                src="/image.jpg"
                alt="A beautiful image"
                width="800"
                height="600"
                loading="lazy"
                decoding="async"
                srcset="/image-800.jpg 800w, /image-1600.jpg 1600w"
                sizes="(max-width: 800px) 100vw, 800px"
            />
            <picture>
                <source srcset="/image.webp" />
                <img src="/image.jpg" alt="Fallback" />
            </picture>
            <video
                src="/video.mp4"
                poster="/poster.jpg"
                width="640"
                height="360"
                controls
                autoplay
                loop
                muted
                preload="metadata"
                playsinline
            ></video>
            <audio src="/audio.mp3" controls autoplay loop preload="auto"></audio>
            <iframe
                src="https://example.com"
                width="560"
                height="315"
                allowfullscreen
                sandbox="allow-scripts allow-same-origin"
                loading="lazy"
            ></iframe>
        </div>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<img"));
    assert!(html.contains("src=\"/image.jpg\""));
    assert!(html.contains("alt=\"A beautiful image\""));
    assert!(html.contains("width=\"800\""));
    assert!(html.contains("height=\"600\""));
    assert!(html.contains("loading=\"lazy\""));
    assert!(html.contains("decoding=\"async\""));
    assert!(html.contains("srcset="));
    assert!(html.contains("sizes="));
    assert!(html.contains("<picture>"));
    assert!(html.contains("<source"));
    assert!(html.contains("<video"));
    assert!(html.contains("poster=\"/poster.jpg\""));
    assert!(html.contains("controls"));
    assert!(html.contains("autoplay"));
    assert!(html.contains("loop"));
    assert!(html.contains("muted"));
    assert!(html.contains("preload="));
    assert!(html.contains("playsinline"));
    assert!(html.contains("<audio"));
    assert!(html.contains("<iframe"));
    assert!(html.contains("allowfullscreen"));
    assert!(html.contains("sandbox="));
}

#[test]
fn test_semantic_html() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <article>
            <header>
                <h1>"Article Title"</h1>
                <address>"By John Doe"</address>
            </header>
            <section>
                <h2>"Introduction"</h2>
                <p>"Some text"</p>
            </section>
            <aside>
                <p>"Related content"</p>
            </aside>
            <footer>
                <time datetime="PT1H">"One hour"</time>
            </footer>
        </article>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<article>"));
    assert!(html.contains("<header>"));
    assert!(html.contains("<address>"));
    assert!(html.contains("<section>"));
    assert!(html.contains("<aside>"));
    assert!(html.contains("<footer>"));
    assert!(html.contains("<time"));
    assert!(html.contains("datetime=\"PT1H\""));
}

#[test]
fn test_aria_attributes() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div
            role="dialog"
            aria-modal="true"
            aria-labelledby="dialog-title"
            aria-describedby="dialog-desc"
        >
            <h2 id="dialog-title">"Confirmation"</h2>
            <p id="dialog-desc">"Are you sure?"</p>
            <button aria-label="Close dialog" aria-pressed="false">"Close"</button>
            <progress value="50" max="100" aria-valuetext="50 percent"></progress>
        </div>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("role=\"dialog\""));
    assert!(html.contains("aria-modal=\"true\""));
    assert!(html.contains("aria-labelledby=\"dialog-title\""));
    assert!(html.contains("aria-describedby=\"dialog-desc\""));
    assert!(html.contains("aria-label=\"Close dialog\""));
    assert!(html.contains("aria-pressed=\"false\""));
    assert!(html.contains("value=\"50\""));
    assert!(html.contains("max=\"100\""));
    assert!(html.contains("aria-valuetext=\"50 percent\""));
}

#[test]
fn test_text_formatting() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <p>
            "This is "
            <strong>"bold"</strong>
            " and "
            <em>"italic"</em>
            " and "
            <code>"code"</code>
            " and "
            <mark>"highlighted"</mark>
            " and "
            <small>"small"</small>
            " and "
            <del>"deleted"</del>
            " and "
            <ins>"inserted"</ins>
            " and "
            <sub>"subscript"</sub>
            " and "
            <sup>"superscript"</sup>
            "."
        </p>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<strong>"));
    assert!(html.contains("<em>"));
    assert!(html.contains("<code>"));
    assert!(html.contains("<mark>"));
    assert!(html.contains("<small>"));
    assert!(html.contains("<del>"));
    assert!(html.contains("<ins>"));
    assert!(html.contains("<sub>"));
    assert!(html.contains("<sup>"));
}

#[test]
fn test_lists() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div>
            <ul>
                <li>"Item 1"</li>
                <li>"Item 2"</li>
            </ul>
            <ol>
                <li>"First"</li>
                <li>"Second"</li>
            </ol>
            <dl>
                <dt>"Term"</dt>
                <dd>"Definition"</dd>
            </dl>
        </div>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<ul>"));
    assert!(html.contains("<ol>"));
    assert!(html.contains("<dl>"));
    assert!(html.contains("<dt>"));
    assert!(html.contains("<dd>"));
    assert!(html.contains("<li>"));
}

#[test]
fn test_interactive_elements() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div>
            <details open>
                <summary>"Click to expand"</summary>
                <p>"Hidden content"</p>
            </details>
            <dialog open>
                <p>"This is a dialog"</p>
            </dialog>
        </div>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<details"));
    assert!(html.contains("<summary>"));
    assert!(html.contains("<dialog"));
    assert!(html.contains("open"));
}

#[test]
fn test_void_elements() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div>
            <br />
            <hr />
            <img src="/test.jpg" alt="Test" />
            <input type="text" />
            <meta charset="utf-8" />
            <link rel="stylesheet" href="/style.css" />
        </div>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("<br"));
    assert!(html.contains("<hr"));
    assert!(html.contains("<img"));
    assert!(html.contains("<input"));
    assert!(html.contains("<meta"));
    assert!(html.contains("<link"));
    // Void elements should not have closing tags
    assert!(!html.contains("</br>"));
    assert!(!html.contains("</hr>"));
    assert!(!html.contains("</img>"));
    assert!(!html.contains("</input>"));
}

// Tests for AttrValue trait integration - Option<T> and bool dynamic attributes
#[test]
fn test_option_attribute_some() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let id: Option<&str> = Some("my-button");
    html_sync! {
        <button id={id}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("id=\"my-button\""));
}

#[test]
fn test_option_attribute_none() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let id: Option<&str> = None;
    html_sync! {
        <button id={id}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    // When None, the attribute should not be rendered at all
    assert!(!html.contains("id="));
}

#[test]
fn test_bool_attribute_true() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let is_disabled = true;
    html_sync! {
        <button disabled={is_disabled}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    // Boolean attribute should render as just the name (no value)
    assert!(html.contains(" disabled"));
}

#[test]
fn test_bool_attribute_false() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let is_disabled = false;
    html_sync! {
        <button disabled={is_disabled}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    // When false, boolean attribute should not be rendered
    assert!(!html.contains("disabled"));
}

#[test]
fn test_binary_expr_dynamic_bool_attribute() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let selected = 2;
    let day = 2;
    html_sync! {
        <button disabled={selected == day}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains(" disabled"));
}

#[test]
fn test_class_list_attribute() {
    use proa_core::ClassList;

    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let classes = ClassList::<3>::new().add("btn").add("btn-primary");
    html_sync! {
        <button class={classes}>"Click me"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("class=\"btn btn-primary\""));
}

#[test]
fn test_mixed_dynamic_attributes() {
    use proa_core::ClassList;

    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let id: Option<&str> = Some("submit-btn");
    let title: Option<&str> = None;
    let is_disabled = true;
    let classes = ClassList::<2>::new().add("btn");

    html_sync! {
        <button
            id={id}
            title={title}
            disabled={is_disabled}
            class={classes}
        >
            "Submit"
        </button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    assert!(html.contains("id=\"submit-btn\""));
    assert!(!html.contains("title=")); // None should not render
    assert!(html.contains(" disabled")); // true should render as boolean attr
    assert!(html.contains("class=\"btn\""));
}

#[test]
fn test_data_attribute_with_none() {
    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    let onclick: Option<&str> = None;
    let onblur: Option<&str> = Some("handleBlur");

    html_sync! {
        <button data-onclick={onclick} data-onblur={onblur}>"Click"</button>
    }
    .render(&mut cx)
    .unwrap();

    let html = String::from_utf8(out).unwrap();
    // None should NOT be in the output at all
    assert!(!html.contains("data-onclick"));
    // Some should be rendered
    assert!(html.contains("data-onblur=\"handleBlur\""));
    // Verify the full output
    assert_eq!(html, "<button data-onblur=\"handleBlur\">Click</button>");
}
