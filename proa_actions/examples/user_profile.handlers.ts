// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// USER-WRITTEN HANDLERS
// This file is only created once and is never overwritten

import { UserAction, UserActionHandlers } from '../generated/user-profile.contract';
import { registerUserActionHandlers } from '../generated/user-profile.runtime';

const userActionHandlers: UserActionHandlers = {
    [UserAction.DELETE_USER]: async (params) => {
        // TODO: Implement handler
        console.log('Handler called:', UserAction.DELETE_USER, params);
    },
    [UserAction.EDIT_USER]: async (params) => {
        // TODO: Implement handler
        console.log('Handler called:', UserAction.EDIT_USER, params);
    },
    [UserAction.VIEW_PROFILE]: async (params) => {
        // TODO: Implement handler
        console.log('Handler called:', UserAction.VIEW_PROFILE, params);
    },
    [UserAction.EXPORT_DATA]: async () => {
        // TODO: Implement handler
        console.log('Handler called:', UserAction.EXPORT_DATA);
    },
};

registerUserActionHandlers(userActionHandlers);

