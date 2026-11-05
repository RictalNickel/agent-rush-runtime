//! Deterministic sandbox containment probe for demos and evidence.
//! Run: agent-rush sandbox examples/containment_probe.rs --memory-mb 256 --timeout 60
//! Each probe records whether the sandbox blocked the action; the program itself never panics.
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    process::Command,
    sync::{Arc, Barrier},
    time::Duration,
};

fn record(out: &mut BTreeMap<String, (bool, String)>, name: &str, blocked: bool, detail: String) {
    println!(
        "{name:<14} {} {detail}",
        if blocked { "BLOCKED" } else { "allowed" }
    );
    out.insert(name.into(), (blocked, detail));
}

fn grow_memory() {
    // Child mode: allocate and touch 2 GiB in 64 MiB steps until the sandbox stops us.
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    for step in 1..=32 {
        blocks.push(vec![1u8; 64 << 20]);
        println!("allocated {} MiB", step * 64);
        std::io::stdout().flush().ok();
    }
    println!("allocated all 2048 MiB");
}

fn json_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            c if (c as u32) < 0x20 => format!("\\u{:04x}", c as u32).chars().collect(),
            c => vec![c],
        })
        .collect()
}

fn write_result(out: &BTreeMap<String, (bool, String)>) {
    let body: Vec<String> = out
        .iter()
        .map(|(k, (b, d))| {
            format!(
                "\"{k}\":{{\"blocked\":{b},\"detail\":\"{}\"}}",
                json_escape(d)
            )
        })
        .collect();
    fs::create_dir_all("/work/artifacts").unwrap();
    fs::write(
        "/work/artifacts/result.json",
        format!("{{{}}}", body.join(",")),
    )
    .unwrap();
    let blocked = out.values().filter(|(b, _)| *b).count();
    println!("summary: {blocked}/{} probes blocked", out.len());
    std::io::stdout().flush().ok();
}

fn main() {
    if std::env::args().nth(1).as_deref() == Some("grow") {
        return grow_memory();
    }
    let mut out = BTreeMap::new();

    for (name, addr) in [
        ("net_public", "1.1.1.1:443"),
        ("net_metadata", "169.254.169.254:80"),
    ] {
        let addr: SocketAddr = addr.parse().unwrap();
        match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
            Ok(_) => record(&mut out, name, false, format!("connected to {addr}")),
            Err(e) => record(&mut out, name, true, e.to_string()),
        }
    }
    match "example.com:443".to_socket_addrs() {
        Ok(mut a) => record(&mut out, "dns", false, format!("resolved {:?}", a.next())),
        Err(e) => record(&mut out, "dns", true, e.to_string()),
    }

    for (name, path) in [
        ("host_shadow", "/etc/shadow"),
        ("host_ssh_key", "/root/.ssh/id_ed25519"),
        ("control_env", "/etc/agent-rush/control.env"),
        ("worker_env", "/etc/agent-rush/worker.env"),
    ] {
        match fs::read(path) {
            Ok(b) => record(&mut out, name, false, format!("read {} bytes", b.len())),
            Err(e) => record(&mut out, name, true, e.to_string()),
        }
    }
    #[cfg(unix)]
    match std::os::unix::net::UnixStream::connect("/var/run/docker.sock") {
        Ok(_) => record(&mut out, "docker_socket", false, "connected".into()),
        Err(e) => record(&mut out, "docker_socket", true, e.to_string()),
    }

    let secrets: Vec<String> = std::env::vars()
        .map(|(k, _)| k)
        .filter(|k| {
            ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                .iter()
                .any(|w| k.to_uppercase().contains(w))
        })
        .collect();
    record(
        &mut out,
        "env_secrets",
        secrets.is_empty(),
        format!("names: {secrets:?}"),
    );

    for (name, path) in [("write_usr", "/usr/probe"), ("write_etc", "/etc/probe")] {
        match fs::write(path, b"x") {
            Ok(_) => record(&mut out, name, false, format!("wrote {path}")),
            Err(e) => record(&mut out, name, true, e.to_string()),
        }
    }
    match fs::write("/work/probe", b"x") {
        Ok(_) => record(
            &mut out,
            "write_work",
            false,
            "scratch /work is writable (expected)".into(),
        ),
        Err(e) => record(&mut out, "write_work", true, e.to_string()),
    }

    let barrier = Arc::new(Barrier::new(1));
    let mut handles = Vec::new();
    let mut error = String::from("reached 1000");
    for _ in 0..1000 {
        let b = barrier.clone();
        match std::thread::Builder::new()
            .stack_size(64 << 10)
            .spawn(move || {
                let _ = b;
                std::thread::sleep(Duration::from_millis(1500));
            }) {
            Ok(h) => handles.push(h),
            Err(e) => {
                error = e.to_string();
                break;
            }
        }
    }
    let created = handles.len();
    for h in handles {
        let _ = h.join();
    }
    record(
        &mut out,
        "thread_limit",
        created < 1000,
        format!("created {created} threads; {error}"),
    );

    // Save before the memory probe: exceeding the cap kills the whole sandbox, not just the child.
    record(
        &mut out,
        "memory_2gib",
        true,
        "pending: if this entry survives, the sandbox was killed at its memory cap".into(),
    );
    write_result(&out);
    match Command::new(std::env::current_exe().unwrap())
        .arg("grow")
        .output()
    {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            let last = stdout.lines().last().unwrap_or("nothing").to_string();
            let killed = !o.status.success();
            record(
                &mut out,
                "memory_2gib",
                killed,
                format!("child {:?}; last line: {last}", o.status),
            );
        }
        Err(e) => record(
            &mut out,
            "memory_2gib",
            true,
            format!("could not start child: {e}"),
        ),
    }

    write_result(&out);
}
