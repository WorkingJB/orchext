import { Theme, useTheme } from "./theme";

/// Settings → Appearance tab. Client-only preference (localStorage),
/// so it lives outside the org/account scoped settings and is
/// available to every workspace context, including local vaults.
export function AppearanceView() {
  const [theme, setTheme] = useTheme();
  const options: { value: Theme; label: string }[] = [
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
    { value: "system", label: "System" },
  ];
  return (
    <div className="p-6 max-w-xl space-y-6">
      <h2 className="text-lg font-semibold">Appearance</h2>
      <section className="space-y-3">
        <h3 className="text-sm font-medium">Theme</h3>
        <div
          role="radiogroup"
          aria-label="Theme"
          className="inline-flex rounded border border-neutral-300 dark:border-neutral-700 overflow-hidden"
        >
          {options.map((opt) => {
            const active = theme === opt.value;
            return (
              <button
                key={opt.value}
                type="button"
                role="radio"
                aria-checked={active}
                onClick={() => setTheme(opt.value)}
                className={
                  "px-3 py-1.5 text-sm transition " +
                  (active
                    ? "bg-brand-600 text-white"
                    : "bg-white text-neutral-700 hover:bg-neutral-100 dark:bg-neutral-900 dark:text-neutral-200 dark:hover:bg-neutral-800")
                }
              >
                {opt.label}
              </button>
            );
          })}
        </div>
        <p className="text-xs text-neutral-500 dark:text-neutral-400">
          System follows your operating system's appearance setting.
        </p>
      </section>
    </div>
  );
}
