use std::os::unix::prelude::CommandExt;
use std::process::{Command, exit};

use nix::sys::personality::Persona;
use nix::sys::{ptrace, personality};
use nix::unistd::{ fork, ForkResult, Pid };

fn main() {
    match unsafe {fork()} {
        Ok(ForkResult::Child) => {
            run_child();
        }
        Ok(ForkResult::Parent { child }) => {
            run_parent(child);
        }
        Err(err) => {
            panic!("[main] fork() failed: {}", err);
        }
    }
}

fn run_child() {

    println!("Hello from child");

    ptrace::traceme().unwrap();

    let pers = personality::get().unwrap();
    assert!(!pers.contains(Persona::ADDR_NO_RANDOMIZE));
    personality::set(pers | Persona::ADDR_NO_RANDOMIZE).unwrap();
}

fn run_parent(child: Pid) {
    println!("Hello from parent. Child pid = {}", child);

    Command::new("./dummy").exec();

    exit(0);
}
