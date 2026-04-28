import { useEffect, useRef, useState } from "react";
import { api, ApiFailure, Organization } from "./api";
import { Context } from "./OrgRail";

/// Org settings pane (Phase 3 platform Slice 1; logo upload added in
/// Slice 2). Admin/owner only — gated by App.tsx.
///
/// `allowed_domains` is rendered read-only with a "available when
/// email infra ships" note, per D17e: the column lands in the schema
/// now but the auto-join code path won't fire until SMTP + email
/// verification are wired.
///
/// Logo: uploaded as a file (PNG / JPEG / GIF / WEBP, ≤ 512KB) and
/// served from `/v1/orgs/:id/logo`. Slice 1's external-URL field is
/// gone — external references didn't render reliably and the bytes-
/// in-Postgres path is simpler.
export function OrgSettingsView({
  ctx,
  onUpdated,
}: {
  ctx: Context & { kind: "org" };
  onUpdated: (org: Organization) => void;
}) {
  const [org, setOrg] = useState<Organization | null>(null);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [logoBusy, setLogoBusy] = useState<"uploading" | "removing" | null>(
    null
  );
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    let cancelled = false;
    setOrg(null);
    setError(null);
    api
      .orgGet(ctx.orgId)
      .then((o) => {
        if (cancelled) return;
        setOrg(o);
        setName(o.name);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof ApiFailure ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [ctx.orgId]);

  async function save(e: React.FormEvent) {
    e.preventDefault();
    if (!org) return;
    const trimmedName = name.trim();
    if (trimmedName.length === 0) {
      setError("Name must not be empty.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const updated = await api.orgUpdate(org.id, {
        name: trimmedName,
      });
      setOrg(updated);
      setSavedAt(Date.now());
      onUpdated(updated);
    } catch (e) {
      setError(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function uploadLogo(file: File) {
    if (!org) return;
    setLogoBusy("uploading");
    setError(null);
    try {
      const result = await api.orgLogoUpload(org.id, file);
      const updated = { ...org, logo_url: result.logo_url };
      setOrg(updated);
      onUpdated(updated);
    } catch (e) {
      setError(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setLogoBusy(null);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  }

  async function removeLogo() {
    if (!org) return;
    if (!confirm("Remove the organization logo?")) return;
    setLogoBusy("removing");
    setError(null);
    try {
      await api.orgLogoDelete(org.id);
      const updated = { ...org, logo_url: null };
      setOrg(updated);
      onUpdated(updated);
    } catch (e) {
      setError(e instanceof ApiFailure ? e.message : String(e));
    } finally {
      setLogoBusy(null);
    }
  }

  if (!org && !error) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500 dark:text-neutral-400">
        Loading settings…
      </div>
    );
  }

  return (
    <div className="h-full overflow-auto p-6">
      <div className="max-w-2xl mx-auto space-y-6">
        <header>
          <h1 className="text-xl font-semibold">Organization settings</h1>
        </header>

        {error && (
          <div className="bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 text-red-700 dark:text-red-400 text-sm rounded-md p-3">
            {error}
          </div>
        )}

        {org && (
          <form
            onSubmit={save}
            className="bg-white dark:bg-neutral-900 border border-neutral-200 dark:border-neutral-800 rounded-md p-5 space-y-4"
          >
            <Field label="Name">
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full border border-neutral-300 dark:border-neutral-700 rounded px-3 py-2 text-sm"
                disabled={busy}
              />
            </Field>

            <Field label="Logo">
              <div className="flex items-center gap-3">
                <div className="w-14 h-14 rounded-md border border-neutral-200 dark:border-neutral-800 bg-neutral-50 dark:bg-neutral-900 overflow-hidden flex items-center justify-center text-xs text-neutral-400 dark:text-neutral-500">
                  {org.logo_url ? (
                    <img
                      src={org.logo_url}
                      alt=""
                      className="w-full h-full object-cover"
                    />
                  ) : (
                    "—"
                  )}
                </div>
                <div className="flex flex-col gap-1.5">
                  <input
                    ref={fileInputRef}
                    type="file"
                    accept="image/png,image/jpeg,image/gif,image/webp"
                    onChange={(e) => {
                      const file = e.target.files?.[0];
                      if (file) void uploadLogo(file);
                    }}
                    disabled={logoBusy !== null}
                    className="text-xs"
                  />
                  {org.logo_url && (
                    <button
                      type="button"
                      onClick={removeLogo}
                      disabled={logoBusy !== null}
                      className="text-xs text-red-700 dark:text-red-400 hover:underline self-start disabled:opacity-50"
                    >
                      Remove logo
                    </button>
                  )}
                </div>
              </div>
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-2">
                PNG, JPEG, GIF, or WEBP up to 512KB. Shown as the
                org&apos;s avatar in the left rail.
                {logoBusy === "uploading" && " Uploading…"}
                {logoBusy === "removing" && " Removing…"}
              </p>
            </Field>

            <Field label="Allowed domains">
              <input
                type="text"
                value={(org.allowed_domains ?? []).join(", ")}
                disabled
                className="w-full border border-neutral-200 dark:border-neutral-800 rounded px-3 py-2 text-sm bg-neutral-50 dark:bg-neutral-900 text-neutral-500 dark:text-neutral-400"
              />
              <p className="text-xs text-neutral-500 dark:text-neutral-400 mt-1">
                Available when email infra ships — auto-join from a
                matching corporate email currently still goes through
                the approval queue.
              </p>
            </Field>

            <div className="flex items-center gap-3 pt-2">
              <button
                type="submit"
                disabled={busy}
                className="text-sm px-3 py-1.5 rounded bg-brand-500 text-white hover:bg-brand-600 disabled:opacity-50"
              >
                {busy ? "Saving…" : "Save changes"}
              </button>
              {savedAt !== null && !busy && (
                <span className="text-xs text-neutral-500 dark:text-neutral-400">Saved.</span>
              )}
            </div>
          </form>
        )}
      </div>
    </div>
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
      <span className="block text-sm font-medium text-neutral-700 dark:text-neutral-300 mb-1">
        {label}
      </span>
      {children}
    </label>
  );
}
