// AUTO-GENERATED - DO NOT EDIT
// This file is regenerated on every build

export namespace UserAction {
    export const DELETE_USER = "DeleteUser" as const;
    export const EDIT_USER = "EditUser" as const;
    export const VIEW_PROFILE = "ViewProfile" as const;
    export const EXPORT_DATA = "ExportData" as const;
}

export interface DeleteUserParams {
    user_id: string;
}

export interface EditUserParams {
    user_id: string;
    name: string;
}

export interface ViewProfileParams {
    user_id: string;
}

export type Handler<P = undefined> = P extends undefined
    ? () => void | Promise<void>
    : (params: P) => void | Promise<void>;

export interface UserActionHandlers {
    [UserAction.DELETE_USER]: Handler<DeleteUserParams>;
    [UserAction.EDIT_USER]: Handler<EditUserParams>;
    [UserAction.VIEW_PROFILE]: Handler<ViewProfileParams>;
    [UserAction.EXPORT_DATA]: Handler;
}

