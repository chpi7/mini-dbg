use std::os::unix::prelude::CommandExt;
use std::process::{Command, exit};

use nix::sys::personality::Persona;
use nix::sys::{ptrace, personality};
use nix::unistd::{ fork, ForkResult, Pid };

pub struct Debugger {
    target: String
}

impl Debugger {
    pub fn create(target: String) -> Debugger {
        Debugger { target: target }
    }

    pub fn run(&self) -> Result<(), ()> {

        match unsafe {fork()} {
            Ok(ForkResult::Child) => {
                run_child(&self.target);
            }
            Ok(ForkResult::Parent { child }) => {
                self.run_parent(child);
            }
            Err(err) => {
                panic!("[main] fork() failed: {}", err);
            }
        }

        Ok(())
    }

    fn run_parent(&self, child: Pid) {
        println!("Hello from parent. Child pid = {}", child);
    
        exit(0);
    }
}

fn run_child(target: &String) {

    println!("Hello from child");

    ptrace::traceme().unwrap();

    // let pers = personality::get().unwrap();
    // assert!(!pers.contains(Persona::ADDR_NO_RANDOMIZE));
    // let res = personality::set(pers | Persona::ADDR_NO_RANDOMIZE);
    // match res {
    //     Ok(..) => {}
    //     Err(err) => {
    //         println!("Could not set ADDR_NO_RANDOMIZE. Errno = {}", err)
    //     } 
    // }
    Command::new("a.out").exec();
    println!("Test 1234");
}