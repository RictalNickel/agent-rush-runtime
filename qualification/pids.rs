fn main() {
    let mut children=Vec::new();let mut denied=false;
    for _ in 0..128 {match std::process::Command::new("/bin/sleep").arg("10").spawn(){Ok(child)=>children.push(child),Err(_)=>{denied=true;break;}}}
    let count=children.len();
    for mut child in children {let _=child.kill();let _=child.wait();}
    assert!(denied && count<64,"PID cap did not stop process creation");
    std::fs::write("/work/artifacts/result.json",format!("{{\"pids_limited\":true,\"children\":{count}}}")).unwrap();
}
