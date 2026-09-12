// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use wellformed_macros::form_schema;

use crate::webmcp::{Confirmation, WebMcpParameter, WebMcpTool};

form_schema!(pub mod pizza_order = "schemas/pizza-order.json");

/// Transport-independent identity for the operation demonstrated by the app.
///
/// Proa does not have the proposed `Operation` abstraction yet, so the demo
/// keeps its identity in one small type and reuses it in the HTML form, route,
/// tests, and explanatory UI.
pub struct CreatePizzaOrder;

impl CreatePizzaOrder {
    pub const NAME: &'static str = "create_pizza_order";
    pub const DESCRIPTION: &'static str =
        "Configure a pizza order, preview it, and ask the user to confirm submission";
    pub const PATH: &'static str = "/orders";

    pub const WEBMCP_TOOL: WebMcpTool =
        WebMcpTool::new(Self::NAME, Self::DESCRIPTION, Confirmation::User);
}

/// Field-level WebMCP contributions for `CreatePizzaOrder`.
///
/// The descriptions and required flags come from the same generated
/// Wellformed facade used by the HTML controls. Each descriptor implements
/// `WebMcp`, so a future field component could contain and delegate to one of
/// these values directly.
pub const PIZZA_ORDER_WEBMCP_PARAMETERS: [WebMcpParameter; 5] = [
    WebMcpParameter::new(
        "customer_name",
        pizza_order::FIELD_CUSTOMER_NAME.description,
        pizza_order::FIELD_CUSTOMER_NAME.required,
    ),
    WebMcpParameter::new(
        "size",
        pizza_order::FIELD_SIZE.description,
        pizza_order::FIELD_SIZE.required,
    ),
    WebMcpParameter::new(
        "topping",
        pizza_order::FIELD_TOPPING.description,
        pizza_order::FIELD_TOPPING.required,
    ),
    WebMcpParameter::new(
        "quantity",
        pizza_order::FIELD_QUANTITY.description,
        pizza_order::FIELD_QUANTITY.required,
    ),
    WebMcpParameter::new(
        "delivery_notes",
        pizza_order::FIELD_DELIVERY_NOTES.description,
        pizza_order::FIELD_DELIVERY_NOTES.required,
    ),
];
