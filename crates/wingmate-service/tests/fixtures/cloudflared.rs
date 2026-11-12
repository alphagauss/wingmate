fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("cloudflared version 2026.10.0");
        return;
    }
    match args.as_slice() {
        [tunnel, no_update, option, origin]
            if tunnel == "tunnel" && no_update == "--no-autoupdate" && option == "--url" =>
        {
            std::fs::write(std::env::var_os("TEST_TUNNEL_ORIGIN").unwrap(), origin).unwrap();
            println!("| https://wingmate-{}.trycloudflare.com |", std::process::id());
        }
        [tunnel, no_update, run]
            if tunnel == "tunnel" && no_update == "--no-autoupdate" && run == "run" =>
        {
            if std::env::var("TUNNEL_TOKEN").as_deref() != Ok("test-tunnel-token") {
                std::process::exit(1);
            }
        }
        _ => std::process::exit(2),
    }
    eprintln!("INF Registered tunnel connection connIndex=0");
    loop {
        std::thread::park();
    }
}
