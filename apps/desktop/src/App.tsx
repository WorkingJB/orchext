import { useEffect, useState } from "react";
import { api, VaultInfo } from "./api";
import { VaultPicker } from "./VaultPicker";
import { Layout } from "./Layout";

export default function App() {
  const [vault, setVault] = useState<VaultInfo | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .vaultInfo()
      .then((v) => setVault(v))
      .finally(() => setLoading(false));
  }, []);

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center text-neutral-500">
        Loading…
      </div>
    );
  }

  if (!vault) {
    return <VaultPicker onOpened={setVault} />;
  }

  return <Layout vault={vault} onSwitch={() => setVault(null)} />;
}
