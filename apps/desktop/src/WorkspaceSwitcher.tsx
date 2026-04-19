import { useEffect, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api, VaultInfo, WorkspaceInfo } from "./api";

/// Dropdown in the header showing the active workspace + a menu to
/// switch, add, rename, or remove. Always sourced from `workspace_list`
/// so the UI reflects on-disk registry state.
export function WorkspaceSwitcher({
  active,
  onSwitched,
}: {
  active: VaultInfo;
  onSwitched: (v: VaultInfo) => void;
}) {
  const [open, setOpen] = useState(false);
  const [workspaces, setWorkspaces] = useState<WorkspaceInfo[]>([]);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const rootRef = useRef<HTMLDivElement | null>(null);

  async function refresh() {
    try {
      const list = await api.workspaceList();
      setWorkspaces(list);
    } catch (e) {
      setErr(String(e));
    }
  }

  useEffect(() => {
    if (open) {
      void refresh();
    }
  }, [open]);

  // Close on outside click.
  useEffect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    }
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  async function switchTo(id: string) {
    if (id === active.workspace_id) {
      setOpen(false);
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      const info = await api.workspaceActivate(id);
      onSwitched(info);
      setOpen(false);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function addWorkspace() {
    setErr(null);
    const selected = await pickDirectory();
    if (!selected) return;
    setBusy(true);
    try {
      const info = await api.workspaceAdd(selected);
      onSwitched(info);
      setOpen(false);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function removeWorkspace(id: string) {
    if (workspaces.length <= 1) {
      setErr("Cannot remove the last workspace.");
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api.workspaceRemove(id);
      // If we removed the active workspace, the backend promoted the
      // first remaining one; reflect that by refetching active info.
      if (id === active.workspace_id) {
        const info = await api.vaultInfo();
        if (info) onSwitched(info);
      } else {
        await refresh();
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  function startRename(id: string, current: string) {
    setRenaming(id);
    setRenameValue(current);
  }

  async function commitRename(id: string) {
    const name = renameValue.trim();
    if (!name) {
      setRenaming(null);
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api.workspaceRename(id, name);
      await refresh();
      // If it was the active one, refresh displayed name.
      if (id === active.workspace_id) {
        const info = await api.vaultInfo();
        if (info) onSwitched(info);
      }
    } catch (e) {
      setErr(String(e));
    } finally {
      setRenaming(null);
      setBusy(false);
    }
  }

  return (
    <div ref={rootRef} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-2 text-left px-2 py-1 rounded hover:bg-neutral-100 transition"
      >
        <span className="font-medium text-sm">{active.name}</span>
        <span className="text-neutral-400">·</span>
        <span className="text-xs text-neutral-500 font-mono truncate max-w-[32ch]">
          {active.root}
        </span>
        <span className="text-neutral-400 text-xs">▾</span>
      </button>

      {open && (
        <div className="absolute left-0 top-full mt-1 w-96 bg-white border border-neutral-200 rounded-lg shadow-lg z-30 py-1">
          <div className="px-3 py-2 text-xs uppercase tracking-wide text-neutral-500">
            Workspaces
          </div>
          {workspaces.length === 0 && (
            <div className="px-3 py-2 text-sm text-neutral-500">
              Loading…
            </div>
          )}
          {workspaces.map((w) => (
            <div
              key={w.id}
              className={
                "group flex items-center gap-2 px-3 py-2 text-sm " +
                (w.active
                  ? "bg-brand-50 text-brand-700"
                  : "hover:bg-neutral-50")
              }
            >
              <button
                onClick={() => switchTo(w.id)}
                disabled={busy}
                className="flex-1 text-left min-w-0"
              >
                {renaming === w.id ? (
                  <input
                    autoFocus
                    value={renameValue}
                    onChange={(e) => setRenameValue(e.target.value)}
                    onBlur={() => commitRename(w.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") commitRename(w.id);
                      if (e.key === "Escape") setRenaming(null);
                    }}
                    className="w-full px-1 py-0.5 border border-neutral-300 rounded text-sm"
                  />
                ) : (
                  <>
                    <div className="font-medium truncate">{w.name}</div>
                    <div className="text-xs text-neutral-500 font-mono truncate">
                      {w.path}
                    </div>
                  </>
                )}
              </button>
              {renaming !== w.id && (
                <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition">
                  <button
                    onClick={() => startRename(w.id, w.name)}
                    className="text-xs text-neutral-500 hover:text-neutral-900 px-1"
                    title="Rename"
                  >
                    Rename
                  </button>
                  <button
                    onClick={() => removeWorkspace(w.id)}
                    className="text-xs text-red-600 hover:text-red-800 px-1"
                    title="Remove from registry (does not delete files)"
                    disabled={workspaces.length <= 1}
                  >
                    Remove
                  </button>
                </div>
              )}
            </div>
          ))}
          <div className="border-t border-neutral-100 my-1" />
          <button
            onClick={addWorkspace}
            disabled={busy}
            className="w-full text-left px-3 py-2 text-sm hover:bg-neutral-50 disabled:opacity-50"
          >
            + Add workspace…
          </button>
          {err && (
            <div className="mx-3 my-2 p-2 bg-red-50 text-red-700 text-xs rounded border border-red-200">
              {err}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

async function pickDirectory(): Promise<string | null> {
  const selected = await openDialog({ directory: true, multiple: false });
  if (!selected || typeof selected !== "string") return null;
  return selected;
}
