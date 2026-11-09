//! 将 DSH 请求的新窗口和授权流程转交给系统浏览器。

use super::{bridge, desktop_window, Desktop};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{
    webview::{NewWindowFeatures, NewWindowResponse},
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

const POPUP_PREFIX: &str = "dsh-popup-";
const POPUP_TIMEOUT: Duration = Duration::from_secs(120);
const PLATFORM_ORIGIN: &str = "https://platform.deepseek.com";
const CONFIRM_TITLE: &str = "在默认浏览器中打开链接？";
const OPEN_BUTTON: &str = "在浏览器中打开";
static NEXT_POPUP: AtomicU64 = AtomicU64::new(0);
static CONFIRMATION_PENDING: AtomicBool = AtomicBool::new(false);

fn workspace_instance(app: &AppHandle) -> Option<String> {
    let origin = desktop_window(app)?.url().ok()?.origin();
    let desktop = app.state::<Arc<Desktop>>();
    let connection = desktop.connection.lock().unwrap();
    let workspace = connection.workspace.as_ref()?;
    (bridge::origin(workspace)? == origin.ascii_serialization()).then(|| workspace.instance.clone())
}

fn is_external_https(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

fn is_login_url(url: &Url) -> bool {
    is_external_https(url)
        && url.origin().ascii_serialization() == PLATFORM_ORIGIN
        && url.path() == "/dsh/authorize"
}

fn open_in_browser(url: Url) {
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(error) = open::that(url.as_str()) {
            eprintln!("无法在浏览器中打开 DSH 链接（{:?}）", error.kind());
        }
    });
}

fn forward_link(app: &AppHandle, instance: String, url: Url) {
    if !is_external_https(&url) {
        return;
    }
    if is_login_url(&url) {
        open_in_browser(url);
        return;
    }
    let Some(parent) = desktop_window(app) else {
        return;
    };
    if CONFIRMATION_PENDING.swap(true, Ordering::Relaxed) {
        return;
    }

    let description = format!("目标网站\n{}", url.origin().ascii_serialization());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if confirm_link(&parent, description).await
            && workspace_instance(&app).as_deref() == Some(instance.as_str())
        {
            open_in_browser(url);
        }
        CONFIRMATION_PENDING.store(false, Ordering::Relaxed);
    });
}

