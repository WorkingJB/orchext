/* tslint:disable */
/* eslint-disable */

/**
 * Fresh 32-byte content key as base64url-nopad. Called once at
 * `init-crypto` time; after that, the same key is unwrapped from
 * storage for the lifetime of the workspace (key rotation is future
 * work, see `phase-2b3-encryption.md`).
 */
export function generateContentKey(): string;

/**
 * Fresh 16-byte KDF salt as base64url-nopad. Called once at
 * `init-crypto` time for a new workspace.
 */
export function generateSalt(): string;

/**
 * Build a key-check blob from a content key. Sent to the server on
 * `init-crypto` so subsequent `publish_session_key` calls can be
 * verified to be using the same key.
 */
export function makeKeyCheck(content_wire: string): string;

/**
 * Derive a master key from `passphrase` + `salt_wire` and unseal the
 * stored wrapped content key. Any failure — wrong passphrase, bad
 * wire, tampered blob — surfaces as the single `decryption failed`
 * error, matching the Rust crate's enumeration-resistance posture.
 */
export function unwrapContentKey(wrapped_wire: string, passphrase: string, salt_wire: string): string;

/**
 * Derive a master key from `passphrase` + `salt_wire` (Argon2id) and
 * seal the content key under it. Output is the base64url-nopad
 * sealed blob that `POST /v1/t/:tid/vault/init-crypto` stores in
 * `tenants.wrapped_content_key`.
 */
export function wrapContentKey(content_wire: string, passphrase: string, salt_wire: string): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly generateContentKey: (a: number) => void;
    readonly generateSalt: (a: number) => void;
    readonly makeKeyCheck: (a: number, b: number, c: number) => void;
    readonly unwrapContentKey: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly wrapContentKey: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly __wbindgen_export: (a: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export3: (a: number, b: number) => number;
    readonly __wbindgen_export4: (a: number, b: number, c: number, d: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
