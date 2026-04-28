import { useCallback, useEffect, useMemo, useState } from "react";
import { api, VaultInfo } from "./api";
import { Context, OrgRail } from "./OrgRail";
import { OnboardingView } from "./OnboardingView";
import { DocumentsTab } from "./DocumentsTab";
import { SettingsView } from "./SettingsView";

type View = "documents" | "settings" | "onboarding";

export function Layout({
  activeVault,
  contexts,
  onSwitched,
  onAdd,
  onRefresh,
}: {
  activeVault: VaultInfo;
  contexts: Context[];
  onSwitched: (v: VaultInfo) => void;
  onAdd: () => void;
  onRefresh: () => Promise<void>;
}) {
  const activeCtx = useMemo<Context | null>(() => {
    return (
      contexts.find((c) => c.workspaceId === activeVault.workspace_id) ?? null
    );
  }, [contexts, activeVault.workspace_id]);

  // Auto-open onboarding on first-run (empty vault).
  const [view, setView] = useState<View>(
    activeVault.document_count === 0 ? "onboarding" : "documents"
  );
  /// Optional doc-id filter for the Proposals sub-tab. Set when the
  /// inline banner on a doc detail asks "Review proposals" so the
  /// user lands directly on that doc's pending proposals; cleared on
  /// context switch or sub-tab navigation.
  const [proposalsFocus, setProposalsFocus] = useState<string | null>(null);

  // When the workspace switches, reset the visible view. Onboarding
  // auto-opens only on truly empty vaults.
  useEffect(() => {
    setView(activeVault.document_count === 0 ? "onboarding" : "documents");
    setProposalsFocus(null);
  }, [activeVault.workspace_id, activeVault.document_count]);

  const switchToWorkspace = useCallback(
    async (workspaceId: string) => {
      if (workspaceId === activeVault.workspace_id) return;
      try {
        const info = await api.workspaceActivate(workspaceId);
        onSwitched(info);
      } catch (e) {
        console.warn("activate failed", e);
      }
    },
    [activeVault.workspace_id, onSwitched]
  );

  const onboardingActive = view === "onboarding";

  return (
    <div className="h-full flex flex-col">
      <header className="border-b border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 px-4 h-12 flex items-center gap-3">
        <span className="font-semibold">Orchext</span>
        <span className="text-neutral-400 dark:text-neutral-500">·</span>
        <span className="text-sm text-neutral-700 dark:text-neutral-300">
          {activeCtx ? badgeLabel(activeCtx) : activeVault.name}
        </span>
        <span className="ml-auto text-xs text-neutral-500 dark:text-neutral-400 font-mono truncate max-w-[40ch]">
          {activeVault.root}
        </span>
      </header>
      <div className="flex flex-1 min-h-0">
        <OrgRail
          contexts={contexts}
          activeWorkspaceId={activeVault.workspace_id}
          onSelect={(ctx) => void switchToWorkspace(ctx.workspaceId)}
          onAdd={onAdd}
        />
        {!onboardingActive && (
          <nav className="w-44 border-r border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-2 flex flex-col gap-1">
            <NavBtn
              label="Documents"
              active={view === "documents"}
              onClick={() => setView("documents")}
            />
            <NavBtn
              label="Settings"
              active={view === "settings"}
              onClick={() => setView("settings")}
            />
          </nav>
        )}
        <main key={activeVault.workspace_id} className="flex-1 min-w-0 bg-neutral-50 dark:bg-neutral-900">
          {view === "onboarding" && (
            <OnboardingView
              onComplete={async () => {
                await onRefresh();
                setView("documents");
              }}
            />
          )}
          {view === "documents" && activeCtx && (
            <DocumentsTab
              ctx={activeCtx}
              proposalsFocus={proposalsFocus}
              onSetProposalsFocus={setProposalsFocus}
              onMutated={onRefresh}
            />
          )}
          {view === "settings" && activeCtx && (
            <SettingsView
              ctx={activeCtx}
              onMutated={onRefresh}
              onOrgUpdated={() => void onRefresh()}
            />
          )}
        </main>
      </div>
    </div>
  );
}

function badgeLabel(ctx: Context): string {
  switch (ctx.kind) {
    case "local":
      return ctx.name;
    case "personal":
      return "Personal";
    case "org":
      return ctx.name;
  }
}

function NavBtn({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={
        "text-left px-3 py-2 rounded-md text-sm transition " +
        (active
          ? "bg-brand-50 dark:bg-brand-700/20 text-brand-700 dark:text-brand-500 font-medium"
          : "text-neutral-700 dark:text-neutral-300 hover:bg-neutral-100 dark:hover:bg-neutral-800")
      }
    >
      {label}
    </button>
  );
}
