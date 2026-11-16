fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("cloudflared version 2026.10.0");
        return;
    }
    let [tunnel, no_update, flag, pidfile, mode @ ..] = args.as_slice() else {
        std::process::exit(2);
    };
    assert_eq!(tunnel, "tunnel");
    assert_eq!(no_update, "--no-autoupdate");
    assert_eq!(flag, "--pidfile");
    match mode {
        [option, origin] if option == "--url" => {
            std::fs::write(std::env::var_os("TEST_TUNNEL_ORIGIN").unwrap(), origin).unwrap();
            println!(
                "| https://wingmate-{}.trycloudflare.com |",
                std::process::id()
            );
        }
        [run] if run == "run" => {
            if std::env::var("TUNNEL_TOKEN").as_deref() != Ok("test-tunnel-token") {
                std::process::exit(1);
            }
        }
        _ => std::process::exit(2),
    }
    std::fs::write(pidfile, std::process::id().to_string()).unwrap();
    loop {
        std::thread::park();
    }
}
