// Thin wrapper around the generated WASM bindings.
//
// wasm-pack emits an async `default()` initializer that fetches and
// instantiates `orchext_crypto_wasm_bg.wasm`. We cache the resulting
// promise so concurrent callers share one instance — React Strict Mode
// and rapid route changes will trigger multiple calls otherwise.
import init, {
  generateContentKey,
  generateSalt,
  makeKeyCheck,
  unwrapContentKey,
  wrapContentKey,
} from "./wasm/orchext_crypto_wasm";

let ready: Promise<void> | null = null;

function ensureReady(): Promise<void> {
  if (!ready) {
    ready = init().then(() => undefined);
  }
  return ready;
}

export const crypto = {
  async generateSalt(): Promise<string> {
    await ensureReady();
    return generateSalt();
  },
  async generateContentKey(): Promise<string> {
    await ensureReady();
    return generateContentKey();
  },
  async wrapContentKey(
    contentWire: string,
    passphrase: string,
    saltWire: string
  ): Promise<string> {
    await ensureReady();
    return wrapContentKey(contentWire, passphrase, saltWire);
  },
  async unwrapContentKey(
    wrappedWire: string,
    passphrase: string,
    saltWire: string
  ): Promise<string> {
    await ensureReady();
    return unwrapContentKey(wrappedWire, passphrase, saltWire);
  },
  async makeKeyCheck(contentWire: string): Promise<string> {
    await ensureReady();
    return makeKeyCheck(contentWire);
  },
};
