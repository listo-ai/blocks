/**
 * com.listo.bacnet — sidebar panel.
 *
 * Lists every `com.listo.bacnet.driver` node in the graph and
 * renders its settings form. Settings are persisted to each node's
 * `settings` slot via the shared `useNodeSettings` + `NodeSettingsForm`
 * helpers from `@listo/block-ui-sdk`.
 */
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BlockShell,
  NodeSettingsForm,
  useAgentClient,
  useNodeSettings,
} from "@listo/block-ui-sdk";

const DRIVER_KIND = "com.listo.bacnet.driver";

export default function Panel() {
  const { data: agent } = useAgentClient();
  const [selected, setSelected] = useState<string | null>(null);

  const devices = useQuery<Array<{ path: string }>>({
    enabled: !!agent,
    queryKey: ["bacnet", "drivers"],
    queryFn: async () => {
      const page = await agent!.nodes.getNodesPage({
        filter: `kind==${DRIVER_KIND}`,
        size: 100,
      });
      return page.data;
    },
    refetchOnWindowFocus: true,
  });

  return (
    <BlockShell title="BACnet Drivers">
      {devices.isLoading && <Empty>Loading…</Empty>}
      {devices.error && (
        <Empty tone="error">
          {devices.error instanceof Error
            ? devices.error.message
            : "Failed to load devices"}
        </Empty>
      )}
      {devices.data && devices.data.length === 0 && (
        <Empty>
          No BACnet driver nodes yet. Drop a{" "}
          <code className="font-mono">{DRIVER_KIND}</code> onto a flow, then
          come back here to configure it.
        </Empty>
      )}
      {devices.data && devices.data.length > 0 && (
        <div className="space-y-4">
          <DriverList
            items={devices.data.map((n) => ({ path: n.path, label: n.path }))}
            selected={selected}
            onSelect={setSelected}
          />
          {selected && <DriverEditor nodePath={selected} />}
        </div>
      )}
    </BlockShell>
  );
}

// ─── Sub-components ────────────────────────────────────────────────────────

function DriverList({
  items,
  selected,
  onSelect,
}: {
  items: Array<{ path: string; label: string }>;
  selected: string | null;
  onSelect: (path: string) => void;
}) {
  return (
    <ul className="space-y-1">
      {items.map((item) => (
        <li key={item.path}>
          <button
            type="button"
            onClick={() => onSelect(item.path)}
            className={[
              "w-full text-left rounded px-3 py-2 text-sm font-mono truncate",
              selected === item.path
                ? "bg-accent text-accent-foreground"
                : "hover:bg-muted",
            ].join(" ")}
          >
            {item.label}
          </button>
        </li>
      ))}
    </ul>
  );
}

function DriverEditor({ nodePath }: { nodePath: string }) {
  const { schema, settings, save, isSaving } = useNodeSettings(nodePath);

  if (!schema) return <Empty>Loading settings…</Empty>;

  return (
    <div className="rounded border p-4 space-y-3">
      <h3 className="text-sm font-semibold truncate">{nodePath}</h3>
      <NodeSettingsForm
        schema={schema}
        value={settings}
        onChange={save}
        disabled={isSaving}
      />
    </div>
  );
}

function Empty({
  children,
  tone,
}: {
  children: React.ReactNode;
  tone?: "error";
}) {
  return (
    <p
      className={[
        "text-sm px-3 py-4",
        tone === "error" ? "text-destructive" : "text-muted-foreground",
      ].join(" ")}
    >
      {children}
    </p>
  );
}
