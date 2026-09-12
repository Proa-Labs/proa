# Proa Actions

Type-safe, compile-time action system for Proa render components with automatic
TypeScript generation and collision detection.

## Features

- **Scoped Actions**: Namespace actions with `#[action(scope = "...")]` to prevent collisions
- **TypeScript Codegen**: Auto-generate TypeScript contracts, runtime, and handler stubs
- **Compile-Time Safety**: Action names and parameters are validated at compile time
- **Collision Detection**: Build-time checks prevent duplicate action names
- **Zero-Cost Abstractions**: Stack-allocated action data with no heap allocations
- **Multi-Event Support**: onclick, onblur, onfocus, and more

## Quick Start

```rust
use proa_actions::{Action, ActionDataStatic};

#[derive(Action)]
#[action(scope = "home_page")]
pub enum ButtonAction {
    HandleClick,
    HandleSave { autosave: bool, notify_user: bool },
}

// Use generated constants
const CLICK: ActionDataStatic = ActionDataStatic::new(ButtonAction::HANDLE_CLICK);
// Generated: "home_page.ButtonAction.HandleClick"
```

## Architecture

### Rust Side

1. **Define Actions**: Use `#[derive(Action)]` with `#[action(scope = "...")]`
2. **Action Data**: Use `ActionDataStatic` for compile-time data
3. **Attach to Components**: Add actions to Button, Input, etc.

### TypeScript Side

1. **Generated Contract** (`*.contract.ts`): Type-safe action constants
2. **Generated Runtime** (`*.runtime.ts`): Event delegation system
3. **User Handlers** (`*.handlers.ts`): Implement action logic

## Module Organization

```
proa_actions/
├── dynamic.rs         - ActionData, ActionHandler trait, metadata types
├── static_data.rs     - ActionDataStatic (zero-cost, stack-allocated)
├── codegen.rs         - TypeScript file generation
└── collision_check.rs - Build-time collision detection
```

### Why Two Action Types?

- **`ActionDataStatic`**: Compile-time, stack-allocated, zero heap allocations
  - Use for: HTML rendering, const definitions
  - Example: `const ACTION: ActionDataStatic = ActionDataStatic::new("demo.ButtonAction.Click")`

- **`ActionData`**: Runtime, heap-allocated, fully dynamic
  - Use for: Deserialization, API payloads, dynamic creation
  - Example: Parsing incoming requests, database storage

## Scoped Actions

Every action requires a unique scope to prevent naming collisions:

```rust
#[derive(Action)]
#[action(scope = "home_page")]    // ← Required!
pub enum HomeActions {
    ViewProfile,
    SendMessage { user_id: u32 },
}
```

**Generated names**: `home_page.HomeActions.ViewProfile`, `home_page.HomeActions.SendMessage`

### Collision Detection

Add this test to your project:

```rust
// tests/collision_check.rs
#[test]
fn test_no_action_collisions() {
    proa_actions::check_action_collisions();
}
```

**Error Example**:
```
❌ ACTION NAME COLLISION DETECTED!

The following action names are defined multiple times:
  • "demo.ButtonAction" appears 3 times

To fix: Choose a different scope name or rename the enum.
```

## TypeScript Integration

### Generated Files

For each action enum, three files are generated:

1. **Contract** (`*.contract.ts`):
   ```typescript
   export namespace ButtonAction {
       export const HANDLE_CLICK = "demo.ButtonAction.HandleClick" as const;
   }

   export interface ButtonActionHandlers {
       [ButtonAction.HANDLE_CLICK]: Handler;
   }
   ```

2. **Runtime** (`*.runtime.ts`):
   ```typescript
   // Event delegation for all event types
   document.addEventListener('click', async (event) => {
       const actionName = target.getAttribute('data-onclick');
       const handler = handlers[actionName];
       await handler(params, event);
   });
   ```

3. **Handlers** (`*.handlers.ts`) - User-written, created once:
   ```typescript
   import { ButtonAction } from './generated/demo.contract';

   const buttonActionHandlers = {
       [ButtonAction.HANDLE_CLICK]: async () => {
           console.log('Button clicked!');
       },
   };
   ```

### Build Script

Generate TypeScript in `build.rs`:

```rust
fn main() {
    use proa_actions::codegen;

    let metadata = vec![/* ... */];

    let contract = codegen::generate_contract(&metadata);
    fs::write("generated/demo.contract.ts", contract)?;

    let runtime = codegen::generate_runtime(&metadata, "demo");
    fs::write("generated/demo.runtime.ts", runtime)?;
}
```

## Multi-Event Support

Actions work with multiple event types:

```rust
Button {
    onclick: Some(ActionDataStatic::new(ButtonAction::HANDLE_CLICK)),
    onblur: Some(ActionDataStatic::new(ButtonAction::HANDLE_BLUR)),
    onfocus: Some(ActionDataStatic::new(ButtonAction::HANDLE_FOCUS)),
    ..Button::new()
}
```

**Renders as**:
```html
<button
  data-onclick="demo.ButtonAction.HandleClick"
  data-onblur="demo.ButtonAction.HandleBlur"
  data-onfocus="demo.ButtonAction.HandleFocus">
  Click Me!
</button>
```

Supported events: `click`, `blur`, `focus`, `mouseover`, `mouseout`, `change`, `input`

## Examples

See `/examples/simple_demo.rs` for a complete example with:
- Action enum definition
- TypeScript generation
- HTML rendering
- Client-side handlers

## Best Practices

### Scope Naming

✅ **Good**:
- `"home_page"` - Specific page
- `"admin_users"` - Feature domain
- `"checkout_flow"` - User workflow

❌ **Bad**:
- `"page"` - Too generic
- `"actions"` - Meaningless
- `"test"` - Temporary names

### When to Use Scopes

- **Different Pages**: Always use different scopes
- **Different Features**: Use different scopes for major features
- **Related Actions**: Can share a scope if tightly coupled

## Performance

- **Zero heap allocations** for `ActionDataStatic`
- **Event delegation** (1 listener vs N element listeners)
- **Compile-time JSON serialization** with `const_json`
- **Stack-allocated** action data structures

## Migration from Non-Scoped Actions

1. Add `#[action(scope = "...")]` to all `#[derive(Action)]`
2. Use generated constants: `ButtonAction::HANDLE_CLICK` instead of string literals
3. Add collision detection test
4. Rebuild project

## See Also

- [Simple action example](examples/simple_action.rs) - Minimal scoped-action usage
- [TypeScript code generator](src/codegen.rs) - Multi-event runtime and handler generation
