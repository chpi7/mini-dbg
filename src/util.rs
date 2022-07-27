use std::{fs::File, io::BufRead};

use nix::unistd::Pid;
use std::io::BufReader;

pub fn get_base_address(pid: Pid) -> Result<usize, ()> {
    let maps = format!("/proc/{}/maps", pid);
    let file = File::open(&maps).expect(format!("Could not open {}", &maps).as_str());
    let reader = BufReader::new(file);

    let line = reader.lines().next().unwrap().unwrap();
    let parts: Vec<&str> = line.split("-").collect();

    Ok(usize::from_str_radix(parts[0], 16).unwrap())
}

pub fn add_offset(address: usize, offset: isize) -> usize {
    if offset < 0 {
        address.checked_sub(offset.wrapping_abs() as usize).expect("Error during add_offset")
    } else {
        address.checked_add(offset as usize).expect("Error during add_offset")
    }
}
