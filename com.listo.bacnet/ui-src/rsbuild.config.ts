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
    distPath: { root: "../ui" },
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
          name: "com_listo_bacnet",
          filename: "remoteEntry.js",
          exposes: {
            "./Panel": "./src/Panel.tsx",
          },
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
