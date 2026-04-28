import { useCallback, useEffect, useState } from "react";
import { api, OrgsListResponse, VaultInfo, WorkspaceInfo } from "./api";
import { VaultPicker, PendingApprovalInfo } from "./VaultPicker";
import { AwaitingApprovalView } from "./AwaitingApprovalView";
import { Layout } from "./Layout";
import { buildContexts, Context, OrgsByServer } from "./OrgRail";

type AppShellState =
  | { kind: "bootstrapping" }
  | { kind: "first_run" }
  | { kind: "adding" }
  | { kind: "pending_approval"; info: PendingApprovalInfo }
  | {
      kind: "ready";
      workspaces: WorkspaceInfo[];
      activeVault: VaultInfo;
      contexts: Context[];
      orgsByServer: OrgsByServer;
    };

export default function App() {
  const [state, setState] = useState<AppShellState>({ kind: "bootstrapping" });

  const refresh = useCallback(async () => {
    const workspaces = await api.workspaceList();
    if (workspaces.length === 0) {
      setState({ kind: "first_run" });
      return;
    }

    // Pull each unique remote server's org list so the rail can route
    // tenants to org metadata (logo, role). One workspace per server
    // is enough — they share the same session token.
    const seenServers = new Set<string>();
    const orgFetches: Promise<[string, OrgsListResponse | null]>[] = [];
    for (const w of workspaces) {
      if (w.kind !== "remote" || !w.server_url) continue;
      if (seenServers.has(w.server_url)) continue;
      seenServers.add(w.server_url);
      orgFetches.push(
        api
          .orgsList(w.id)
          .then((r): [string, OrgsListResponse] => [w.server_url!, r])
          .catch((): [string, null] => [w.server_url!, null])
      );
    }
    const orgsByServer: OrgsByServer = new Map();
    for (const [serverUrl, resp] of await Promise.all(orgFetches)) {
      if (resp) orgsByServer.set(serverUrl, resp);
    }

    const contexts = buildContexts(workspaces, orgsByServer);

    // Hydrate org logo data URLs in parallel. Failures are non-fatal
    // — the rail falls back to initials if the fetch flops.
    await Promise.all(
      contexts
        .filter(
          (c): c is Context & { kind: "org" } =>
            c.kind === "org" && c.logoUrl !== null
        )
        .map(async (c) => {
          try {
            const logo = await api.orgLogoGet(c.workspaceId, c.orgId);
            c.logoData = logo?.data_url ?? null;
          } catch {
            c.logoData = null;
          }
        })
    );

    // Open / refresh the active vault. `vault_info` auto-opens the
    // active registered workspace if one exists.
    const activeVault = await api.vaultInfo();
    if (!activeVault) {
      // Edge case: workspaces exist but none active. Activate the
      // first one so the rail has a target.
      const first = workspaces[0];
      const activated = await api.workspaceActivate(first.id);
      setState({
        kind: "ready",
        workspaces,
        activeVault: activated,
        contexts,
        orgsByServer,
      });
      return;
    }
    setState({
      kind: "ready",
      workspaces,
      activeVault,
      contexts,
      orgsByServer,
    });
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  if (state.kind === "bootstrapping") {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500 dark:text-neutral-400">
        Loading…
      </div>
    );
  }

  if (state.kind === "first_run" || state.kind === "adding") {
    return (
      <VaultPicker
        onOpened={() => {
          void refresh();
        }}
        onPending={(info) => setState({ kind: "pending_approval", info })}
        onCancel={
          state.kind === "adding" ? () => void refresh() : undefined
        }
      />
    );
  }

  if (state.kind === "pending_approval") {
    return (
      <AwaitingApprovalView
        pending={state.info.pending}
        email={state.info.accountEmail}
        // "Sign out" on awaiting-approval is just "drop the transient
        // login state". No server-side session to revoke yet because
        // we never registered a workspace; the bearer was never
        // persisted past this branch. Dropping back to the regular
        // shell (or first-run) is the right shape.
        onSignOut={() => void refresh()}
      />
    );
  }

  return (
    <Layout
      activeVault={state.activeVault}
      contexts={state.contexts}
      onSwitched={(vault) => {
        setState((prev) =>
          prev.kind === "ready" ? { ...prev, activeVault: vault } : prev
        );
      }}
      onAdd={() => setState({ kind: "adding" })}
      onRefresh={refresh}
    />
  );
}
