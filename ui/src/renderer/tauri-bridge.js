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
    checkUpdate: () => safeInvoke("app_check_update"),
    installUpdate: (downloadUrl, expectedSize = null) =>
      safeInvoke("app_install_update", { downloadUrl, expectedSize }),
    cancelUpdate: () => safeInvoke("app_cancel_update"),
    openUrl: (url) => safeInvoke("app_open_url", { url }),
    getSettings: () => safeInvoke("app_settings_get"),
    setSettings: (patch) => safeInvoke("app_settings_set", { patch }),
    openDataDir: () => safeInvoke("app_open_data_dir"),
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
    readCustomRgb: (id, group) => safeInvoke("device_custom_rgb_read", { id, group }),
    writeCustomRgb: (id, group, rgb, activate = true) =>
      safeInvoke("device_custom_rgb_write", { id, group, rgb, activate }),
    startDirectRgb: (id, includeLogo = true) =>
      safeInvoke("studio_direct_start", { id, includeLogo }),
    stopDirectRgb: (id) => safeInvoke("studio_direct_stop", { id }),
    streamRgbFrame: (id, keysRgb, logoRgb = null) =>
      safeInvoke("studio_stream_frame", { id, keysRgb, logoRgb }),
    startNativeRgb: (id, config) =>
      safeInvoke("studio_native_start", { id, config }),
    updateNativeRgb: (config) =>
      safeInvoke("studio_native_update", { config }),
    stopNativeRgb: () => safeInvoke("studio_native_stop"),
    getStudioStatus: () => safeInvoke("studio_status"),
    setStudioSensors: (audio, ambilight, keys) =>
      safeInvoke("studio_sensors_set", { audio, ambilight, keys }),
    pollStudioTelemetry: () => safeInvoke("studio_telemetry"),
    startExternalRgbServer: (id, udpPort, httpPort, includeLogo = true) =>
      safeInvoke("studio_server_start", { id, udpPort, httpPort, includeLogo }),
    stopExternalRgbServer: () => safeInvoke("studio_server_stop"),
    exportStudioPreset: (name, content) => safeInvoke("studio_export_preset", { name, content }),
    importStudioPreset: () => safeInvoke("studio_import_preset"),
    hideToTray: () => safeInvoke("app_hide_to_tray"),
    onScreenProgress: (cb) => bindEvent("screen:progress", cb),
    onUpdateProgress: (cb) => bindEvent("update:progress", cb),
    onDeviceEvent: (cb) => bindEvent("device:event", cb),
    onStudioStopped: (cb) => bindEvent("studio:stopped", cb),
    onTrayHidden: (cb) => bindEvent("studio:tray-hidden", cb),
    onTrayShown: (cb) => bindEvent("studio:tray-shown", cb),
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
