// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_core::{DataLoader, WebContext, WebRenderSync, WriteBuf, WriteError};
use proa_macros::html_sync;
use rsjs::signal;
use rsjs_macros::rsjs;

use crate::forms::{pizza_order, CreatePizzaOrder, PIZZA_ORDER_WEBMCP_PARAMETERS};
use crate::webmcp::{WebMcp, WebMcpComposeError, WebMcpContext};

const PAGE_SHELL: &str = "page-shell";
const FORM_PANEL: &str = "form-panel";
const PREVIEW_PANEL: &str = "preview-panel";

#[rsjs::client]
fn total_dollars(size: String, quantity: String) -> i64 {
    if size == "large" {
        if quantity == "6" {
            108_i64
        } else if quantity == "5" {
            90_i64
        } else if quantity == "4" {
            72_i64
        } else if quantity == "3" {
            54_i64
        } else if quantity == "2" {
            36_i64
        } else {
            18_i64
        }
    } else if size == "medium" {
        if quantity == "6" {
            90_i64
        } else if quantity == "5" {
            75_i64
        } else if quantity == "4" {
            60_i64
        } else if quantity == "3" {
            45_i64
        } else if quantity == "2" {
            30_i64
        } else {
            15_i64
        }
    } else {
        if quantity == "6" {
            72_i64
        } else if quantity == "5" {
            60_i64
        } else if quantity == "4" {
            48_i64
        } else if quantity == "3" {
            36_i64
        } else if quantity == "2" {
            24_i64
        } else {
            12_i64
        }
    }
}

#[rsjs::client]
fn customer_name_is_long_enough(value: String) -> bool {
    value.len() >= 2
}

pub struct PizzaPage {
    pub form: PizzaForm,
}

impl PizzaPage {
    pub fn initial() -> Self {
        Self {
            form: PizzaForm::initial(),
        }
    }

    pub fn from_state(state: &pizza_order::State) -> Self {
        Self {
            form: PizzaForm::from_state(state),
        }
    }
}

impl WebMcp for PizzaPage {
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        // Page-level composition is set union: expose every complete tool
        // contributed by the page's child components.
        self.form.describe_webmcp(cx)
    }
}

impl<B, L> WebRenderSync<L, B> for PizzaPage
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <main data-component="pizza-page" class={PAGE_SHELL}>
                <header class="hero">
                    <p class="eyebrow">"PROA × WELLFORMED × RSJS × WEBMCP"</p>
                    <h1>"One operation. Two participants."</h1>
                    <p class="lede">
                        "A person and an agent can configure the same native form. RSJS keeps the preview live; Wellformed validates the request on the server."
                    </p>
                </header>
                <section class="demo-grid">
                    {self.form}
                    <aside class="explanation-panel">
                        <p class="panel-kicker">"Execution path"</p>
                        <ol class="execution-list">
                            <li>
                                <span>"1"</span>
                                "WebMCP discovers the native form."
                            </li>
                            <li>
                                <span>"2"</span>
                                "The browser fills its visible controls."
                            </li>
                            <li>
                                <span>"3"</span>
                                "The bridge reconciles those values into RSJS."
                            </li>
                            <li>
                                <span>"4"</span>
                                "The user reviews the live preview and submits."
                            </li>
                            <li>
                                <span>"5"</span>
                                "Wellformed validates and normalizes on the server."
                            </li>
                        </ol>
                        <p class="footnote">
                            "There is deliberately no "
                            <code>"toolautosubmit"</code>
                            ". The operation ends with human confirmation."
                        </p>
                    </aside>
                </section>
            </main>
        }
        .render(cx)
    }
}

pub struct PizzaForm {
    customer_name: String,
    size: String,
    topping: String,
    quantity: String,
    delivery_notes: String,
    customer_name_error: Option<String>,
    size_error: Option<String>,
    topping_error: Option<String>,
    quantity_error: Option<String>,
    delivery_notes_error: Option<String>,
}

impl PizzaForm {
    pub fn initial() -> Self {
        Self {
            customer_name: String::new(),
            size: "medium".to_owned(),
            topping: "margherita".to_owned(),
            quantity: "1".to_owned(),
            delivery_notes: String::new(),
            customer_name_error: None,
            size_error: None,
            topping_error: None,
            quantity_error: None,
            delivery_notes_error: None,
        }
    }

