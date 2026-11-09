//! 将 DSH 请求的新窗口和授权流程转交给系统浏览器。

use super::{bridge, Desktop};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    webview::{NewWindowFeatures, NewWindowResponse},
    AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder,
};

const POPUP_PREFIX: &str = "dsh-popup-";
const POPUP_TIMEOUT: Duration = Duration::from_secs(120);
static NEXT_POPUP: AtomicU64 = AtomicU64::new(0);

fn workspace_instance(app: &AppHandle) -> Option<String> {
    let origin = app.get_webview_window("main")?.url().ok()?.origin();
    let desktop = app.state::<Arc<Desktop>>();
    let connection = desktop.connection.lock().unwrap();
    let workspace = connection.workspace.as_ref()?;
    (bridge::origin(workspace)? == origin.ascii_serialization()).then(|| workspace.instance.clone())
}

fn browser_destination(url: &Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && match url.origin().ascii_serialization().as_str() {
            "https://platform.deepseek.com" => true,
            "https://trtgsjkv6r.feishu.cn" => {
                url.path() == "/share/base/form/shrcnlCoGElW7MQznGy9r3YYXcg"
            }
            _ => false,
        }
}

fn open_in_browser(url: Url) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = open::that(url.as_str()) {
            eprintln!("无法在浏览器中打开 DSH 链接（{:?}）", error.kind());
        }
    });
}

pub(super) fn open(
    app: &AppHandle,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    let Some(instance) = workspace_instance(app) else {
        return NewWindowResponse::Deny;
    };
    if browser_destination(&url) {
        open_in_browser(url);
        return NewWindowResponse::Deny;
    }
    if url.as_str() != "about:blank" || has_popup(app) {
        return NewWindowResponse::Deny;
    }
    let label = format!(
        "{POPUP_PREFIX}{}",
        NEXT_POPUP.fetch_add(1, Ordering::Relaxed)
    );
    let navigation_app = app.clone();
    let navigation_label = label.clone();
    let finished = AtomicBool::new(false);
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .window_features(features)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .on_navigation(move |url| {
            if url.as_str() == "about:blank" {
                return true;
            }
            if !finished.swap(true, Ordering::Relaxed) {
                close(&navigation_app, &navigation_label);
                if browser_destination(url)
                    && workspace_instance(&navigation_app).as_deref() == Some(instance.as_str())
                {
                    open_in_browser(url.clone());
                }
            }
            false
        })
        .build();
    let window = match window {
        Ok(window) => window,
        Err(error) => {
            eprintln!("无法创建 DSH 授权窗口：{error}");
            return NewWindowResponse::Deny;
        }
    };

    #[cfg(windows)]
    {
        use webview2_com::WindowCloseRequestedEventHandler;

        let app = app.clone();
        let label = label.clone();
        // Wry 只销毁 WebView2 的容器 HWND，Tauri 仍需回收外层窗口。
        let _ = window.with_webview(move |webview| unsafe {
            if let Ok(webview) = webview.controller().CoreWebView2() {
                let handler = WindowCloseRequestedEventHandler::create(Box::new(move |_, _| {
                    close(&app, &label);
                    Ok(())
                }));
                let _ = webview.add_WindowCloseRequested(&handler, &mut 0);
            }
        });
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(POPUP_TIMEOUT).await;
        close(&app, &label);
    });
    NewWindowResponse::Create { window }
}

fn has_popup(app: &AppHandle) -> bool {
    app.webview_windows()
        .keys()
        .any(|label| label.starts_with(POPUP_PREFIX))
}

fn close(app: &AppHandle, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

pub(super) fn close_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(POPUP_PREFIX) {
            let _ = window.destroy();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_trusted_platform_and_feedback_urls_are_forwarded() {
        for url in [
            "https://platform.deepseek.com/dsh/authorize?state=test",
            "https://platform.deepseek.com/",
            "https://platform.deepseek.com/usage",
            "https://trtgsjkv6r.feishu.cn/share/base/form/shrcnlCoGElW7MQznGy9r3YYXcg?hide_uid=1",
        ] {
            assert!(browser_destination(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "http://platform.deepseek.com/dsh/authorize",
            "https://platform.deepseek.com:444/dsh/authorize",
            "https://platform.deepseek.com.example/dsh/authorize",
            "https://user:password@platform.deepseek.com/dsh/authorize",
            "blob:https://platform.deepseek.com/example",
            "https://trtgsjkv6r.feishu.cn/share/base/form/other",
            "https://other.feishu.cn/share/base/form/shrcnlCoGElW7MQznGy9r3YYXcg",
            "http://127.0.0.1:12345/",
            "file:///C:/example",
        ] {
            assert!(!browser_destination(&Url::parse(url).unwrap()), "{url}");
        }
    }
}
