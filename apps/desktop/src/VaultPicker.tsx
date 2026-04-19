import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, VaultInfo } from "./api";

export function VaultPicker({
  onOpened,
  onCancel,
  title = "Mytex",
  description,
}: {
  onOpened: (v: VaultInfo) => void;
  onCancel?: () => void;
  title?: string;
  description?: string;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  async function pickAndOpen() {
    setErr(null);
    const selected = await open({ directory: true, multiple: false });
    if (!selected || typeof selected !== "string") {
      return;
    }
    setBusy(true);
    try {
      const info = await api.workspaceAdd(selected);
      onOpened(info);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="h-full flex items-center justify-center">
      <div className="max-w-md w-full p-8 bg-white rounded-xl shadow-sm border border-neutral-200">
        <h1 className="text-2xl font-semibold text-neutral-900 mb-2">{title}</h1>
        <p className="text-neutral-600 mb-6 text-sm">
          {description ?? (
            <>
              Pick a folder to use as your vault. If it's empty, Mytex will
              create the skeleton (type directories +{" "}
              <span className="font-mono">.mytex/</span>). Existing vaults open
              in place.
            </>
          )}
        </p>
        <button
          onClick={pickAndOpen}
          disabled={busy}
          className="w-full bg-brand-600 text-white py-2.5 px-4 rounded-lg hover:bg-brand-700 transition disabled:opacity-50"
        >
          {busy ? "Opening…" : "Choose vault folder"}
        </button>
        {onCancel && (
          <button
            onClick={onCancel}
            className="mt-3 w-full text-sm text-neutral-500 hover:text-neutral-900 py-2"
          >
            Cancel
          </button>
        )}
        {err && (
          <div className="mt-4 p-3 bg-red-50 text-red-700 text-sm rounded-lg border border-red-200">
            {err}
          </div>
        )}
      </div>
    </div>
  );
}