    pub fn from_state(state: &pizza_order::State) -> Self {
        let customer_name = pizza_order::field_customer_name(state);
        let size = pizza_order::field_size(state);
        let topping = pizza_order::field_topping(state);
        let quantity = pizza_order::field_quantity(state);
        let delivery_notes = pizza_order::field_delivery_notes(state);

        Self {
            customer_name: customer_name.value_str().to_owned(),
            size: size.value_str().to_owned(),
            topping: topping.value_str().to_owned(),
            quantity: quantity.value_str().to_owned(),
            delivery_notes: delivery_notes.value_str().to_owned(),
            customer_name_error: customer_name.error_message().map(str::to_owned),
            size_error: size.error_message().map(str::to_owned),
            topping_error: topping.error_message().map(str::to_owned),
            quantity_error: quantity.error_message().map(str::to_owned),
            delivery_notes_error: delivery_notes.error_message().map(str::to_owned),
        }
    }
}

impl WebMcp for PizzaForm {
    fn describe_webmcp(&self, cx: &mut WebMcpContext) -> Result<(), WebMcpComposeError> {
        // Form-level composition is an object product: the parent owns the
        // operation while its field descriptors contribute input properties.
        cx.tool(
            CreatePizzaOrder::WEBMCP_TOOL,
            &PIZZA_ORDER_WEBMCP_PARAMETERS,
        )
    }
}

