//! CLI 连接信息与本机诊断。
use crate::{service, toolchains};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

pub fn connect_info(home: &Path, pair: bool) -> Result<Value> {
    let service = service::connection_status(home)?.context("管理服务未运行")?;
    let command = if pair {
        service::ShareCommand::PairLocal
    } else {
        service::ShareCommand::Status
    };
    let status = service::share(home, &command)?;
    let address = status.local_address.with_context(|| {
        status
            .web_error
            .as_deref()
            .unwrap_or("本机 Web 入口不可用")
            .to_owned()
    })?;
    let mut info = json!({
        "schema_version": 1,
        "host": address.ip(),
        "port": address.port(),
        "url": format!("http://{address}"),
        "service_instance": service.instance,
    });
    if pair {
        info["pairing_url"] = json!(status.pairing_url.context("管理服务未返回配对链接")?);
        info["pairing_expires_at"] = json!(status
            .pairing_expires_at
            .context("管理服务未返回配对链接有效期")?);
    }
    Ok(info)
}

#[cfg(target_os = "linux")]
pub fn gateway_stdio(home: &Path) -> Result<()> {
    use std::net::{Shutdown, TcpStream};

    let address = service::connection_status(home)?
        .and_then(|service| service.web_address)
        .context("本机网关未运行")?;
    anyhow::ensure!(address.ip().is_loopback(), "网关地址必须属于回环接口");
    let mut stream =
        TcpStream::connect_timeout(&address.into(), std::time::Duration::from_secs(4))?;
    let mut request = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut request);
        let _ = request.shutdown(Shutdown::Write);
    });
    let mut output = io::stdout().lock();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        output.write_all(&buffer[..count])?;
        output.flush()?;
    }
}

pub fn doctor(home: &Path) -> Value {
    let node = match toolchains::diagnostics(home) {
        Ok(node) => node,
        Err(error) => json!({"ok": false, "error": format!("{error:#}")}),
    };
    let service = match service::connection_status(home) {
        Ok(Some(status)) => {
            let gateway_error = if status.web_address.is_none() {
                match service::share(home, &service::ShareCommand::Status) {
                    Ok(share) => share.web_error,
                    Err(error) => Some(format!("{error:#}")),
                }
            } else {
                None
            };
            json!({
                "phase": "running",
                "gateway_address": status.web_address,
                "gateway_error": gateway_error,
                "business_phase": status.business.as_ref().map_or("stopped", |state| state.phase.as_str()),
            })
        }
        Ok(None) => json!({"phase": "stopped", "business_phase": "stopped"}),
        Err(error) => json!({"phase": "unavailable", "error": format!("{error:#}")}),
    };
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "home": home,
        "node": node,
        "service": service,
    })
}

pub fn logs(home: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let mut args = args.peekable();
    let names: &[&str] = match args.peek().map(String::as_str) {
        Some("host") => {
            args.next();
            &["host.log"]
        }
        Some("service") => {
            args.next();
            &["service.log"]
        }
        _ => &["host.log", "service.log"],
    };
    let lines: usize = match args.next().as_deref() {
        None => 100,
        Some("--tail") => args
            .next()
            .context("--tail 需要行数")?
            .parse()
            .context("--tail 需要正整数")?,
        _ => anyhow::bail!("用法：wingmate logs [host|service] [--tail <行数>]"),
    };
    anyhow::ensure!(lines > 0, "--tail 需要正整数");
    anyhow::ensure!(args.next().is_none(), "命令参数过多");

    let mut out = io::stdout().lock();
    let mut printed_header = false;
    for name in names {
        let path = home.join(name);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                eprintln!("尚无日志：{}", path.display());
                continue;
            }
            Err(error) => return Err(error).with_context(|| format!("打开 {}", path.display())),
        };
        if names.len() > 1 {
            if printed_header {
                writeln!(out)?;
            }
            writeln!(out, "==> {name} <==")?;
            printed_header = true;
        }
        write_log_tail(file, lines, &mut out)
            .with_context(|| format!("读取 {}", path.display()))?;
    }
    Ok(())
}

fn write_log_tail(mut file: File, mut lines: usize, out: &mut impl Write) -> io::Result<()> {
    let end = file.seek(SeekFrom::End(0))?;
    let mut offset = end;
    let mut start = 0;
    let mut buffer = [0; 8192];
    'scan: while offset > 0 {
        let size = offset.min(buffer.len() as u64) as usize;
        offset -= size as u64;
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut buffer[..size])?;
        for (index, byte) in buffer[..size].iter().enumerate().rev() {
            let position = offset + index as u64;
            if *byte == b'\n' && position + 1 != end {
                lines -= 1;
                if lines == 0 {
                    start = position + 1;
                    break 'scan;
                }
            }
        }
    }
    file.seek(SeekFrom::Start(start))?;
    io::copy(&mut file.take(end - start), out)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_tail_reads_across_blocks_and_keeps_original_line_endings() -> Result<()> {
        let mut file = tempfile::tempfile()?;
        let mut long_line = vec![b'a'; 8192];
        long_line.extend_from_slice(b"\r\n");
        file.write_all(b"old line\r\n")?;
        file.write_all(&long_line)?;
        file.write_all(b"last line")?;

        let mut out = Vec::new();
        write_log_tail(file.try_clone()?, 1, &mut out)?;
        assert_eq!(out, b"last line");
        out.clear();
        write_log_tail(file.try_clone()?, 2, &mut out)?;
        assert_eq!(out, [long_line.as_slice(), b"last line"].concat());

        file.seek(SeekFrom::End(0))?;
        file.write_all(b"\r\n")?;
        out.clear();
        write_log_tail(file, 1, &mut out)?;
        assert_eq!(out, b"last line\r\n");
        Ok(())
    }
}
