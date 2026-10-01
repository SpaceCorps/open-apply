import { defineConfig } from "vite-plus";

// The site is served from https://spacecorps.github.io/open-apply/, so every asset path needs this prefix.
export default defineConfig({
  base: "/open-apply/",
  build: {
    target: "es2023",
  },
  fmt: {},
  lint: {
    jsPlugins: [{ name: "vite-plus", specifier: "vite-plus/oxlint-plugin" }],
    rules: { "vite-plus/prefer-vite-plus-imports": "error" },
    options: { typeAware: true, typeCheck: true },
  },
});
