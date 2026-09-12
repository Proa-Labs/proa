// AUTO-GENERATED - DO NOT EDIT
// This file is regenerated on every build

import type { UserActionHandlers } from './contract';

let userActionHandlers: UserActionHandlers | null = null;

export function registerUserActionHandlers(handlers: UserActionHandlers) {
    userActionHandlers = handlers;
}

// Event delegation for actions
document.addEventListener('click', async (event) => {
    const target = event.target as HTMLElement;
    const actionEl = target.closest('[data-action]');
    if (!actionEl) return;

    const actionName = actionEl.getAttribute('data-action');
    if (!actionName) return;

    // Parse parameters from data attributes
    const params: Record<string, string> = {};
    for (const attr of Array.from(actionEl.attributes)) {
        if (attr.name.startsWith('data-param-')) {
            const paramName = attr.name.replace('data-param-', '');
            params[paramName] = attr.value;
        }
    }

    if (actionName.startsWith('UserAction.')) {
        if (!userActionHandlers) {
            console.error('No handlers registered for UserAction');
            return;
        }
        const handler = userActionHandlers[actionName as keyof UserActionHandlers];
        if (handler) {
            try {
                await handler(params as any);
            } catch (error) {
                console.error('Action handler error:', error);
            }
        }
    }

});
