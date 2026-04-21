import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "@rsbuild/core";
import { pluginReact } from "@rsbuild/plugin-react";
import { MF_SHARED_SINGLETONS } from "@listo/block-ui-sdk/mf";

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));

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
          // com.acme.hello → com_acme_hello
          name: "com_acme_hello",
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
