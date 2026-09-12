// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Build-time collision detection for action names
//!
//! Call `check_action_collisions()` from your build.rs to detect duplicate
//! action scope+name combinations across your workspace.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug)]
pub struct ActionCollision {
    pub action_name: String,
    pub count: usize,
}

/// Check for action name collisions and panic if any are found
///
/// This should be called from build.rs:
///
/// ```no_run
/// proa_actions::check_action_collisions();
/// ```
pub fn check_action_collisions() {
    let registry_path = get_registry_path();

    if !registry_path.exists() {
        // No registry file yet - this is OK, might be first build
        return;
    }

    let contents = fs::read_to_string(&registry_path).expect("Failed to read action registry");

    let mut action_counts: HashMap<String, usize> = HashMap::new();

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        *action_counts.entry(line.to_string()).or_insert(0) += 1;
    }

    let collisions: Vec<ActionCollision> = action_counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(action_name, count)| ActionCollision { action_name, count })
        .collect();

    if !collisions.is_empty() {
        let mut error_msg = String::from(
            "\n\n❌ ACTION NAME COLLISION DETECTED!\n\n\
             The following action names are defined multiple times:\n\n",
        );

        for collision in &collisions {
            error_msg.push_str(&format!(
                "  • \"{}\" appears {} times\n",
                collision.action_name, collision.count
            ));
        }

        error_msg.push_str(
            "\nEach action must have a unique scope + name combination.\n\
             To fix this:\n\
             1. Choose a different scope name using #[action(scope = \"unique_name\")]\n\
             2. Or rename one of the conflicting action enums\n\n\
             Example:\n\
             #[derive(Action)]\n\
             #[action(scope = \"home_page\")]  // ← Make this unique!\n\
             enum ButtonAction { ... }\n\n",
        );

        panic!("{}", error_msg);
    }
}

/// Clear the action registry (useful for clean builds)
pub fn clear_action_registry() {
    let registry_path = get_registry_path();
    if registry_path.exists() {
        fs::remove_file(&registry_path).ok();
    }
}

fn get_registry_path() -> PathBuf {
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .or_else(|_| {
            std::env::var("OUT_DIR").map(|out| {
                PathBuf::from(out)
                    .ancestors()
                    .find(|p| p.ends_with("target"))
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| "target".to_string())
            })
        })
        .unwrap_or_else(|_| "target".to_string());

    PathBuf::from(target_dir).join("proa_action_registry.txt")
}
