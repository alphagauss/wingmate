use tauri::{ipc::CapabilityBuilder, AppHandle, Manager, Url, WebviewWindow};
use wingmate_service::service::Snapshot;

pub(super) fn origin(state: &Snapshot) -> Option<String> {
    let url = Url::parse(state.url.as_deref()?).ok()?;
    Some(url.origin().ascii_serialization())
}

pub(super) fn authorize(app: &AppHandle, origin: &str) -> tauri::Result<()> {
    app.add_capability(
        CapabilityBuilder::new("wingmate-workspace")
            .webview("main")
            .local(false)
            .remote(format!("{origin}/*"))
            .permission("allow-manage-backend"),
    )
}

pub(super) fn expose(window: &WebviewWindow, url: &Url, state: &Snapshot) {
    let Some(origin) = origin(state) else { return };
    if url.origin().ascii_serialization() != origin {
        return;
    }
    let version = state.runtime.strip_prefix("dsh-").unwrap_or(&state.runtime);
    let data = serde_json::json!([origin, state.instance, version]);
    // Recheck in the document: navigation can change between PageLoad and eval.
    let _ = window.eval(&format!(
        r#"(([origin, instance, version]) => {{
            if (window !== window.top || location.origin !== origin || window.__WINGMATE_BACKEND__) return;
            const invoke = window.__TAURI_INTERNALS__.invoke;
            Object.defineProperty(window, '__WINGMATE_BACKEND__', {{ value: Object.freeze({{
                version,
                manage: action => invoke('manage_backend', {{ action, instance }}),
            }}) }});
            window.dispatchEvent(new Event('wingmate:backend-bridge'));
        }})({data})"#
    ));
}
