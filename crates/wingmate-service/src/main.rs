fn main() -> anyhow::Result<()> {
    let mut args = std::env::args_os().skip(1);
    anyhow::ensure!(
        args.next().as_deref() == Some(std::ffi::OsStr::new("--home")),
        "用法: wingmate-service --home <绝对目录>"
    );
    let home =
        std::path::PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("缺少管理目录"))?);
    anyhow::ensure!(
        home.is_absolute() && args.next().is_none(),
        "管理目录必须是绝对路径"
    );
    wingmate_service::host::run(&home)
}
