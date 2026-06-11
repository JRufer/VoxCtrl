// vite.config.ts
import { defineConfig } from "file:///home/jrufer/Development/VoxCtrl/node_modules/vite/dist/node/index.js";
import { svelte } from "file:///home/jrufer/Development/VoxCtrl/node_modules/@sveltejs/vite-plugin-svelte/src/index.js";
import tailwindcss from "file:///home/jrufer/Development/VoxCtrl/node_modules/@tailwindcss/vite/dist/index.mjs";
var host = process.env.TAURI_DEV_HOST;
var vite_config_default = defineConfig({
  plugins: [svelte(), tailwindcss()],
  // Vite options tailored for Tauri development
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host ? {
      protocol: "ws",
      host,
      port: 5183
    } : void 0,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"]
    }
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    target: process.env.TAURI_ENV_PLATFORM == "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    outDir: "dist"
  }
});
export {
  vite_config_default as default
};
//# sourceMappingURL=data:application/json;base64,ewogICJ2ZXJzaW9uIjogMywKICAic291cmNlcyI6IFsidml0ZS5jb25maWcudHMiXSwKICAic291cmNlc0NvbnRlbnQiOiBbImNvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9kaXJuYW1lID0gXCIvaG9tZS9qcnVmZXIvRGV2ZWxvcG1lbnQvVm94Q3RybFwiO2NvbnN0IF9fdml0ZV9pbmplY3RlZF9vcmlnaW5hbF9maWxlbmFtZSA9IFwiL2hvbWUvanJ1ZmVyL0RldmVsb3BtZW50L1ZveEN0cmwvdml0ZS5jb25maWcudHNcIjtjb25zdCBfX3ZpdGVfaW5qZWN0ZWRfb3JpZ2luYWxfaW1wb3J0X21ldGFfdXJsID0gXCJmaWxlOi8vL2hvbWUvanJ1ZmVyL0RldmVsb3BtZW50L1ZveEN0cmwvdml0ZS5jb25maWcudHNcIjtpbXBvcnQgeyBkZWZpbmVDb25maWcgfSBmcm9tIFwidml0ZVwiO1xuaW1wb3J0IHsgc3ZlbHRlIH0gZnJvbSBcIkBzdmVsdGVqcy92aXRlLXBsdWdpbi1zdmVsdGVcIjtcbmltcG9ydCB0YWlsd2luZGNzcyBmcm9tIFwiQHRhaWx3aW5kY3NzL3ZpdGVcIjtcblxuY29uc3QgaG9zdCA9IHByb2Nlc3MuZW52LlRBVVJJX0RFVl9IT1NUO1xuXG5leHBvcnQgZGVmYXVsdCBkZWZpbmVDb25maWcoe1xuICBwbHVnaW5zOiBbc3ZlbHRlKCksIHRhaWx3aW5kY3NzKCldLFxuICAvLyBWaXRlIG9wdGlvbnMgdGFpbG9yZWQgZm9yIFRhdXJpIGRldmVsb3BtZW50XG4gIGNsZWFyU2NyZWVuOiBmYWxzZSxcbiAgc2VydmVyOiB7XG4gICAgcG9ydDogNTE3MyxcbiAgICBzdHJpY3RQb3J0OiB0cnVlLFxuICAgIGhvc3Q6IGhvc3QgfHwgZmFsc2UsXG4gICAgaG1yOiBob3N0XG4gICAgICA/IHtcbiAgICAgICAgICBwcm90b2NvbDogXCJ3c1wiLFxuICAgICAgICAgIGhvc3QsXG4gICAgICAgICAgcG9ydDogNTE4MyxcbiAgICAgICAgfVxuICAgICAgOiB1bmRlZmluZWQsXG4gICAgd2F0Y2g6IHtcbiAgICAgIC8vIDMuIHRlbGwgdml0ZSB0byBpZ25vcmUgd2F0Y2hpbmcgYHNyYy10YXVyaWBcbiAgICAgIGlnbm9yZWQ6IFtcIioqL3NyYy10YXVyaS8qKlwiXSxcbiAgICB9LFxuICB9LFxuICBlbnZQcmVmaXg6IFtcIlZJVEVfXCIsIFwiVEFVUklfRU5WXypcIl0sXG4gIGJ1aWxkOiB7XG4gICAgdGFyZ2V0OlxuICAgICAgcHJvY2Vzcy5lbnYuVEFVUklfRU5WX1BMQVRGT1JNID09IFwid2luZG93c1wiXG4gICAgICAgID8gXCJjaHJvbWUxMDVcIlxuICAgICAgICA6IFwic2FmYXJpMTNcIixcbiAgICBtaW5pZnk6ICFwcm9jZXNzLmVudi5UQVVSSV9FTlZfREVCVUcgPyBcImVzYnVpbGRcIiA6IGZhbHNlLFxuICAgIHNvdXJjZW1hcDogISFwcm9jZXNzLmVudi5UQVVSSV9FTlZfREVCVUcsXG4gICAgb3V0RGlyOiBcImRpc3RcIixcbiAgfSxcbn0pO1xuIl0sCiAgIm1hcHBpbmdzIjogIjtBQUFrUixTQUFTLG9CQUFvQjtBQUMvUyxTQUFTLGNBQWM7QUFDdkIsT0FBTyxpQkFBaUI7QUFFeEIsSUFBTSxPQUFPLFFBQVEsSUFBSTtBQUV6QixJQUFPLHNCQUFRLGFBQWE7QUFBQSxFQUMxQixTQUFTLENBQUMsT0FBTyxHQUFHLFlBQVksQ0FBQztBQUFBO0FBQUEsRUFFakMsYUFBYTtBQUFBLEVBQ2IsUUFBUTtBQUFBLElBQ04sTUFBTTtBQUFBLElBQ04sWUFBWTtBQUFBLElBQ1osTUFBTSxRQUFRO0FBQUEsSUFDZCxLQUFLLE9BQ0Q7QUFBQSxNQUNFLFVBQVU7QUFBQSxNQUNWO0FBQUEsTUFDQSxNQUFNO0FBQUEsSUFDUixJQUNBO0FBQUEsSUFDSixPQUFPO0FBQUE7QUFBQSxNQUVMLFNBQVMsQ0FBQyxpQkFBaUI7QUFBQSxJQUM3QjtBQUFBLEVBQ0Y7QUFBQSxFQUNBLFdBQVcsQ0FBQyxTQUFTLGFBQWE7QUFBQSxFQUNsQyxPQUFPO0FBQUEsSUFDTCxRQUNFLFFBQVEsSUFBSSxzQkFBc0IsWUFDOUIsY0FDQTtBQUFBLElBQ04sUUFBUSxDQUFDLFFBQVEsSUFBSSxrQkFBa0IsWUFBWTtBQUFBLElBQ25ELFdBQVcsQ0FBQyxDQUFDLFFBQVEsSUFBSTtBQUFBLElBQ3pCLFFBQVE7QUFBQSxFQUNWO0FBQ0YsQ0FBQzsiLAogICJuYW1lcyI6IFtdCn0K
