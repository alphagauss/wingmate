use anyhow::{bail, Result};

fn main() -> Result<()> {
    #[cfg(any(feature = "bundled-runtime", feature = "release-client"))]
    bail!("此工具必须通过 --no-default-features 编译");
    #[cfg(not(any(feature = "bundled-runtime", feature = "release-client")))]
    match std::env::args().nth(1).as_deref() {
        Some("prepare-dev") => wingmate_launcher::development::prepare(true),
        Some("prepare-build") => wingmate_launcher::development::prepare(false),
        Some("stop-dev") => {
            let home = wingmate_launcher::directory()?;
            wingmate_launcher::service::stop(&home)?;
            wingmate_launcher::service::shutdown(&home)?;
            println!("开发后台已停止");
            Ok(())
        }
        _ => bail!("使用 prepare-dev、prepare-build 或 stop-dev"),
    }
}
