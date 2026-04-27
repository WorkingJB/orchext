import { useEffect, useState } from "react";
import {
  api,
  MemberDetail,
  TeamMemberDetail,
  TeamSummary,
} from "./api";
import { Context } from "./OrgRail";

/// Teams admin / browser pane (Phase 3 platform Slice 2). Mirrors
/// the web's TeamsView surface — Slack-style two-pane: list of teams
/// on the left, detail (members + manager controls) on the right.
/// Available to every org member; admin-only affordances live inside.
export function TeamsView({ ctx }: { ctx: Context & { kind: "org" } }) {
  const isOrgAdmin = ctx.role === "owner" || ctx.role === "admin";

  const [teams, setTeams] = useState<TeamSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [showNew, setShowNew] = useState(false);
  const [newName, setNewName] = useState("");
  const [busy, setBusy] = useState(false);

  async function reload() {
    setError(null);
    try {
      const r = await api.teamsList(ctx.workspaceId, ctx.orgId);
      setTeams(r.teams);
      if (r.teams.length > 0) {
        setSelected((cur) => {
          if (cur && r.teams.some((t) => t.id === cur)) return cur;
          return r.teams[0].id;
        });
      } else {
        setSelected(null);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.orgId, ctx.workspaceId]);

  async function createTeam(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = newName.trim();
    if (!trimmed) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.teamCreate(ctx.workspaceId, ctx.orgId, trimmed);
      setNewName("");
      setShowNew(false);
      await reload();
      setSelected(created.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function deleteTeam(teamId: string) {
    if (!confirm("Delete this team? Members lose access to team docs.")) return;
    setBusy(true);
    setError(null);
    try {
      await api.teamDelete(ctx.workspaceId, ctx.orgId, teamId);
      await reload();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!teams && !error) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500">
        Loading teams…
      </div>
    );
  }

  return (
    <div className="h-full flex min-h-0">
      <aside className="w-64 shrink-0 border-r border-neutral-200 bg-white flex flex-col">
        <header className="px-3 py-2 border-b border-neutral-200 flex items-center justify-between">
          <h2 className="text-sm font-semibold text-neutral-700">Teams</h2>
          {isOrgAdmin && (
            <button
              type="button"
              onClick={() => setShowNew((v) => !v)}
              className="text-xs px-2 py-0.5 rounded bg-brand-500 text-white hover:bg-brand-600"
            >
              + New
            </button>
          )}
        </header>
        {showNew && isOrgAdmin && (
          <form
            onSubmit={createTeam}
            className="px-3 py-2 border-b border-neutral-200 space-y-2 bg-neutral-50"
          >
            <input
              autoFocus
              type="text"
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder="Team name"
              className="w-full border border-neutral-300 rounded px-2 py-1 text-sm"
              disabled={busy}
            />
            <div className="flex gap-2">
              <button
                type="submit"
                disabled={busy || newName.trim() === ""}
                className="text-xs px-2 py-1 rounded bg-brand-500 text-white hover:bg-brand-600 disabled:opacity-50"
              >
                Create
              </button>
              <button
                type="button"
                onClick={() => {
                  setShowNew(false);
                  setNewName("");
                }}
                className="text-xs px-2 py-1 rounded text-neutral-600 hover:bg-neutral-200"
              >
                Cancel
              </button>
            </div>
          </form>
        )}
        <ul className="flex-1 overflow-auto">
          {teams && teams.length === 0 && (
            <li className="px-3 py-4 text-xs text-neutral-500">
              No teams yet.
            </li>
          )}
          {teams?.map((t) => (
            <li key={t.id}>
              <button
                onClick={() => setSelected(t.id)}
                className={
                  "w-full text-left px-3 py-2 text-sm border-b border-neutral-100 transition " +
                  (selected === t.id
                    ? "bg-brand-50 text-brand-700"
                    : "hover:bg-neutral-50 text-neutral-700")
                }
              >
                <div className="font-medium">{t.name}</div>
                <div className="text-xs text-neutral-500 flex items-center gap-2">
                  <span>{t.member_count} members</span>
                  {t.viewer_role && (
                    <span className="px-1.5 rounded bg-neutral-100 text-neutral-600">
                      {t.viewer_role}
                    </span>
                  )}
                </div>
              </button>
            </li>
          ))}
        </ul>
        {error && (
          <div className="text-xs text-red-700 bg-red-50 border-t border-red-200 p-2">
            {error}
          </div>
        )}
      </aside>
      <div className="flex-1 min-w-0">
        {selected && teams ? (
          <TeamDetail
            ctx={ctx}
            team={teams.find((t) => t.id === selected) ?? null}
            isOrgAdmin={isOrgAdmin}
            onChanged={reload}
            onDeleted={() => deleteTeam(selected)}
          />
        ) : (
          <div className="h-full flex items-center justify-center text-neutral-500 text-sm">
            {teams && teams.length === 0
              ? "Create a team to start grouping members."
              : "Select a team."}
          </div>
        )}
      </div>
    </div>
  );
}

function TeamDetail({
  ctx,
  team,
  isOrgAdmin,
  onChanged,
  onDeleted,
}: {
  ctx: Context & { kind: "org" };
  team: TeamSummary | null;
  isOrgAdmin: boolean;
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const [members, setMembers] = useState<TeamMemberDetail[] | null>(null);
  const [orgMembers, setOrgMembers] = useState<MemberDetail[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [addAccountId, setAddAccountId] = useState<string>("");
  const [addRole, setAddRole] = useState<"manager" | "member">("member");
  const [renameOpen, setRenameOpen] = useState(false);
  const [renameValue, setRenameValue] = useState("");

  const isTeamManager = team?.viewer_role === "manager";
  const canManage = isOrgAdmin || isTeamManager;

  useEffect(() => {
    if (!team) {
      setMembers(null);
      return;
    }
    setRenameValue(team.name);
    let cancelled = false;
    (async () => {
      try {
        const [m, om] = await Promise.all([
          api.teamMembers(ctx.workspaceId, ctx.orgId, team.id),
          isOrgAdmin
            ? api.orgMembers(ctx.workspaceId, ctx.orgId)
            : Promise.resolve({ members: [] as MemberDetail[] }),
        ]);
        if (cancelled) return;
        setMembers(m.members);
        setOrgMembers(om.members);
        setError(null);
      } catch (e) {
        if (!cancelled) {
          setError(String(e));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [team?.id, ctx.orgId, ctx.workspaceId, isOrgAdmin]);

  if (!team) return null;

  async function add(e: React.FormEvent) {
    e.preventDefault();
    if (!addAccountId) return;
    setBusy(addAccountId);
    setError(null);
    try {
      await api.teamMemberAdd(
        ctx.workspaceId,
        ctx.orgId,
        team!.id,
        addAccountId,
        addRole
      );
      const m = await api.teamMembers(ctx.workspaceId, ctx.orgId, team!.id);
      setMembers(m.members);
      setAddAccountId("");
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function changeRole(
    accountId: string,
    role: "manager" | "member"
  ) {
    setBusy(accountId);
    setError(null);
    try {
      await api.teamMemberUpdate(
        ctx.workspaceId,
        ctx.orgId,
        team!.id,
        accountId,
        role
      );
      const m = await api.teamMembers(ctx.workspaceId, ctx.orgId, team!.id);
      setMembers(m.members);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function remove(accountId: string) {
    if (!confirm("Remove this member from the team?")) return;
    setBusy(accountId);
    setError(null);
    try {
      await api.teamMemberRemove(
        ctx.workspaceId,
        ctx.orgId,
        team!.id,
        accountId
      );
      const m = await api.teamMembers(ctx.workspaceId, ctx.orgId, team!.id);
      setMembers(m.members);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function rename(e: React.FormEvent) {
    e.preventDefault();
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === team!.name) {
      setRenameOpen(false);
      return;
    }
    setBusy("rename");
    setError(null);
    try {
      await api.teamUpdate(ctx.workspaceId, ctx.orgId, team!.id, {
        name: trimmed,
      });
      setRenameOpen(false);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  const memberIds = new Set(members?.map((m) => m.account_id) ?? []);
  const candidates =
    orgMembers?.filter((m) => !memberIds.has(m.account_id)) ?? [];

  return (
    <div className="h-full overflow-auto p-6">
      <div className="max-w-2xl mx-auto space-y-6">
        <header className="flex items-baseline justify-between">
          {renameOpen && canManage ? (
            <form onSubmit={rename} className="flex items-center gap-2 flex-1">
              <input
                autoFocus
                type="text"
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                className="border border-neutral-300 rounded px-2 py-1 text-lg font-semibold flex-1"
                disabled={busy === "rename"}
              />
              <button
                type="submit"
                disabled={busy === "rename"}
                className="text-xs px-2 py-1 rounded bg-brand-500 text-white"
              >
                Save
              </button>
              <button
                type="button"
                onClick={() => {
                  setRenameOpen(false);
                  setRenameValue(team.name);
                }}
                className="text-xs px-2 py-1 rounded text-neutral-600"
              >
                Cancel
              </button>
            </form>
          ) : (
            <>
              <div>
                <h1 className="text-xl font-semibold">{team.name}</h1>
                <p className="text-xs text-neutral-500">slug: {team.slug}</p>
              </div>
              <div className="flex gap-2">
                {canManage && (
                  <button
                    onClick={() => setRenameOpen(true)}
                    className="text-xs px-2 py-1 rounded text-neutral-600 hover:bg-neutral-100"
                  >
                    Rename
                  </button>
                )}
                {isOrgAdmin && (
                  <button
                    onClick={onDeleted}
                    className="text-xs px-2 py-1 rounded text-red-700 hover:bg-red-50"
                  >
                    Delete
                  </button>
                )}
              </div>
            </>
          )}
        </header>

        {error && (
          <div className="bg-red-50 border border-red-200 text-red-700 text-sm rounded-md p-3">
            {error}
          </div>
        )}

        <section className="bg-white border border-neutral-200 rounded-md p-5 space-y-4">
          <h2 className="text-sm font-semibold text-neutral-700">Members</h2>
          {!members && (
            <div className="text-sm text-neutral-500">Loading members…</div>
          )}
          {members && members.length === 0 && (
            <div className="text-sm text-neutral-500">No members yet.</div>
          )}
          {members && members.length > 0 && (
            <ul className="divide-y divide-neutral-100">
              {members.map((m) => (
                <li
                  key={m.account_id}
                  className="py-2 flex items-center gap-3"
                >
                  <div className="flex-1 min-w-0">
                    <div className="text-sm font-medium truncate">
                      {m.display_name || m.email}
                    </div>
                    <div className="text-xs text-neutral-500 truncate">
                      {m.email}
                    </div>
                  </div>
                  {canManage ? (
                    <select
                      value={m.role}
                      onChange={(e) =>
                        changeRole(
                          m.account_id,
                          e.target.value as "manager" | "member"
                        )
                      }
                      disabled={busy === m.account_id}
                      className="text-sm border border-neutral-300 rounded px-2 py-1"
                    >
                      <option value="manager">manager</option>
                      <option value="member">member</option>
                    </select>
                  ) : (
                    <span className="text-xs px-2 py-0.5 rounded bg-neutral-100 text-neutral-600">
                      {m.role}
                    </span>
                  )}
                  {canManage && (
                    <button
                      type="button"
                      onClick={() => remove(m.account_id)}
                      disabled={busy === m.account_id}
                      className="text-xs px-2 py-1 rounded text-red-700 hover:bg-red-50"
                    >
                      Remove
                    </button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </section>

        {isOrgAdmin && candidates.length > 0 && (
          <section className="bg-white border border-neutral-200 rounded-md p-5 space-y-3">
            <h2 className="text-sm font-semibold text-neutral-700">
              Add member
            </h2>
            <form onSubmit={add} className="flex flex-wrap items-center gap-2">
              <select
                value={addAccountId}
                onChange={(e) => setAddAccountId(e.target.value)}
                className="text-sm border border-neutral-300 rounded px-2 py-1 flex-1 min-w-[200px]"
              >
                <option value="">Select an org member…</option>
                {candidates.map((m) => (
                  <option key={m.account_id} value={m.account_id}>
                    {m.display_name || m.email} ({m.email})
                  </option>
                ))}
              </select>
              <select
                value={addRole}
                onChange={(e) =>
                  setAddRole(e.target.value as "manager" | "member")
                }
                className="text-sm border border-neutral-300 rounded px-2 py-1"
              >
                <option value="member">member</option>
                <option value="manager">manager</option>
              </select>
              <button
                type="submit"
                disabled={!addAccountId || busy !== null}
                className="text-sm px-3 py-1.5 rounded bg-brand-500 text-white hover:bg-brand-600 disabled:opacity-50"
              >
                Add
              </button>
            </form>
          </section>
        )}
      </div>
    </div>
  );
}
