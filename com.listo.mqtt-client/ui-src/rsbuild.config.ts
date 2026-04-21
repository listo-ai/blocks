import { createRequire } from "node:module";
import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";
import { createSharedSingletons } from "@listo/block-ui-sdk/mf";

const require = createRequire(import.meta.url);

// Block UI does NOT bake in `PUBLIC_AGENT_URL`. `@listo/ui-core` is a
// Module Federation shared singleton — at runtime the block panel
// consumes Studio's ui-core instance, which was built with the correct
// URL. A block standalone (no host) is not a supported mode.

export default defineConfig({
  plugins: [pluginReact()],

  source: {
    entry: { index: "./src/index.ts" },
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
          // Identical to Studio's host config so React, zustand,
          // react-query AND @listo/ui-core/ui-kit/etc. are never
          // duplicated at runtime. The factory resolves each workspace
          // package's real semver from its own package.json — MF never
          // sees a `workspace:*` specifier.
          shared: createSharedSingletons(),
          // Monorepo — types flow via workspace:* links; MF DTS zip
          // fetching is unused. See studio/rsbuild.config.ts for why.
          dts: false,
        }),
      );
    },
  },
});
