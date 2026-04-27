import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, LogoData, Organization } from "./api";
import { Context } from "./OrgRail";

/// Org settings pane (Phase 3 platform Slice 1; logo upload added in
/// Slice 2). Admin/owner only — gated by Layout.
///
/// Logo: uploaded as a file (PNG / JPEG / GIF / WEBP, ≤ 512KB) via
/// the OS native file picker. Slice 1's external-URL field is gone
/// — external references didn't render reliably and the bytes-in-
/// Postgres path is simpler. The desktop renders the logo from a
/// data URL fetched via the `org_logo_get` Tauri command (since the
/// browser's `<img src>` can't attach the workspace bearer token).
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
  const [logoData, setLogoData] = useState<LogoData | null>(null);
  const [logoBusy, setLogoBusy] = useState<"uploading" | "removing" | null>(
    null
  );

  useEffect(() => {
    let cancelled = false;
    setOrg(null);
    setError(null);
    setLogoData(null);
    api
      .orgGet(ctx.workspaceId, ctx.orgId)
      .then((o) => {
        if (cancelled) return;
        setOrg(o);
        setName(o.name);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    api
      .orgLogoGet(ctx.workspaceId, ctx.orgId)
      .then((logo) => {
        if (!cancelled) setLogoData(logo);
      })
      .catch(() => {
        // 404 maps to null; non-404 errors are tolerable on the
        // settings pane (the user can still upload a replacement).
      });
    return () => {
      cancelled = true;
    };
  }, [ctx.orgId, ctx.workspaceId]);

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
      const updated = await api.orgUpdate(ctx.workspaceId, org.id, {
        name: trimmedName,
      });
      setOrg(updated);
      setSavedAt(Date.now());
      onUpdated(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function pickAndUploadLogo() {
    if (!org) return;
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }],
    });
    const path = typeof selected === "string" ? selected : null;
    if (!path) return;
    setLogoBusy("uploading");
    setError(null);
    try {
      const result = await api.orgLogoUpload(ctx.workspaceId, org.id, path);
      const updated = { ...org, logo_url: result.logo_url };
      setOrg(updated);
      onUpdated(updated);
      // Refetch the bytes so the preview updates.
      const fresh = await api.orgLogoGet(ctx.workspaceId, org.id);
      setLogoData(fresh);
    } catch (e) {
      setError(String(e));
    } finally {
      setLogoBusy(null);
    }
  }

  async function removeLogo() {
    if (!org) return;
    if (!confirm("Remove the organization logo?")) return;
    setLogoBusy("removing");
    setError(null);
    try {
      await api.orgLogoDelete(ctx.workspaceId, org.id);
      const updated = { ...org, logo_url: null };
      setOrg(updated);
      setLogoData(null);
      onUpdated(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setLogoBusy(null);
    }
  }

  if (!org && !error) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500">
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
          <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-md p-3">
            {error}
          </div>
        )}

        {org && (
          <form
            onSubmit={save}
            className="bg-white border border-neutral-200 rounded-md p-5 space-y-4"
          >
            <Field label="Name">
              <input
                type="text"
                value={name}
                onChange={(e) => setName(e.target.value)}
                className="w-full border border-neutral-300 rounded px-3 py-2 text-sm"
                disabled={busy}
              />
            </Field>

            <Field label="Logo">
              <div className="flex items-center gap-3">
                <div className="w-14 h-14 rounded-md border border-neutral-200 bg-neutral-50 overflow-hidden flex items-center justify-center text-xs text-neutral-400">
                  {logoData ? (
                    <img
                      src={logoData.data_url}
                      alt=""
                      className="w-full h-full object-cover"
                    />
                  ) : (
                    "—"
                  )}
                </div>
                <div className="flex flex-col gap-1.5">
                  <button
                    type="button"
                    onClick={pickAndUploadLogo}
                    disabled={logoBusy !== null}
                    className="text-xs px-2 py-1 rounded bg-brand-500 text-white hover:bg-brand-600 disabled:opacity-50 self-start"
                  >
                    {logoBusy === "uploading" ? "Uploading…" : "Choose file…"}
                  </button>
                  {logoData && (
                    <button
                      type="button"
                      onClick={removeLogo}
                      disabled={logoBusy !== null}
                      className="text-xs text-red-700 hover:underline self-start disabled:opacity-50"
                    >
                      {logoBusy === "removing" ? "Removing…" : "Remove logo"}
                    </button>
                  )}
                </div>
              </div>
              <p className="text-xs text-neutral-500 mt-2">
                PNG, JPEG, GIF, or WEBP up to 512KB. Shown as the
                org&apos;s avatar in the left rail.
              </p>
            </Field>

            <Field label="Allowed domains">
              <input
                type="text"
                value={renderAllowedDomains(org.allowed_domains)}
                disabled
                className="w-full border border-neutral-200 rounded px-3 py-2 text-sm bg-neutral-50 text-neutral-500"
              />
              <p className="text-xs text-neutral-500 mt-1">
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
                <span className="text-xs text-neutral-500">Saved.</span>
              )}
            </div>
          </form>
        )}
      </div>
    </div>
  );
}

function renderAllowedDomains(value: unknown): string {
  if (Array.isArray(value)) return value.join(", ");
  return "";
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
      <span className="block text-sm font-medium text-neutral-700 mb-1">
        {label}
      </span>
      {children}
    </label>
  );
}
