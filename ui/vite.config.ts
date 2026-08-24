import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the bundle over the `tauri://` custom protocol, which sends no
// CORS headers. Vite stamps `crossorigin` on its module tags and a crossorigin
// module fetch against an opaque origin is blocked, leaving a blank window.
function stripCrossorigin(): Plugin {
  return {
    name: "strip-crossorigin",
    enforce: "post",
    transformIndexHtml(html) {
      return html.replace(/\s+crossorigin/g, "");
    },
  };
}

export default defineConfig({
  plugins: [react(), stripCrossorigin()],
  clearScreen: false,
  server: { port: 1421, strictPort: true },
  build: { target: "safari15" },
});
