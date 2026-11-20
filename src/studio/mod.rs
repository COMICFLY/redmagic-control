//! 灯效编程工作室原生后端引擎：
//! 统一协调 WASAPI 系统音频频谱、屏幕取色流光（Ambilight）、全局按键涟漪感知与本地 UDP/HTTP/WS 控灯桥。

pub mod ambilight;
pub mod audio;
pub mod keyhook;
pub mod runner;
pub mod server;

use serde::Serialize;

use crate::devices::manager::DeviceManager;
use ambilight::{AmbilightEngine, AmbilightSnapshot};
use audio::{AudioEngine, AudioSnapshot};
use keyhook::{KeyActivitySnapshot, KeySensor};
use runner::{NativeRgbRunner, NativeStreamStatus};
use server::{ExternalRgbServer, ExternalServerStatus};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudioTelemetry {
    pub audio: AudioSnapshot,
    pub ambilight: AmbilightSnapshot,
    pub keys: KeyActivitySnapshot,
    pub server: ExternalServerStatus,
    pub native: NativeStreamStatus,
    pub external_keys: Option<Vec<u8>>,
    pub external_logo: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct StudioEngine {
    pub audio: AudioEngine,
    pub ambilight: AmbilightEngine,
    pub keys: KeySensor,
    pub runner: NativeRgbRunner,
    pub server: ExternalRgbServer,
}

impl StudioEngine {
    pub fn new() -> Self {
        Self {
            audio: AudioEngine::new(),
            ambilight: AmbilightEngine::new(),
            keys: KeySensor::new(),
            runner: NativeRgbRunner::new(),
            server: ExternalRgbServer::new(),
        }
    }

    pub fn is_rust_owned_streaming(&self) -> bool {
        self.runner.is_running() || self.server.is_running()
    }

    pub fn set_sensors(&self, audio: bool, ambilight: bool, keys: bool) {
        self.audio.set_enabled(audio);
        self.ambilight.set_enabled(ambilight);
        self.keys.set_enabled(keys);
    }

    pub fn poll_telemetry(&self) -> StudioTelemetry {
        let server = self.server.status();
        let native = self.runner.status();
        let (external_keys, external_logo) = if server.running {
            match self.server.latest_frame() {
                Some((k, l)) => (Some(k), Some(l)),
                None => (None, None),
            }
        } else {
            (None, None)
        };
        StudioTelemetry {
            audio: self.audio.snapshot(),
            ambilight: self.ambilight.snapshot(),
            keys: self.keys.snapshot(),
            server,
            native,
            external_keys,
            external_logo,
        }
    }

    pub fn shutdown(&self, manager: &DeviceManager) {
        self.runner.stop(manager);
        self.server.stop(manager);
        self.audio.stop();
        self.ambilight.stop();
        self.keys.stop();
    }
}
