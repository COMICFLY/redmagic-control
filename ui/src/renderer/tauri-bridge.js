/* Tauri v2 桥接层：将 Rust 后端命令与事件映射为与原前端完全一致的 window.rm 契约 */
(function () {
  if (window.rm) return;
  const tauri = window.__TAURI__;
  if (!tauri || !tauri.core) return;

  const invoke = tauri.core.invoke;
  const listen = tauri.event && tauri.event.listen;
  const getCurrentWindow = tauri.window && tauri.window.getCurrentWindow;

  async function safeInvoke(cmd, args) {
    try {
      const data = await invoke(cmd, args || {});
      return { ok: true, data };
    } catch (err) {
      return { ok: false, error: typeof err === "string" ? err : (err && err.message) || String(err) };
    }
  }

  function bindEvent(eventName, cb) {
    if (!listen || typeof cb !== "function") return () => {};
    let unlistenFn = null;
    let disposed = false;
    listen(eventName, (evt) => {
      if (!disposed) cb(evt.payload);
    }).then((fn) => {
      if (disposed) fn();
      else unlistenFn = fn;
    }).catch(() => {});
    return () => {
      disposed = true;
      if (unlistenFn) unlistenFn();
    };
  }

  window.rm = {
    getMeta: () => safeInvoke("app_meta"),
    listDevices: () => safeInvoke("device_list"),
    rescanDevices: () => safeInvoke("device_rescan"),
    wakeDevice: (id) => safeInvoke("device_wake", { id }),
    switchLink: (id, linkId) => safeInvoke("device_switch_link", { id, linkId }),
    readState: (id) => safeInvoke("device_state", { id }),
    readBattery: (id) => safeInvoke("device_battery", { id }),
    writeConfig: (id, patch) => safeInvoke("device_config_write", { id, patch }),
    writeKey: (id, slot, hex) => safeInvoke("device_key_write", { id, slot, hex }),
    resetKey: (id, slot) => safeInvoke("device_key_reset", { id, slot }),
    resetAllKeys: (id) => safeInvoke("device_key_reset_all", { id }),
    uploadScreenImage: (id, base64Rgb565, opts) =>
      safeInvoke("device_screen_upload", { id, base64Rgb565, opts: opts || {} }),
    saveScreenToLibrary: (id, base64Rgb565, name) =>
      safeInvoke("device_screen_lib_save", { id, base64Rgb565, name: name || "" }),
    deleteScreenLibraryItem: (id, uuid) =>
      safeInvoke("device_screen_lib_delete", { id, uuid }),
    pickScreenFile: () => safeInvoke("screen_pick_file"),
    pasteScreenClipboard: () => safeInvoke("screen_paste_clipboard"),
    onScreenProgress: (cb) => bindEvent("screen:progress", cb),
    onDeviceEvent: (cb) => bindEvent("device:event", cb),
    win: {
      minimize: () => getCurrentWindow && getCurrentWindow().minimize(),
      toggleMax: async () => {
        if (!getCurrentWindow) return false;
        const w = getCurrentWindow();
        await w.toggleMaximize();
        return w.isMaximized();
      },
      close: () => getCurrentWindow && getCurrentWindow().close(),
    },
  };
})();
