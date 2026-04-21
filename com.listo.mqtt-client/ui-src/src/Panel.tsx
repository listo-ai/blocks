/**
 * com.listo.mqtt-client — sidebar panel.
 *
 * Lists every `com.listo.mqtt-client.client` node in the graph and
 * renders its settings form. Settings are persisted to each node's
 * `settings` slot via `AgentClient.slots.writeSlot`; the shared
 * `useNodeSettings` + `NodeSettingsForm` from `@listo/block-ui-sdk`
 * handles the JSON-Schema form + debounced save.
 */
import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  BlockShell,
  NodeSettingsForm,
  useAgentClient,
  useKinds,
  useNode,
  useNodeSettings,
  useSlot,
} from "@listo/block-ui-sdk";

const CLIENT_KIND = "com.listo.mqtt-client.client";

export default function Panel() {
  const { data: agent } = useAgentClient();
  const [selected, setSelected] = useState<string | null>(null);

  const clients = useQuery<Array<{ path: string }>>({
    enabled: !!agent,
    queryKey: ["mqtt-client", "clients"],
    queryFn: async () => {
      const page = await agent!.nodes.getNodesPage({
        filter: `kind==${CLIENT_KIND}`,
        size: 100,
      });
      return page.data;
    },
    // Refetch on focus so new clients added from the canvas show up
    // without a hard refresh.
    refetchOnWindowFocus: true,
  });

  return (
    <BlockShell title="MQTT Clients">
      {clients.isLoading && <Empty>Loading…</Empty>}
      {clients.error && (
        <Empty tone="error">
          {clients.error instanceof Error
            ? clients.error.message
            : "Failed to load clients"}
        </Empty>
      )}
      {clients.data && clients.data.length === 0 && (
        <Empty>
          No MQTT client nodes yet. Drop a{" "}
          <code className="font-mono">{CLIENT_KIND}</code> onto a flow, then
          come back here to configure it.
        </Empty>
      )}
      {clients.data && clients.data.length > 0 && (
        <div className="space-y-4">
          <ClientList
            items={clients.data.map((n) => ({
              path: n.path,
              label: n.path,
            }))}
            selected={selected}
            onSelect={setSelected}
          />
          {selected && <ClientEditor nodePath={selected} />}
        </div>
      )}
    </BlockShell>
  );
}

// ─── Sub-components ────────────────────────────────────────────────────────

function ClientList({
  items,
  selected,
  onSelect,
}: {
  items: { path: string; label: string }[];
  selected: string | null;
  onSelect: (path: string) => void;
}) {
  return (
    <ul className="space-y-1">
      {items.map((it) => {
        const active = it.path === selected;
        return (
          <li key={it.path}>
            <button
              type="button"
              onClick={() => onSelect(it.path)}
              className={
                "w-full rounded-md px-3 py-2 text-left text-sm font-mono transition " +
                (active
                  ? "bg-primary text-primary-foreground"
                  : "hover:bg-muted")
              }
            >
              {it.label}
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function ClientEditor({ nodePath }: { nodePath: string }) {
  const node = useNode(nodePath);
  const settingsSlot = useSlot(nodePath, "settings");
  const { data: agent } = useAgentClient();
  const kinds = useKinds();

  // Extract the settings JSON Schema from the client kind's manifest.
  const schema = useMemo(() => {
    const k = kinds.data?.find((x) => x.id === CLIENT_KIND);
    return (k?.settings_schema ?? { type: "object", properties: {} }) as Record<
      string,
      unknown
    >;
  }, [kinds.data]);

  const state = useNodeSettings(nodePath, settingsSlot, async (path, data) => {
    if (!agent) return;
    await agent.slots.writeSlot(path, "settings", data);
  });

  return (
    <div className="rounded-md border border-border p-3 space-y-3">
      <div>
        <div className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
          Node
        </div>
        <div className="text-sm font-mono">{nodePath}</div>
      </div>

      <NodeSettingsForm rawSchema={schema} {...state} />

      {node.error && (
        <p className="text-xs text-destructive">
          {node.error instanceof Error ? node.error.message : "error"}
        </p>
      )}
    </div>
  );
}

function Empty({
  children,
  tone = "muted",
}: {
  children: React.ReactNode;
  tone?: "muted" | "error";
}) {
  return (
    <p
      className={
        "text-sm " +
        (tone === "error" ? "text-destructive" : "text-muted-foreground")
      }
    >
      {children}
    </p>
  );
}
