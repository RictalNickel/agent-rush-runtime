use std::io::Write;
fn main(){println!("allocating");std::io::stdout().flush().unwrap();let mut bytes=vec![7u8;384*1024*1024];for b in &mut bytes{*b=std::hint::black_box(*b+1);}std::hint::black_box(&bytes);println!("allocation escaped");}
