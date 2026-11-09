use super::{bridge, desktop_window, Desktop, StartupPage};
use std::{cell::Cell, sync::Arc, time::Duration};
use tauri::{Manager, Url, WebviewWindow, WindowEvent};
use wingmate_service::service::Snapshot;

#[derive(Clone)]
pub(super) struct Source {
    address: String,
    instance: Option<String>,
}

impl Source {
    fn new(url: &Url, page: &StartupPage, workspace: Option<&Snapshot>) -> Option<Self> {
        if page.is_home(url) {
            return Some(Self {
                address: url.as_str().to_owned(),
                instance: None,
            });
        }
        let workspace = workspace?;
        let source = Self {
            address: bridge::origin(workspace)?,
            instance: Some(workspace.instance.clone()),
        };
        source.matches(url).then_some(source)
    }

    fn matches(&self, url: &Url) -> bool {
        if self.instance.is_none() {
            url.as_str() == self.address
        } else {
            url.origin().ascii_serialization() == self.address
        }
    }
}

pub(super) fn expose(window: &WebviewWindow, source: &Source, theme: Theme, navigation: u32) {
    let args = serde_json::json!([
        source.address,
        source.instance,
        theme,
        wingmate_service::IS_DEV,
        navigation
    ]);
    let script = include_str!("titlebar.js");
    let _ = window.eval(&format!("({script})({args})"));
}

#[derive(Default)]
pub(super) struct Frame {
    navigation: u32,
    ready: bool,
    source: Option<Source>,
}

impl Frame {
    pub(super) fn navigation(&self) -> u32 {
        self.navigation
    }

    pub(super) fn start(&mut self, window: &WebviewWindow) {
        self.navigation += 1;
        self.ready = false;
        self.source = None;
        watch(window, self.navigation);
    }

    pub(super) fn bind(
        &mut self,
        url: &Url,
        page: &StartupPage,
        workspace: Option<&Snapshot>,
    ) -> Option<&Source> {
        if !cfg!(windows) {
            return None;
        }
        if self.source.is_none() {
            self.source = Source::new(url, page, workspace);
        }
        self.source.as_ref().filter(|source| source.matches(url))
    }

    fn allows(&self, url: &Url, instance: Option<&str>, navigation: u32) -> bool {
        self.navigation == navigation
            && self
                .source
                .as_ref()
                .is_some_and(|source| source.instance.as_deref() == instance && source.matches(url))
    }
}

pub(super) fn watch(window: &WebviewWindow, navigation: u32) {
    if !cfg!(windows) {
        return;
    }
    let app = window.app_handle().clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            let desktop = handle.state::<Arc<Desktop>>();
            let current = desktop.connection.lock().unwrap();
            let needs_fallback =
                current.titlebar.navigation == navigation && !current.titlebar.ready;
            drop(current);
            if needs_fallback {
                if let Some(window) = desktop_window(&handle) {
                    let _ = window.set_decorations(true);
                    let _ = show_if_hidden(&window);
                }
            }
        });
    });
}

fn show_if_hidden(window: &WebviewWindow) -> tauri::Result<()> {
    if !window.is_visible()? {
        window.show()?;
    }
    Ok(())
}

pub(super) fn observe(window: &WebviewWindow) {
    if !cfg!(windows) {
        return;
    }
    let main = window.clone();
    let last = Cell::new(None);
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Resized(_)) {
            if let Ok(maximized) = main.is_maximized() {
                if last.replace(Some(maximized)) != Some(maximized) {
                    let _ = main.eval(format!(
                        "window.__WINGMATE_TITLEBAR__?.setMaximized({maximized})"
                    ));
                }
            }
        }
    });
}

pub(super) fn connection_title(window: &WebviewWindow, title: &str, disconnected: bool) {
    let title = if disconnected {
        format!("{title} · 后台连接暂时中断")
    } else {
        title.to_owned()
    };
    let main = window.clone();
    let _ = window.run_on_main_thread(move || {
        if main.title().ok().as_deref() != Some(title.as_str()) {
            let _ = main.set_title(&title);
            if cfg!(windows) {
                let _ = main.eval(format!(
                    "window.__WINGMATE_TITLEBAR__?.setDisconnected({disconnected})"
                ));
            }
        }
    });
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Action {
    Ready,
    Fallback,
    Minimize,
    ToggleMaximize,
    Close,
    Theme(Theme),
}

#[derive(Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[tauri::command]
pub(super) fn window_chrome(
    window: WebviewWindow,
    desktop: tauri::State<'_, Arc<Desktop>>,
    action: Action,
    instance: Option<String>,
    navigation: u32,
) -> Result<bool, String> {
    let url = window.url().map_err(|error| error.to_string())?;
    if !cfg!(windows)
        || window.label() != desktop.window_label
        || !desktop.connection.lock().unwrap().titlebar.allows(
            &url,
            instance.as_deref(),
            navigation,
        )
    {
        return Err("此页面不能操作 Wingmate 主窗口".into());
    }
    let result: tauri::Result<()> = match action {
        Action::Ready => {
            let ready = || -> tauri::Result<bool> {
                if window.is_decorated()? {
                    window.set_decorations(false)?;
                }
                let maximized = window.is_maximized()?;
                show_if_hidden(&window)?;
                let mut current = desktop.connection.lock().unwrap();
                if current.titlebar.navigation == navigation {
                    current.titlebar.ready = true;
                }
                Ok(maximized)
            };
            return ready().map_err(|error| error.to_string());
        }
        Action::Fallback => {
            desktop.connection.lock().unwrap().titlebar.ready = false;
            watch(&window, navigation);
            return Ok(false);
        }
        Action::Theme(theme) => {
            if instance.is_none() {
                return Err("仅工作台可以同步主题".into());
            }
            desktop.connection.lock().unwrap().titlebar_theme = theme;
            return Ok(false);
        }
        Action::Minimize => window.minimize(),
        Action::ToggleMaximize if window.is_maximized().unwrap_or(false) => window.unmaximize(),
        Action::ToggleMaximize => window.maximize(),
        Action::Close => {
            return window
                .close()
                .map(|()| false)
                .map_err(|error| error.to_string())
        }
    };
    result
        .and_then(|()| window.is_maximized())
        .map_err(|error| error.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn window_controls_stay_bound_to_the_loaded_document_when_backend_stops() {
        let workspace = Url::parse("http://127.0.0.1:32123/").unwrap();
        let other_origin = Url::parse("http://127.0.0.1:32124/").unwrap();
        let mut frame = Frame {
            navigation: 2,
            source: Some(Source {
                address: workspace.origin().ascii_serialization(),
                instance: Some("loaded".into()),
            }),
            ..Frame::default()
        };

        assert!(frame.bind(&workspace, &StartupPage::new(), None).is_some());
        assert!(frame.allows(&workspace, Some("loaded"), 2));
        assert!(!frame.allows(&workspace, Some("loaded"), 1));
        assert!(!frame.allows(&workspace, Some("other"), 2));
        assert!(!frame.allows(&other_origin, Some("loaded"), 2));
    }
}
