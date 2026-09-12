// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Simplified demonstration of the action system
//! This example demonstrates the codegen without HTML rendering

use proa_actions::{codegen::*, Action, ActionHandler, ActionMetadata, ActionParam, ActionVariant};

// Define actions for user profile page
#[derive(Debug, Clone, Action)]
#[action(scope = "user_profile")]
enum UserAction {
    DeleteUser { user_id: String },
    EditUser { user_id: String, name: String },
    ViewProfile { user_id: String },
    ExportData,
}

fn main() {
    println!("=== Proa Actions System Demo ===\n");

    // 1. Test action serialization
    println!("1. Testing action serialization:");
    let delete = UserAction::DeleteUser {
        user_id: "usr_123".to_string(),
    };
    let action_data = delete.to_action_data();
    println!("   Action: {}", action_data.action);
    println!("   Params: {}\n", action_data.params);

    // 2. Create metadata for codegen
    let user_action_metadata = ActionMetadata {
        scope: "user_profile".to_string(),
        name: "UserAction".to_string(),
        variants: vec![
            ActionVariant {
                name: "DeleteUser".to_string(),
                constant: "DELETE_USER".to_string(),
                params: vec![ActionParam {
                    name: "user_id".to_string(),
                    type_name: "String".to_string(),
                }],
                handler: None,
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
                handler: None,
            },
            ActionVariant {
                name: "ViewProfile".to_string(),
                constant: "VIEW_PROFILE".to_string(),
                params: vec![ActionParam {
                    name: "user_id".to_string(),
                    type_name: "String".to_string(),
                }],
                handler: None,
            },
            ActionVariant {
                name: "ExportData".to_string(),
                constant: "EXPORT_DATA".to_string(),
                params: vec![],
                handler: None,
            },
        ],
    };

    let actions = vec![user_action_metadata];

    // 3. Generate TypeScript files
    println!("2. Generating TypeScript files:");

    // Generate contract
    let contract = generate_contract(&actions);
    std::fs::write("user_profile.contract.ts", &contract).expect("Failed to write contract");
    println!("   ✓ Generated user_profile.contract.ts");

    // Generate runtime
    let runtime = generate_runtime(&actions, "user-profile");
    std::fs::write("user_profile.runtime.ts", &runtime).expect("Failed to write runtime");
    println!("   ✓ Generated user_profile.runtime.ts");

    // Generate handler stub
    let handlers = generate_handler_stub(&actions, "user-profile");
    std::fs::write("user_profile.handlers.ts", &handlers).expect("Failed to write handlers");
    println!("   ✓ Generated user_profile.handlers.ts\n");

    // 4. Generate example HTML
    println!("3. Generating example HTML:");
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8" />
    <title>Proa Actions Demo</title>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body class="p-8 bg-gray-50">
    <div class="max-w-2xl mx-auto">
        <div class="bg-white rounded-lg shadow-md p-6">
            <h1 class="text-2xl font-bold mb-4">User Profile - John Doe</h1>

            <div class="space-y-2 mb-6">
                <p><span class="font-semibold">Email:</span> john.doe@example.com</p>
                <p><span class="font-semibold">ID:</span> <span class="text-gray-500">usr_123</span></p>
            </div>

            <div class="flex gap-3">
                <button
                    type="button"
                    class="px-4 py-2 bg-blue-600 text-white rounded hover:bg-blue-700"
                    data-action="UserAction.ViewProfile"
                    data-param-user_id="usr_123"
                >
                    View Profile
                </button>

                <button
                    type="button"
                    class="px-4 py-2 bg-green-600 text-white rounded hover:bg-green-700"
                    data-action="UserAction.EditUser"
                    data-param-user_id="usr_123"
                    data-param-name="John Doe"
                >
                    Edit User
                </button>

                <button
                    type="button"
                    class="px-4 py-2 bg-red-600 text-white rounded hover:bg-red-700"
                    data-action="UserAction.DeleteUser"
                    data-param-user_id="usr_123"
                >
                    Delete User
                </button>

                <button
                    type="button"
                    class="px-4 py-2 bg-gray-600 text-white rounded hover:bg-gray-700"
                    data-action="UserAction.ExportData"
                >
                    Export Data
                </button>
            </div>
        </div>

        <div class="mt-6 p-4 bg-blue-50 rounded-lg">
            <h2 class="text-lg font-semibold mb-2">📝 Instructions</h2>
            <ul class="list-disc list-inside space-y-1 text-sm text-gray-700">
                <li>Open browser console to see action logs</li>
                <li>Click buttons to trigger actions</li>
                <li>Check user_profile.handlers.ts to implement logic</li>
                <li>All actions are type-safe end-to-end</li>
            </ul>
        </div>
    </div>

    <script type="module">
        // Simple runtime for demo (inline version of generated runtime)
        const userActionHandlers = {{}};

        function registerUserActionHandlers(handlers) {{
            Object.assign(userActionHandlers, handlers);
        }}

        document.addEventListener('click', async (event) => {{
            const target = event.target;
            const actionEl = target.closest('[data-action]');
            if (!actionEl) return;

            const actionName = actionEl.getAttribute('data-action');
            if (!actionName) return;

            const params = {{}};
            for (const attr of Array.from(actionEl.attributes)) {{
                if (attr.name.startsWith('data-param-')) {{
                    const paramName = attr.name.replace('data-param-', '');
                    params[paramName] = attr.value;
                }}
            }}

            const handler = userActionHandlers[actionName];
            if (handler) {{
                try {{
                    await handler(params);
                }} catch (error) {{
                    console.error('Action handler error:', error);
                }}
            }} else {{
                console.warn('No handler registered for:', actionName);
            }}
        }});

        // Register demo handlers
        registerUserActionHandlers({{
            'UserAction.DELETE_USER': async (params) => {{
                console.log('🗑️  DeleteUser called with:', params);
                alert(`Delete user ${{params.user_id}}?`);
            }},
            'UserAction.EDIT_USER': async (params) => {{
                console.log('✏️  EditUser called with:', params);
                alert(`Edit user ${{params.user_id}} (${{params.name}})`);
            }},
            'UserAction.VIEW_PROFILE': async (params) => {{
                console.log('👀 ViewProfile called with:', params);
                alert(`View profile for ${{params.user_id}}`);
            }},
            'UserAction.EXPORT_DATA': async () => {{
                console.log('📦 ExportData called');
                alert('Exporting data...');
            }},
        }});

        console.log('✅ Action handlers registered!');
    </script>
</body>
</html>"#.to_string();

    std::fs::write("simple_demo.html", &html).expect("Failed to write HTML");
    println!("   ✓ Generated simple_demo.html\n");

    println!("🎉 Demo complete!");
    println!();
    println!("Next steps:");
    println!("  1. Open simple_demo.html in your browser");
    println!("  2. Click the buttons to see actions in action");
    println!("  3. Check the TypeScript files to see the generated code");
}
