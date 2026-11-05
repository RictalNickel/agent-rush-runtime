use std::io::Write;
// Grows 32 MiB at a time toward 768 MiB. Started at 256 MiB with a 512 MiB ceiling, it must be
// scaled past 256 MiB live and then killed at the ceiling, never reaching the last line.
fn main() {
    let mut held: Vec<Vec<u8>> = Vec::new();
    for step in 1..=24u32 {
        let mut chunk = vec![0u8; 32 * 1024 * 1024];
        for i in (0..chunk.len()).step_by(4096) {
            chunk[i] = step as u8;
        }
        held.push(chunk);
        println!("holding {} MiB", step * 32);
        std::io::stdout().flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    std::hint::black_box(&held);
    println!("allocation escaped");
}
