use super::desktop_window;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Url, WebviewWindow};
use wingmate_service::service::{Snapshot, StartupStage};

#[derive(Clone, PartialEq, Eq, serde::Serialize)]
pub(super) struct CardStatus {
    message: &'static str,
    hint: &'static str,
    waiting: bool,
    error: Option<String>,
    actions: &'static str,
}

impl CardStatus {
    fn waiting(message: &'static str, hint: &'static str) -> Self {
        Self {
            message,
            hint,
            waiting: true,
            error: None,
            actions: "",
        }
    }

    fn for_stage(stage: StartupStage) -> Self {
        match stage {
            StartupStage::Checking => Self::waiting("正在检查后台…", ""),
            StartupStage::Connecting => Self::waiting("正在连接已有后台…", ""),
            StartupStage::Extracting => {
                Self::waiting("正在解压运行文件…", "首次启动可能需要一点时间。")
            }
            StartupStage::Starting => {
                Self::waiting("正在启动后台…", "后台就绪后将自动打开工作台。")
            }
            StartupStage::Stopping => Self::waiting("正在停止后台…", ""),
        }
    }

    pub(super) fn failure(stage: StartupStage, error: String) -> Self {
        let message = match stage {
            StartupStage::Checking => "无法检查后台",
            StartupStage::Connecting => "无法连接已有后台",
            StartupStage::Extracting => "运行文件解压失败",
            StartupStage::Starting => "后台启动失败",
            StartupStage::Stopping => "后台停止失败",
        };
        let hint = match stage {
            StartupStage::Extracting => "检查磁盘空间和目录权限后再试。",
            StartupStage::Stopping => "重新连接后可再次管理后台。",
            _ => "可重试启动或连接已有后台。",
        };
        Self {
            message,
            hint,
            waiting: false,
            error: Some(error),
            actions: "retry",
        }
    }

    pub(super) fn stopped() -> Self {
        Self {
            message: "后台已停止",
            hint: "启动后将自动打开工作台。",
            waiting: false,
            error: None,
            actions: "start",
        }
    }

    pub(super) fn update_available() -> Self {
        Self {
            message: "有新的后台运行包可用",
            hint: "可继续使用当前后台，或重启后台以启用新版功能。",
            waiting: false,
            error: None,
            actions: "update",
        }
    }
}

#[derive(Clone)]
pub(super) struct StartupPage {
    url: Url,
    status: Arc<Mutex<CardStatus>>,
}

impl StartupPage {
    pub(super) fn new() -> Self {
        // Tauri's bundled app protocol uses an HTTP origin on WebView2.
        let url = if cfg!(any(windows, target_os = "android")) {
            "http://tauri.localhost/"
        } else {
            "tauri://localhost"
        };
        Self {
            url: url.parse().unwrap(),
            status: Arc::new(Mutex::new(CardStatus::for_stage(StartupStage::Checking))),
        }
    }

    pub(super) fn loaded(&self, window: &WebviewWindow, url: &Url) {
        if self.is_home(url) {
            self.render(window);
        }
    }

    pub(super) fn is_home(&self, url: &Url) -> bool {
        url == &self.url
    }

    fn render(&self, window: &WebviewWindow) {
        let status = self.status.lock().unwrap().clone();
        let args = serde_json::to_string(&(self.url.as_str(), status)).unwrap();
        let _ = window.eval(&format!(
            r#"(([url, status]) => {{
                if (location.href === url) window.renderWingmateHome?.(status);
            }})({args})"#
        ));
    }

    fn update(&self, window: &WebviewWindow, status: CardStatus, show_card: bool) {
        let mut current = self.status.lock().unwrap();
        if *current == status {
            return;
        }
        *current = status;
        drop(current);

        if window.url().as_ref().ok() == Some(&self.url) {
            self.render(window);
        } else if show_card {
            let _ = window.navigate(self.url.clone());
        }
    }

    pub(super) fn show(&self, app: &AppHandle, status: CardStatus) {
        self.show_status(app, status, true);
    }

    pub(super) fn show_progress(&self, app: &AppHandle, stage: StartupStage) {
        // 仅检查或连接已有后台时，让当前工作台继续显示，直到连接结果确定。
        let show_card = !matches!(stage, StartupStage::Checking | StartupStage::Connecting);
        self.show_status(app, CardStatus::for_stage(stage), show_card);
    }

    fn show_status(&self, app: &AppHandle, status: CardStatus, show_card: bool) {
        let page = self.clone();
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = desktop_window(&handle) {
                page.update(&window, status, show_card);
            }
        });
    }

    pub(super) fn open_workspace(&self, app: &AppHandle, state: &Snapshot) {
        let Some(url) = state.url.as_ref().and_then(|url| url.parse().ok()) else {
            self.show(
                app,
                CardStatus::failure(
                    StartupStage::Connecting,
                    state
                        .error
                        .clone()
                        .unwrap_or_else(|| "后台未提供可用的工作台地址".into()),
                ),
            );
            return;
        };
        let page = self.clone();
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = desktop_window(&handle) {
                if window.url().as_ref().ok() == Some(&page.url) {
                    page.update(&window, CardStatus::waiting("正在打开工作台…", ""), false);
                }
                if window.navigate(url).is_err() {
                    page.update(
                        &window,
                        CardStatus {
                            message: "无法打开工作台",
                            hint: "可重试连接后台。",
                            waiting: false,
                            error: None,
                            actions: "retry",
                        },
                        true,
                    );
                }
            }
        });
    }
}
