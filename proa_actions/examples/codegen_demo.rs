// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_actions::codegen::*;
use proa_actions::{ActionHandlerMetadata, ActionMetadata, ActionParam, ActionVariant};

fn main() {
    // Define action metadata (normally this would be extracted from Rust code)
    let user_action = ActionMetadata {
        scope: "user".to_string(),
        name: "UserAction".to_string(),
        variants: vec![
            ActionVariant {
                name: "DeleteUser".to_string(),
                constant: "DELETE_USER".to_string(),
                params: vec![ActionParam {
                    name: "user_id".to_string(),
                    type_name: "String".to_string(),
                }],
                handler: Some(ActionHandlerMetadata {
                    event: Some("click".to_string()),
                    element: Some("HTMLButtonElement".to_string()),
                    ts_event_type: None,
                    prevent_default: true,
                }),
            },
            ActionVariant {
                name: "EditUser".to_string(),
                constant: "EDIT_USER".to_string(),
                params: vec![
                    ActionParam {
                        name: "user_id".to_string(),
                        type_name: "String".to_string(),
                    },
                    ActionParam {
                        name: "name".to_string(),
                        type_name: "String".to_string(),
                    },
                ],
                handler: Some(ActionHandlerMetadata {
                    event: Some("click".to_string()),
                    element: Some("HTMLButtonElement".to_string()),
                    ts_event_type: None,
                    prevent_default: true,
                }),
            },
            ActionVariant {
                name: "ViewProfile".to_string(),
                constant: "VIEW_PROFILE".to_string(),
                params: vec![],
                handler: Some(ActionHandlerMetadata {
                    event: Some("click".to_string()),
                    element: Some("HTMLButtonElement".to_string()),
                    ts_event_type: None,
                    prevent_default: false,
                }),
            },
        ],
    };

    let actions = vec![user_action];

    // Generate TypeScript files
    println!("=== CONTRACT.TS ===\n");
    println!("{}", generate_contract(&actions));

    println!("\n=== RUNTIME.TS ===\n");
    println!("{}", generate_runtime(&actions, "user-profile"));

    println!("\n=== HANDLERS.TS (stub) ===\n");
    println!("{}", generate_handler_stub(&actions, "user-profile"));
}
