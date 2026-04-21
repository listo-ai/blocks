import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";
import { MF_SHARED_SINGLETONS } from "@listo/block-ui-sdk/mf";

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));

function requireEnv(name: string): string {
  const v = process.env[name];
  if (!v) {
    throw new Error(
      `${name} is required to build this block — set it at build time, e.g.\n` +
        `  PUBLIC_AGENT_URL=http://localhost:8082 pnpm build`,
    );
  }
  return v;
}

export default defineConfig({
  plugins: [pluginReact()],

  source: {
    entry: { index: "./src/index.ts" },
    // `PUBLIC_AGENT_URL` is the agent the block's bundled AgentClient
    // connects to. Required — set it explicitly at build time; fail
    // loudly otherwise, don't silently default.
    define: {
      "import.meta.env.PUBLIC_AGENT_URL": JSON.stringify(
        requireEnv("PUBLIC_AGENT_URL"),
      ),
    },
  },

  output: {
    // Build into ../ui/ so block.yaml's `entry: ui/remoteEntry.js` keeps working.
    distPath: { root: "../ui" },
    // `auto` tells the MF runtime to resolve chunk URLs relative to where
    // `remoteEntry.js` actually loaded from. Without this the manifest bakes
    // in `publicPath: "/"`, so Studio (the host) then fetches the block's
    // chunks from Studio's own origin — 404s into its SPA fallback.
    assetPrefix: "auto",
  },

  tools: {
    postcss: {
      postcssOptions: {
        plugins: [require("@tailwindcss/postcss")],
      },
    },
    rspack: (_config, { appendPlugins }) => {
      const { ModuleFederationPlugin } = require("@module-federation/enhanced/rspack");

      appendPlugins(
        new ModuleFederationPlugin({
          // MF remote name — must match the dot-escaped block id convention:
          // com.listo.mqtt-client → com_listo_mqtt_client
          name: "com_listo_mqtt_client",
          filename: "remoteEntry.js",
          exposes: {
            // The only surface block authors need to know about.
            "./Panel": "./src/Panel.tsx",
          },
          // Shared singletons — same list as the Studio host so React,
          // zustand, react-query etc. are never duplicated at runtime.
          shared: MF_SHARED_SINGLETONS,
          dts: {
            generateTypes: { tsConfigPath: "./tsconfig.json" },
            outputDir: "../ui",
          },
        }),
      );
    },
  },
});
