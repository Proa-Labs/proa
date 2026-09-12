// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use proa_actions::{Action, ActionHandler};

#[derive(Debug, Clone, Action)]
#[action(scope = "user_actions")]
enum UserAction {
    DeleteUser { user_id: String },
    EditUser { user_id: String, name: String },
    ViewProfile,
}

fn main() {
    // Test creating actions
    let delete = UserAction::DeleteUser {
        user_id: "123".to_string(),
    };

    let edit = UserAction::EditUser {
        user_id: "456".to_string(),
        name: "John Doe".to_string(),
    };

    let view = UserAction::ViewProfile;

    // Test serialization
    let delete_data = delete.to_action_data();
    println!(
        "Delete action: {}",
        serde_json::to_string_pretty(&delete_data).unwrap()
    );

    let edit_data = edit.to_action_data();
    println!(
        "\nEdit action: {}",
        serde_json::to_string_pretty(&edit_data).unwrap()
    );

    let view_data = view.to_action_data();
    println!(
        "\nView action: {}",
        serde_json::to_string_pretty(&view_data).unwrap()
    );

    // Test deserialization
    let delete_restored = UserAction::from_action_data(delete_data).unwrap();
    println!("\nRestored delete: {:?}", delete_restored);

    // Test constants
    println!("\nConstants:");
    println!("  DeleteUser: {}", UserAction::DELETE_USER);
    println!("  EditUser: {}", UserAction::EDIT_USER);
    println!("  ViewProfile: {}", UserAction::VIEW_PROFILE);
}