#[rsjs(component, client)]
impl<B, L> WebRenderSync<L, B> for PizzaForm
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let customer_name_error = FieldError {
            id: pizza_order::FIELD_CUSTOMER_NAME.error_id,
            message: self.customer_name_error,
        };
        let size_error = FieldError {
            id: pizza_order::FIELD_SIZE.error_id,
            message: self.size_error,
        };
        let topping_error = FieldError {
            id: pizza_order::FIELD_TOPPING.error_id,
            message: self.topping_error,
        };
        let quantity_error = FieldError {
            id: pizza_order::FIELD_QUANTITY.error_id,
            message: self.quantity_error,
        };
        let delivery_notes_error = FieldError {
            id: pizza_order::FIELD_DELIVERY_NOTES.error_id,
            message: self.delivery_notes_error,
        };

        let customer_name: rsjs::Signal<String> = signal(self.customer_name);
        let size: rsjs::Signal<String> = signal(self.size);
        let topping: rsjs::Signal<String> = signal(self.topping);
        let quantity: rsjs::Signal<String> = signal(self.quantity);
        let delivery_notes: rsjs::Signal<String> = signal(self.delivery_notes);

        html_sync! {
            <form
                data-component="pizza-order-form"
                class={FORM_PANEL}
                method="post"
                action={CreatePizzaOrder::PATH}
                toolname={CreatePizzaOrder::NAME}
                tooldescription={CreatePizzaOrder::DESCRIPTION}
            >
                <div class="panel-heading">
                    <div>
                        <p class="panel-kicker">"Native form + WebMCP"</p>
                        <h2>"Build your pizza"</h2>
                    </div>
                    <span class="agent-state" data-agent-state aria-live="polite">
                        "Waiting for a person or agent"
                    </span>
                </div>
                <div class="field-stack">
                    <label for="customer-name">
                        {pizza_order::FIELD_CUSTOMER_NAME.label_or_name()}
                    </label>
                    <input
                        id="customer-name"
                        name="customer_name"
                        type="text"
                        required={pizza_order::FIELD_CUSTOMER_NAME.required}
                        minlength="2"
                        maxlength="80"
                        autocomplete="name"
                        toolparamdescription={pizza_order::FIELD_CUSTOMER_NAME.description}
                        bind_value={customer_name}
                        aria-invalid={customer_name_error.message.is_some()}
                        aria-describedby={customer_name_error.message.is_some().then_some(customer_name_error.id)}
                    />
                    <p class="client-validation" aria-live="polite">
                        <span hidden={!customer_name_is_long_enough(customer_name.get())}>
                            "Name is valid"
                        </span>
                        <span hidden={customer_name_is_long_enough(customer_name.get())}>
                            "Enter at least two characters"
                        </span>
                    </p>
                    {customer_name_error}
                </div>
                <div class="field-row">
                    <div class="field-stack">
                        <label for="pizza-size">{pizza_order::FIELD_SIZE.label_or_name()}</label>
                        <select
                            id="pizza-size"
                            name="size"
                            required={pizza_order::FIELD_SIZE.required}
                            toolparamdescription={pizza_order::FIELD_SIZE.description}
                            bind_value={size}
                            aria-invalid={size_error.message.is_some()}
                        >
                            <option value="small" selected={size.get() == "small"}>
                                "Small · $12"
                            </option>
                            <option value="medium" selected={size.get() == "medium"}>
                                "Medium · $15"
                            </option>
                            <option value="large" selected={size.get() == "large"}>
                                "Large · $18"
                            </option>
                        </select>
                        {size_error}
                    </div>
                    <div class="field-stack">
                        <label for="pizza-quantity">
                            {pizza_order::FIELD_QUANTITY.label_or_name()}
                        </label>
                        <select
                            id="pizza-quantity"
                            name="quantity"
                            required={pizza_order::FIELD_QUANTITY.required}
                            toolparamdescription={pizza_order::FIELD_QUANTITY.description}
                            bind_value={quantity}
                            aria-invalid={quantity_error.message.is_some()}
                        >
                            <option value="1" selected={quantity.get() == "1"}>"1"</option>
                            <option value="2" selected={quantity.get() == "2"}>"2"</option>
                            <option value="3" selected={quantity.get() == "3"}>"3"</option>
                            <option value="4" selected={quantity.get() == "4"}>"4"</option>
                            <option value="5" selected={quantity.get() == "5"}>"5"</option>
                            <option value="6" selected={quantity.get() == "6"}>"6"</option>
                        </select>
                        {quantity_error}
                    </div>
                </div>
                <div class="field-stack">
                    <label for="pizza-topping">{pizza_order::FIELD_TOPPING.label_or_name()}</label>
                    <select
                        id="pizza-topping"
                        name="topping"
                        required={pizza_order::FIELD_TOPPING.required}
                        toolparamdescription={pizza_order::FIELD_TOPPING.description}
                        bind_value={topping}
                        aria-invalid={topping_error.message.is_some()}
                    >
                        <option value="margherita" selected={topping.get() == "margherita"}>
                            "Margherita"
                        </option>
                        <option value="pepperoni" selected={topping.get() == "pepperoni"}>
                            "Pepperoni"
                        </option>
                        <option value="mushroom" selected={topping.get() == "mushroom"}>
                            "Mushroom"
                        </option>
                    </select>
                    {topping_error}
                </div>
                <div class="field-stack">
                    <label for="delivery-notes">
                        {pizza_order::FIELD_DELIVERY_NOTES.label_or_name()}
                        <span>"Optional"</span>
                    </label>
                    <textarea
                        id="delivery-notes"
                        name="delivery_notes"
                        rows="3"
                        maxlength="160"
                        toolparamdescription={pizza_order::FIELD_DELIVERY_NOTES.description}
                        bind_value={delivery_notes}
                        aria-invalid={delivery_notes_error.message.is_some()}
                    ></textarea>
                    {delivery_notes_error}
                </div>
                <section data-component="pizza-preview" class={PREVIEW_PANEL} aria-live="polite">
                    <p class="panel-kicker">"RSJS live projection"</p>
                    <div class="pizza-disc" data-topping={topping.get()} aria-hidden="true">
                        <span></span>
                        <span></span>
                        <span></span>
                        <span></span>
                        <span></span>
                    </div>
                    <dl>
                        <div>
                            <dt>"For"</dt>
                            <dd>{customer_name.get()}</dd>
                        </div>
                        <div>
                            <dt>"Pizza"</dt>
                            <dd>{size.get()}" / "{topping.get()}</dd>
                        </div>
                        <div>
                            <dt>"Quantity"</dt>
                            <dd>{quantity.get()}</dd>
                        </div>
                        <div class="total">
                            <dt>"Total"</dt>
                            <dd>"$"{total_dollars(size.get(), quantity.get())}</dd>
                        </div>
                    </dl>
                </section>
                <button class="submit-button" type="submit">"Confirm order"</button>
            </form>
        }
        .render(cx)
    }
}

struct FieldError {
    id: &'static str,
    message: Option<String>,
}

impl<B, L> WebRenderSync<L, B> for FieldError
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        if let Some(message) = self.message {
            html_sync! {
                <p id={self.id} class="server-error" role="alert">{message}</p>
            }
            .render(cx)?;
        }
        Ok(())
    }
}

pub struct ReceiptPage {
    pub normalized_order: String,
}

impl<B, L> WebRenderSync<L, B> for ReceiptPage
where
    B: WriteBuf,
    L: DataLoader,
{
    fn render(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        html_sync! {
            <main data-component="order-receipt" class="receipt-shell">
                <p class="eyebrow">"SERVER OPERATION COMPLETE"</p>
                <h1>"Order accepted."</h1>
                <p class="lede">
                    "The payload below has passed the Wellformed schema and includes its normalized values."
                </p>
                <pre>{self.normalized_order}</pre>
                <a class="submit-button" href="/">"Build another pizza"</a>
            </main>
        }
        .render(cx)
    }
}
