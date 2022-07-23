use nix::libc;
use nix::sys::personality::Persona;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::{personality, ptrace};
use nix::unistd::{fork, ForkResult, Pid};
use std::collections::{HashMap, HashSet};
use std::ffi;
use std::io::{stdin, stdout, Write};
use std::mem::size_of;

use crate::debuginfo::DebugInfo;
use crate::util::get_base_address;

pub struct Debugger {
    target: String,
    child_pid: Option<Pid>,
    breakpoints: HashMap<usize, Breakpoint>,
    breakpoint_restore_info: HashMap<usize, u8>,
    breakpoints_outstanding: HashSet<usize>,
    debug_info: DebugInfo,
}

#[derive(Debug)]
enum Breakpoint {
    Address(usize),
}

#[derive(Debug)]
enum ReplCommand {
    Start,
    Continue,
    Exit,
    Unknown,
    SetBp(usize),
    DeleteBp(usize),
    GetRegs,
    SingleStep,
}

impl Debugger {
    pub fn create(target: String) -> Debugger {
        let debug_info = DebugInfo::create(&target);
        Debugger {
            target: target,
            child_pid: Option::None,
            breakpoints: HashMap::new(),
            breakpoint_restore_info: HashMap::new(),
            breakpoints_outstanding: HashSet::new(),
            debug_info,
        }
    }

    pub fn run(&mut self) -> Result<(), ()> {

        self.run_repl();

        Ok(())
    }

    fn run_repl(&mut self) {
        loop {
            let cmd = self.get_command();

            if let ReplCommand::Exit = cmd {
                if let Some(pid) = self.child_pid {
                    ptrace::kill(pid).expect("Could not kill child process.");
                }
                return;
            }

            if self.handle_command(cmd) {
                match self.wait_for_child() {
                    Ok(_) => {
                        if let Some(pid) = self.child_pid {
                            let rip = ptrace::getregs(pid).unwrap().rip as usize;
                            let base = get_base_address(pid).expect("Could not get base address");
                            self.debug_info.get_location(rip - base);
                        }
                    }
                    Err(err) => {
                        println!("Got error {}", err);
                        break;
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
            "cont" => ReplCommand::Continue,
            "c" => ReplCommand::Continue,
            "start" => ReplCommand::Start,
            "exit" => ReplCommand::Exit,
            "e" => ReplCommand::Exit,
            "regs" => ReplCommand::GetRegs,
            "r" => ReplCommand::GetRegs,
            "s" => ReplCommand::SingleStep,
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
                        let parsed_addr = self
                            .parse_address(parts[2])
                            .expect("Address could not be parsed.");
                        bp_type(parsed_addr)
                    }
                } else {
                    ReplCommand::Unknown
                }
            }
        }
    }

    fn parse_address(&self, addr: &str) -> Option<usize> {
        let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
            &addr[2..]
        } else {
            &addr
        };
        usize::from_str_radix(addr_without_0x, 16).ok()
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
                    let rip = ptrace::getregs(pid).expect("Could not read registers").rip;
                    let addresses_to_set: Vec<usize> = (&self.breakpoints_outstanding)
                        .into_iter()
                        .map(|v| (*v))
                        .collect();
                    self.breakpoints_outstanding.clear();
                    
                    let mut perform_one_singlestep = false;
                    for addr in addresses_to_set {
                        perform_one_singlestep |= rip == (addr as u64);
                        self.set_breakpoint(addr)
                            .expect("Could not re-set breakpoint.");
                    }
                    if perform_one_singlestep {
                        println!("Jump over re-set bp");
                        ptrace::step(pid, None).expect("Could not single step");
                        wait().expect("wait for child while stepping over re-set bp failed.");
                    }
                    ptrace::cont(pid, None).expect("Failed continue process");

                    true
                } else {
                    false
                }
            }

            ReplCommand::SetBp(addr) => {
                self.set_breakpoint(addr).unwrap();
                false
            }

            ReplCommand::DeleteBp(addr) => {
                self.restore_breakpoint(addr).unwrap();
                false
            }

            ReplCommand::SingleStep => {
                ptrace::step(self.child_pid.unwrap(), None).expect("Could not single step");
                false
            }

            ReplCommand::GetRegs => {
                let regs =
                    ptrace::getregs(self.child_pid.unwrap()).expect("Could not read registers");
                println!("{:?}", regs);
                false
            }

            _ => {
                println!("Unhandled command {:?}", cmd);
                false
            }
        }
    }

    fn set_breakpoint(&mut self, addr: usize) -> Result<(), nix::Error> {
        let old_byte = self.write_byte(addr, 0xcc)?;
        println!("Breakpoint at {:#x} added.", addr);
        self.breakpoints.insert(addr, Breakpoint::Address(addr));
        self.breakpoint_restore_info.insert(addr, old_byte);

        Ok(())
    }

    fn restore_breakpoint(&mut self, addr: usize) -> Result<bool, nix::Error> {
        if let Some(old_byte) = self.breakpoint_restore_info.get(&addr) {
            self.write_byte(addr, *old_byte).ok();
            Ok(true)
        } else {
            println!("No restore info at address {:#x} found.", addr);
            Ok(false)
        }
    }

    fn align_addr_to_word(&self, addr: usize) -> usize {
        addr & (-(size_of::<usize>() as isize) as usize)
    }

    fn write_byte(&self, addr: usize, byte: u8) -> Result<u8, nix::Error> {
        let aligned_addr = self.align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word =
            ptrace::read(self.child_pid.unwrap(), aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((byte as u64) << 8 * byte_offset);
        // println!(
        //     "Replace at {:#018x}: {:#18x} -> {:#18x}",
        //     aligned_addr, word, updated_word
        // );
        unsafe {
            ptrace::write(
                self.child_pid.unwrap(),
                aligned_addr as ptrace::AddressType,
                updated_word as *mut std::ffi::c_void,
            )?;
        }
        Ok(orig_byte as u8)
    }

    fn wait_for_child(&mut self) -> Result<(), nix::Error> {
        if let None = self.child_pid {
            // println!("Child not running. Start with 'run'");
            return Ok(());
        }

        match wait()? {
            WaitStatus::Stopped(_pid, sig_num) => match sig_num {
                Signal::SIGTRAP => {
                    println!("Got SIGTRAP");

                    // restore bp so user does not see it
                    let mut regs =
                        ptrace::getregs(self.child_pid.unwrap()).expect("Could not get registers");
                    let rip = (regs.rip - 1) as usize;

                    // only mark for restoring if it was out own breakpoint
                    if self.restore_breakpoint(rip)? {
                        // save bp for restoring on cont
                        self.breakpoints_outstanding.insert(rip);

                        // wind back rip by one
                        regs.rip -= 1;
                        ptrace::setregs(self.child_pid.unwrap(), regs)
                            .expect("Could not write registers");
                    }
                }

                Signal::SIGSEGV => {
                    println!("Got SIGSEGV");
                }

                signal => {
                    println!("Got signal {}", signal);
                }
            },

            WaitStatus::Exited(_, exit_status) => {
                println!("Child exited with status {}", exit_status);
                self.child_pid = None;
            }

            status => {
                println!("Received status {:?}", status);
            }
        }

        Ok(())
    }

    fn start_child(&mut self) -> Result<(), ()> {
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
