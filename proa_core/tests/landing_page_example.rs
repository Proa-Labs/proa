// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Verify the landing page code example compiles correctly.
use proa_core::text::text;
use proa_core::{DataLoader, WebContext, WebRenderSync, WriteError};
use proa_macros::html_sync;
#[derive(Clone, Copy)]
enum Tier {
    Free,
    Pro,
}
impl Tier {
    fn as_str(self) -> &'static str {
        match self {
            Tier::Free => "free",
            Tier::Pro => "pro",
        }
    }
}
#[derive(Clone, Copy)]
enum InventoryState {
    Available(u32),
    Low(u32),
    Out,
}
struct Product {
    name: &'static str,
    price: &'static str,
    tier: Tier,
    on_sale: bool,
    stock: InventoryState,
}
struct ProductGrid {
    products: Vec<Product>,
}
impl<L: DataLoader> WebRenderSync<L> for ProductGrid {
    fn render(self, cx: &mut WebContext<L>) -> Result<(), WriteError> {
        html_sync! {
            <div class="product-grid">
                {for product in &self.products {
                    html_sync! {
                        <div class="card" data-variant={product.tier.as_str()}>
                            <h3>{text(product.name)}</h3>
                            <p class="price">{text(product.price)}</p>
                            {if product.on_sale {
                                html_sync! {
                                    <span class="badge badge-sale">"Sale"</span>
                                }
                            }}
                            {match product.stock {
                                InventoryState::Available(n) => {
                                    html_sync! {
                                        <span class="text-green">{n}" available"</span>
                                    }
                                }
                                InventoryState::Low(n) => {
                                    html_sync! {
                                        <span class="text-amber">"Only "{n}" left"</span>
                                    }
                                }
                                InventoryState::Out => {
                                    html_sync! {
                                        <span class="text-red">"Sold out"</span>
                                    }
                                }
                            }}
                        </div>
                    }
                }}
            </div>
        }
        .render(cx)
    }
}
#[test]
fn landing_page_example_compiles_and_renders() {
    let grid = ProductGrid {
        products: vec![
            Product {
                name: "Widget",
                price: "$29",
                tier: Tier::Free,
                on_sale: true,
                stock: InventoryState::Available(42),
            },
            Product {
                name: "Gadget",
                price: "$99",
                tier: Tier::Pro,
                on_sale: false,
                stock: InventoryState::Low(3),
            },
            Product {
                name: "Doohickey",
                price: "$199",
                tier: Tier::Pro,
                on_sale: false,
                stock: InventoryState::Out,
            },
        ],
    };
    let mut cx = WebContext::new();
    grid.render(&mut cx).unwrap();
    let html = cx.into_output().into_string();
    assert!(html.contains("product-grid"));
    assert!(html.contains("Widget"));
    assert!(html.contains("Sale"));
    assert!(html.contains("42 available"));
    assert!(html.contains("Only 3 left"));
    assert!(html.contains("Sold out"));
    assert!(html.contains(r#"data-variant="pro""#));
}