#[cfg(windows)]
async fn confirm_link(parent: &WebviewWindow, description: String) -> bool {
    use windows_sys::Win32::UI::{
        Controls::{
            TaskDialogIndirect, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TASKDIALOG_BUTTON,
            TDF_ALLOW_DIALOG_CANCELLATION, TDF_POSITION_RELATIVE_TO_WINDOW, TDF_SIZE_TO_CONTENT,
            TD_INFORMATION_ICON,
        },
        WindowsAndMessaging::{IDCANCEL, IDOK},
    };

    let Ok(hwnd) = parent.hwnd() else {
        return false;
    };
    let hwnd = hwnd.0 as usize;
    tauri::async_runtime::spawn_blocking(move || {
        let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<_>>();
        let title = wide(CONFIRM_TITLE);
        let content = wide(&description);
        let open = wide(OPEN_BUTTON);
        let cancel = wide("取消");
        let buttons = [
            TASKDIALOG_BUTTON {
                nButtonID: IDOK,
                pszButtonText: open.as_ptr(),
            },
            TASKDIALOG_BUTTON {
                nButtonID: IDCANCEL,
                pszButtonText: cancel.as_ptr(),
            },
        ];
        let config = TASKDIALOGCONFIG {
            cbSize: std::mem::size_of::<TASKDIALOGCONFIG>() as u32,
            hwndParent: hwnd as _,
            dwFlags: TDF_ALLOW_DIALOG_CANCELLATION
                | TDF_POSITION_RELATIVE_TO_WINDOW
                | TDF_SIZE_TO_CONTENT,
            pszWindowTitle: title.as_ptr(),
            pszContent: content.as_ptr(),
            Anonymous1: TASKDIALOGCONFIG_0 {
                pszMainIcon: TD_INFORMATION_ICON,
            },
            cButtons: buttons.len() as u32,
            pButtons: buttons.as_ptr(),
            nDefaultButton: IDCANCEL,
            pfCallback: Some(center_task_dialog),
            lpCallbackData: hwnd as isize,
            ..Default::default()
        };
        let mut pressed = 0;
        // 同步调用返回前，配置引用的按钮和 UTF-16 文本始终有效。
        let status = unsafe {
            TaskDialogIndirect(
                &config,
                &mut pressed,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        status >= 0 && pressed == IDOK
    })
    .await
    .unwrap_or(false)
}

#[cfg(windows)]
unsafe extern "system" fn center_task_dialog(
    dialog: windows_sys::Win32::Foundation::HWND,
    message: u32,
    _: usize,
    _: isize,
    parent: isize,
) -> i32 {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        UI::{
            Controls::TDN_CREATED,
            WindowsAndMessaging::{
                GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
            },
        },
    };

    if message != TDN_CREATED as u32 {
        return 0;
    }
    // 系统的相对定位可能带偏移。显示前按实际尺寸居中，并保留在屏幕工作区内。
    let mut owner = RECT::default();
    let mut bounds = RECT::default();
    let mut monitor = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        if GetWindowRect(parent as _, &mut owner) != 0
            && GetWindowRect(dialog, &mut bounds) != 0
            && GetMonitorInfoW(
                MonitorFromWindow(parent as _, MONITOR_DEFAULTTONEAREST),
                &mut monitor,
            ) != 0
        {
            let work = monitor.rcWork;
            let width = bounds.right - bounds.left;
            let height = bounds.bottom - bounds.top;
            let x = ((owner.left + owner.right - width) / 2)
                .clamp(work.left, (work.right - width).max(work.left));
            let y = ((owner.top + owner.bottom - height) / 2)
                .clamp(work.top, (work.bottom - height).max(work.top));
            SetWindowPos(
                dialog,
                std::ptr::null_mut(),
                x,
                y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            );
        }
    }
    0
}

#[cfg(target_os = "linux")]
async fn confirm_link(parent: &WebviewWindow, description: String) -> bool {
    use gtk::prelude::*;
    use gtk::{ButtonsType, MessageDialog, MessageType, ResponseType, WindowPosition};
    use std::{cell::RefCell, rc::Rc};

    let (result, answer) = tokio::sync::oneshot::channel();
    let window = parent.clone();
    if parent
        .run_on_main_thread(move || {
            let Ok(parent) = window.gtk_window() else {
                return;
            };
            let dialog = MessageDialog::builder()
                .transient_for(&parent)
                .modal(true)
                .destroy_with_parent(true)
                .window_position(WindowPosition::CenterOnParent)
                .message_type(MessageType::Info)
                .buttons(ButtonsType::None)
                .title(CONFIRM_TITLE)
                .text(CONFIRM_TITLE)
                .secondary_text(description)
                .build();
            dialog.add_button("取消", ResponseType::Cancel);
            dialog.add_button(OPEN_BUTTON, ResponseType::Accept);
            dialog.set_default_response(ResponseType::Cancel);

            // 父窗口销毁时 GTK 不发送 response，仍需结束等待。
            let result = Rc::new(RefCell::new(Some(result)));
            let cancel = result.clone();
            dialog.connect_destroy(move |_| {
                if let Some(result) = cancel.borrow_mut().take() {
                    let _ = result.send(false);
                }
            });
            dialog.connect_response(move |dialog, response| {
                if let Some(result) = result.borrow_mut().take() {
                    let _ = result.send(response == ResponseType::Accept);
                }
                // 此处仍在 GTK 主线程，回调参数是当前有效的对话框。
                unsafe { dialog.destroy() };
            });
            dialog.show();
        })
        .is_err()
    {
        return false;
    }
    answer.await.unwrap_or(false)
}

