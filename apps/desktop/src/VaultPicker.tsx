import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, ConnectRemoteOutcome, PendingSignup, VaultInfo } from "./api";

type Mode = "menu" | "remote";

/// Entry point for adding a workspace — either a local folder or a
/// remote `orchext-server` connection. Triggered from first-run
/// (App boot with empty registry) and from the rail's "+ Add"
/// affordance after launch.
///
/// Phase 3 platform Slice 1 added the remote path: the desktop's
/// equivalent of the web client's signup/login flow. Returns a
/// connected workspace via `onOpened`, or surfaces an awaiting-
/// approval state via `onPending` for App.tsx to gate on.
export function VaultPicker({
  onOpened,
  onPending,
  onCancel,
  title = "Orchext",
}: {
  onOpened: (v: VaultInfo) => void;
  onPending?: (info: PendingApprovalInfo) => void;
  onCancel?: () => void;
  title?: string;
}) {
  const [mode, setMode] = useState<Mode>("menu");
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

  if (mode === "remote") {
    return (
      <div className="h-full flex items-center justify-center">
        <div className="max-w-md w-full p-8 bg-white rounded-xl shadow-sm border border-neutral-200">
          <h1 className="text-2xl font-semibold text-neutral-900 mb-2">{title}</h1>
          <p className="text-neutral-600 mb-6 text-sm">
            Connect to an Orchext server. Sign in with your existing
            account; if your account is awaiting admin approval,
            you&apos;ll be told here.
          </p>
          <RemoteConnectForm
            onConnected={onOpened}
            onPending={(p) => onPending?.(p)}
            onCancel={() => setMode("menu")}
            onError={setErr}
          />
          {err && (
            <div className="mt-4 p-3 bg-red-50 text-red-700 text-sm rounded-lg border border-red-200">
              {err}
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="h-full flex items-center justify-center">
      <div className="max-w-md w-full p-8 bg-white rounded-xl shadow-sm border border-neutral-200">
        <h1 className="text-2xl font-semibold text-neutral-900 mb-2">{title}</h1>
        <p className="text-neutral-600 mb-6 text-sm">
          Add a workspace. Local vaults live entirely on this machine;
          remote workspaces connect to an Orchext server and surface
          your personal vault plus any organizations you&apos;ve been
          added to.
        </p>
        <button
          onClick={pickAndOpen}
          disabled={busy}
          className="w-full bg-brand-600 text-white py-2.5 px-4 rounded-lg hover:bg-brand-700 transition disabled:opacity-50"
        >
          {busy ? "Opening…" : "Open local folder…"}
        </button>
        <button
          onClick={() => setMode("remote")}
          disabled={busy}
          className="mt-3 w-full bg-white border border-neutral-300 text-neutral-800 py-2.5 px-4 rounded-lg hover:bg-neutral-50 transition disabled:opacity-50"
        >
          Connect to a server…
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

export type PendingApprovalInfo = {
  serverUrl: string;
  accountEmail: string;
  pending: PendingSignup[];
};

function RemoteConnectForm({
  onConnected,
  onPending,
  onCancel,
  onError,
}: {
  onConnected: (v: VaultInfo) => void;
  onPending: (info: PendingApprovalInfo) => void;
  onCancel: () => void;
  onError: (msg: string | null) => void;
}) {
  const [serverUrl, setServerUrl] = useState("");
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    onError(null);
    if (!serverUrl.trim() || !email.trim() || !password) return;
    setBusy(true);
    try {
      const outcome: ConnectRemoteOutcome = await api.workspaceConnectRemote({
        server_url: serverUrl.trim(),
        email: email.trim(),
        password,
      });
      if (outcome.kind === "connected") {
        onConnected(outcome.workspace);
      } else {
        onPending({
          serverUrl: outcome.server_url,
          accountEmail: outcome.account_email,
          pending: outcome.pending,
        });
      }
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3">
      <Field label="Server URL">
        <input
          type="url"
          value={serverUrl}
          onChange={(e) => setServerUrl(e.target.value)}
          placeholder="https://orchext.example.com"
          className="w-full px-3 py-2 border border-neutral-300 rounded text-sm"
          disabled={busy}
        />
      </Field>
      <Field label="Email">
        <input
          type="email"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          className="w-full px-3 py-2 border border-neutral-300 rounded text-sm"
          disabled={busy}
        />
      </Field>
      <Field label="Password">
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className="w-full px-3 py-2 border border-neutral-300 rounded text-sm"
          disabled={busy}
        />
      </Field>
      <div className="flex items-center gap-3 pt-2">
        <button
          type="submit"
          disabled={busy || !serverUrl.trim() || !email.trim() || !password}
          className="text-sm px-3 py-1.5 rounded bg-brand-600 text-white hover:bg-brand-700 disabled:opacity-50"
        >
          {busy ? "Connecting…" : "Connect"}
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="text-sm text-neutral-500 hover:text-neutral-900"
        >
          Back
        </button>
      </div>
    </form>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <label className="block">
      <span className="block text-xs font-medium text-neutral-700 mb-1">
        {label}
      </span>
      {children}
    </label>
  );
}
