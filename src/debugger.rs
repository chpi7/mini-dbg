use nix::libc;
use nix::sys::personality::Persona;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::{personality, ptrace};
use nix::unistd::{fork, ForkResult, Pid};
use std::ffi;

pub struct Debugger {
    target: String,
    child_pid: Option<Pid>,
}

impl Debugger {
    pub fn create(target: String) -> Debugger {
        Debugger {
            target: target,
            child_pid: Option::None,
        }
    }

    pub fn run(&mut self) -> Result<(), ()> {
        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                self.run_child(&self.target);
            }
            Ok(ForkResult::Parent { child }) => {
                self.child_pid = Some(child);
                self.run_repl(child);
            }
            Err(err) => {
                panic!("[main] fork() failed: {}", err);
            }
        }

        Ok(())
    }

    fn run_repl(&self, child: Pid) {
        wait().expect("wait for child failed");
        println!("child pid = {}", child);

        if let Some(pid) = self.child_pid {
            ptrace::cont(pid, None).expect("Failed continue process");
        }

        loop {
            match wait() {
                Ok(WaitStatus::Stopped(_pid, sig_num)) => match sig_num {
                    Signal::SIGTRAP => {
                        println!("Got sigtrap");
                    }

                    Signal::SIGSEGV => {
                        println!("Got sigsegv");
                        break;
                    }

                    _ => {
                        println!("Got other signal {}", sig_num);
                        break;
                    }
                },

                Ok(WaitStatus::Exited(_pid, exit_status)) => {
                    println!("Child exited with status {}", exit_status);
                    break;
                }

                Ok(status) => {
                    println!("Received status {:?}", status);
                }

                Err(err) => {
                    println!("Got error {:?}", err);
                }
            }
        }
    }

    fn run_child(&self, target: &str) {
        ptrace::traceme().expect("traceme failed");

        let pers = personality::get().unwrap();
        assert!(!pers.contains(Persona::ADDR_NO_RANDOMIZE));
        let res = personality::set(pers | Persona::ADDR_NO_RANDOMIZE);
        match res {
            Ok(..) => {}
            Err(err) => {
                println!("Could not set ADDR_NO_RANDOMIZE. Errno = {}", err);
            }
        }

        let c_target_hold = ffi::CString::new(target.clone()).unwrap();
        let c_target = c_target_hold.as_ptr();

        let mut argv: Vec<*const i8> = Vec::new();
        let mut env: Vec<*const i8> = Vec::new();

        argv.push(c_target);
        argv.push(std::ptr::null());

        env.push(std::ptr::null());

        unsafe {
            let ret = libc::execv(c_target, argv.as_ptr());
            println!("Programm returned {}", ret);
        }
    }
}
