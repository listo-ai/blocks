/**
 * com.acme.hello — Panel
 *
 * Sidebar panel mounted by Studio via Module Federation.
 *
 * Uses only `@listo/block-ui-sdk` — never imports from `@listo/ui-core`
 * directly. If something is missing from the SDK, add it there.
 */
import { BlockShell } from "@listo/block-ui-sdk";
import { useNode, useSlot } from "@listo/block-ui-sdk";

const GREETER_KIND = "com.acme.hello.greeter";

interface Props {
  /** Graph path of the greeter node this panel is scoped to.
   *  Injected by the Studio sidebar when mounting the panel. */
  nodePath?: string;
}

export default function Panel({ nodePath = "/greeter-1" }: Props) {
  const node = useNode(nodePath);
  const outSlot = useSlot(nodePath, "out");

  if (node.isLoading) {
    return (
      <BlockShell title="Hello Block">
        <p className="text-sm text-muted-foreground">Loading…</p>
      </BlockShell>
    );
  }

  if (node.error || !node.data) {
    return (
      <BlockShell title="Hello Block">
        <p className="text-sm text-destructive">
          {node.error?.message ?? "Node not found"}
        </p>
      </BlockShell>
    );
  }

  const greeting =
    typeof outSlot?.value === "string"
      ? outSlot.value
      : "—";

  const generation = outSlot ? `gen ${outSlot.generation}` : null;

  return (
    <BlockShell title="Hello Block">
      <div className="space-y-3">
        {/* Node info */}
        <div className="space-y-1">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            Node
          </p>
          <p className="text-sm font-mono text-foreground">{node.data.path}</p>
          <p className="text-xs text-muted-foreground">
            Kind: <span className="font-mono">{GREETER_KIND}</span>
          </p>
        </div>

        {/* Latest greeting output */}
        <div className="space-y-1">
          <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            Last greeting
          </p>
          <p className="rounded bg-muted px-3 py-2 text-sm font-medium text-foreground">
            {greeting}
          </p>
          {generation && (
            <p className="text-xs text-muted-foreground">{generation}</p>
          )}
        </div>
      </div>
    </BlockShell>
  );
}
