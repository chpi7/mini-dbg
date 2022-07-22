use nix::libc;
use nix::sys::personality::Persona;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::{personality, ptrace};
use nix::unistd::{fork, ForkResult, Pid};
use std::collections::HashMap;
use std::ffi;
use std::io::{stdin, stdout, Write};

pub struct Debugger {
    target: String,
    child_pid: Option<Pid>,
    breakpoints: HashMap<u64, Breakpoint>
}

#[derive(Debug)]
enum Breakpoint {
    Address(u64),
    Name(String)
}

#[derive(Debug)]
enum ReplCommand {
    Start,
    Continue,
    Exit,
    Unknown,
    SetBp(Breakpoint),
    DeleteBp(Breakpoint),
    GetRegs,
    SingleStep,
}

impl Debugger {
    pub fn create(target: String) -> Debugger {
        Debugger {
            target: target,
            child_pid: Option::None,
            breakpoints: HashMap::new(),
        }
    }

    pub fn run(&mut self) -> Result<(), ()> {
        
        println!("mini-dbg v0.1");

        self.run_repl();

        Ok(())
    }

    fn run_repl(&mut self) {
        loop {
            
            let cmd = self.get_command();
            
            if let ReplCommand::Exit = cmd {
                return
            }
            
            if self.handle_command(cmd) {
                match self.wait_for_child() {
                    Ok(_) => {}
                    Err(err) => {
                        println!("Got error {}", err);
                        break
                    }
                }
            }
        }
    }

    /// Very sophisticated command parser.
    fn get_command(&self) -> ReplCommand {
        let mut input = String::new();

        print!("> ");
        stdout().flush().unwrap();
        stdin().read_line(&mut input).expect("Could not read line");
        
        match input.trim() {
            "cont" => { ReplCommand::Continue }
            "start" => { ReplCommand::Start }
            "exit" => { ReplCommand::Exit }
            "regs" => { ReplCommand::GetRegs }
            "s" => { ReplCommand::SingleStep }
            _ => {
                if input.starts_with("bp") {
                    let parts: Vec<&str> = input.trim().split(' ').collect();
                    if parts.len() < 3 {
                        println!("Too few arguments in bp command");
                        ReplCommand::Unknown
                    } else {
                        let bp_type = if parts[1] == "set" {
                            ReplCommand::SetBp
                        } else {
                            ReplCommand::DeleteBp
                        };
                        parts[2].parse::<u64>().map_or_else(
                            |_| bp_type(Breakpoint::Name(String::from(parts[2]))), 
                            |v| bp_type(Breakpoint::Address(v))
                        )
                    }
                } else {
                    ReplCommand::Unknown
                }
            }
        }
    }

    /// Returns true if we should run the child, and false if not.
    fn handle_command(&mut self, cmd: ReplCommand) -> bool {

        match cmd {
            ReplCommand::Start => {
                self.start_child().expect("could not start child process");
                false
            }

            ReplCommand::Continue => {
                if let Some(pid) = self.child_pid {
                    ptrace::cont(pid, None).expect("Failed continue process");
                    true
                } else {
                    false
                }
            }

            ReplCommand::SetBp(bp) => {
                self.set_breakpoint(bp).unwrap();
                false
            }

            ReplCommand::DeleteBp(bp) => {
                self.delete_breakpoint(bp).unwrap();
                false
            }

            ReplCommand::SingleStep => {
                ptrace::step(self.child_pid.unwrap(), None).expect("Could not single step");
                false
            }

            ReplCommand::GetRegs => {
                let regs = ptrace::getregs(self.child_pid.unwrap()).expect("Could not read registers");
                println!("{:?}", regs);
                false
            }

            _ => {
                println!("Unhandled command {:?}", cmd);
                false
            }
        }

    }

    fn set_breakpoint(&mut self, bp: Breakpoint) -> Result<(),()> {        
        let addr = match bp {
            Breakpoint::Address(a) => { Some(a) }
            Breakpoint::Name(ref n) => { self.get_symbol_address(n) }
        };

        if let Some(addr) = addr {
            println!("{:?} added.", bp);

            self.breakpoints.insert(addr, bp);
            Ok(())
        } else {
            println!("Breakpoint could not be resolved to address. {:?}", bp);
            Err(())
        }
    }

    fn delete_breakpoint(&mut self, bp: Breakpoint) -> Result<(),()> {
        let addr = match bp {
            Breakpoint::Address(a) => { Some(a) }
            Breakpoint::Name(ref n) => { self.get_symbol_address(n) }
        };

        if let Some(ref addr) = addr {
            if self.breakpoints.contains_key(addr) {
                self.breakpoints.remove(addr);
                println!("{:?} deleted.", bp);
            } else {
                println!("Breakpoint {:?} does not exist", bp);
            }
            Ok(())
        } else {
            println!("Breakpoint could not be resolved to address. {:?}", bp);
            Err(())
        }
    }

    fn get_symbol_address(&self, _name: &str) -> Option<u64> {
        None
    }

    fn wait_for_child(&self) -> Result<(), &str> {

        if let None = self.child_pid {
            // println!("Child not running. Start with 'run'");
            return Ok(());
        }

        match wait() {
            Ok(WaitStatus::Stopped(_pid, sig_num)) => match sig_num {
                Signal::SIGTRAP => {
                    Ok(())
                }

                Signal::SIGSEGV => {
                    Err("Got SIGSEGV")
                }

                _ => {
                    Ok(())
                }
            },

            Ok(WaitStatus::Exited(_pid, exit_status)) => {
                println!("Child exited with status {}", exit_status);
                Ok(())
            }

            Ok(status) => {
                println!("Received status {:?}", status);
                Ok(())
            }

            Err(err) => {
                println!("Got error {:?}", err);
                Err(err.desc())
            }
        }
    }

    fn start_child(&mut self) -> Result<(),()> {

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                run_child(&self.target);
            }
            Ok(ForkResult::Parent { child }) => {
                self.child_pid = Some(child);
                println!("child pid = {}", child);
            }
            Err(err) => {
                panic!("[main] fork() failed: {}", err);
            }
        }

        wait().expect("wait for child failed");

        Ok(())
    }
}

fn run_child(target: &str) {
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