#[cfg(target_os = "macos")]
async fn confirm_link(parent: &WebviewWindow, description: String) -> bool {
    let (result, answer) = tokio::sync::oneshot::channel();
    let window = parent.clone();
    if parent
        .run_on_main_thread(move || {
            if window.ns_window().is_err() {
                return;
            }
            // rfd 在构造面板的 future 时就会从 NSView 读取父窗口。
            let dialog = native_link_dialog(&window, description).show();
            tauri::async_runtime::spawn(async move {
                let _ = result.send(link_dialog_confirmed(dialog.await));
            });
        })
        .is_err()
    {
        return false;
    }
    answer.await.unwrap_or(false)
}

#[cfg(all(not(windows), not(target_os = "linux"), not(target_os = "macos")))]
async fn confirm_link(parent: &WebviewWindow, description: String) -> bool {
    link_dialog_confirmed(native_link_dialog(parent, description).show().await)
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn native_link_dialog(parent: &WebviewWindow, description: String) -> rfd::AsyncMessageDialog {
    rfd::AsyncMessageDialog::new()
        .set_parent(parent)
        .set_title(CONFIRM_TITLE)
        .set_description(description)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(
            OPEN_BUTTON.into(),
            "取消".into(),
        ))
}

#[cfg(all(not(windows), not(target_os = "linux")))]
fn link_dialog_confirmed(answer: rfd::MessageDialogResult) -> bool {
    matches!(answer, rfd::MessageDialogResult::Custom(button) if button == OPEN_BUTTON)
}

pub(super) fn open(
    app: &AppHandle,
    url: Url,
    features: NewWindowFeatures,
) -> NewWindowResponse<tauri::Wry> {
    let Some(instance) = workspace_instance(app) else {
        return NewWindowResponse::Deny;
    };
    if url.as_str() != "about:blank" {
        forward_link(app, instance, url);
        return NewWindowResponse::Deny;
    }
    if has_popup(app) {
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
                if is_login_url(url)
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

    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;

        let app = app.clone();
        let label = label.clone();
        // WebKitGTK 的 window.close() 会销毁 WebView，外层窗口仍由 Tauri 回收。
        let _ = window.with_webview(move |webview| {
            webview.inner().connect_destroy(move |_| {
                let app = app.clone();
                let label = label.clone();
                gtk::glib::idle_add_local_once(move || close(&app, &label));
            });
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
    fn confirmation_accepts_https_links_without_credentials() {
        for url in [
            "https://platform.deepseek.com/",
            "https://trtgsjkv6r.feishu.cn/share/base/form/shrcnlCoGElW7MQznGy9r3YYXcg?hide_uid=1",
            "https://example.org/docs",
        ] {
            assert!(is_external_https(&Url::parse(url).unwrap()), "{url}");
        }
        for url in [
            "https://user:password@platform.deepseek.com/dsh/authorize",
            "http://127.0.0.1:12345/",
            "file:///C:/example",
            "mailto:someone@example.org",
            "javascript:alert(1)",
            "blob:https://platform.deepseek.com/example",
        ] {
            assert!(!is_external_https(&Url::parse(url).unwrap()), "{url}");
        }
    }

    #[test]
    fn only_dsh_login_bypasses_confirmation() {
        let login = Url::parse("https://platform.deepseek.com/dsh/authorize?state=test").unwrap();
        assert!(is_login_url(&login));
        for url in [
            "https://platform.deepseek.com/",
            "https://platform.deepseek.com/dsh/authorize/other",
            "https://platform.deepseek.com:444/dsh/authorize",
            "https://platform.deepseek.com.example/dsh/authorize",
            "https://user@platform.deepseek.com/dsh/authorize",
        ] {
            assert!(!is_login_url(&Url::parse(url).unwrap()), "{url}");
        }
    }
}
