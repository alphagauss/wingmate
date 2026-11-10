use anyhow::{bail, Result};

fn main() -> Result<()> {
    #[cfg(feature = "bundled-runtime")]
    bail!("此工具必须通过 --no-default-features 编译");
    #[cfg(not(feature = "bundled-runtime"))]
    match std::env::args().nth(1).as_deref() {
        Some("prepare-dev") => wingmate_service::development::prepare(true),
        Some("prepare-build") => wingmate_service::development::prepare(false),
        Some("stop-dev") => {
            wingmate_service::service::stop(&wingmate_service::directory()?)?;
            println!("开发后台已停止");
            Ok(())
        }
        _ => bail!("使用 prepare-dev、prepare-build 或 stop-dev"),
    }
}
