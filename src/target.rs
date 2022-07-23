use std::collections::HashMap;
use std::ffi;
use std::mem::size_of;

use nix::libc;
use nix::sys::personality::Persona;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::{personality, ptrace};
use nix::unistd::{fork, ForkResult, Pid};

use crate::debuginfo::DebugInfo;
use crate::util::get_base_address;

pub struct Breakpoint {
    pub address: usize,
    pub idx: u32,
    original_byte: u8,
    /// Set to true if this bp was hit on SIGTRAP.
    set_on_continue: bool,
}

impl Breakpoint {
    pub fn pprint(&self, debug_info: &DebugInfo, base_address: usize) {
        let location = debug_info.get_function_at_addr(self.address - base_address);
        if let Some(location) = location {
            print!("Breakpoint {} at {}", self.idx, location);
        } else {
            print!("Breakpoint {} at {:#x}", self.idx, self.address);
        }
    }
}

pub struct Target {
    _executable_path: String,
    pid: Pid,
    base_address: usize,
    next_bp_num: u32,
    pub breakpoints: HashMap<usize, Breakpoint>,
    debug_info: DebugInfo,
}

impl Target {
    pub fn create(target: &str) -> Result<Target, nix::Error> {
        let pid = Target::fork_child(target)?;
        let debug_info = DebugInfo::create(target);
        Ok(Target {
            _executable_path: String::from(target),
            pid,
            base_address: get_base_address(pid).unwrap_or(0),
            next_bp_num: 0,
            breakpoints: HashMap::new(),
            debug_info,
        })
    }

    pub fn print_registers(&self) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid)?;
        println!("{:?}", regs);
        Ok(())
    }

    pub fn step(&self) -> Result<(), nix::Error> {
        ptrace::step(self.pid, None)?;
        Ok(())
    }

    pub fn cont(&mut self) -> Result<(), nix::Error> {
        let rip = ptrace::getregs(self.pid).expect("Could not get RIP.").rip as usize;
        let mut need_single_step = false;
        for (addr, _) in self
            .breakpoints
            .iter()
            .filter(|&(_, bp)| bp.set_on_continue)
        {
            self.write_byte(*addr, 0xcc)?;
            need_single_step = *addr == rip;
        }
        for (_, bp) in self.breakpoints.iter_mut() {
            bp.set_on_continue = false;
        }

        if need_single_step {
            self.step()?;
            wait()?;
        }

        ptrace::cont(self.pid, None)
    }

    pub fn wait(&mut self) -> Result<WaitStatus, nix::Error> {
        let wait_status = wait()?;
        if let WaitStatus::Stopped(_, Signal::SIGTRAP) = wait_status {
            let mut regs = ptrace::getregs(self.pid).expect("Could not get registers.");
            regs.rip -= 1; // set rip to the breakpoint address

            if let Some(breakpoint) = self.breakpoints.get_mut(&(regs.rip as usize)) {
                // we hit our own breakpoint --> restore byte (after this if) and mark for re-setting (here).
                breakpoint.set_on_continue = true;
                ptrace::setregs(self.pid, regs).expect("Could not set registers.");
            } else {
                // not our breakpoint, this is executed after step() for example.
            };

            if let Some(breakpoint) = self.breakpoints.get(&(regs.rip as usize)) {
                self.restore_breakpoint(breakpoint.address)?;
            }
        }

        Ok(wait_status)
    }

    pub fn set_breakpoint(&mut self, addr: usize) -> Result<(), nix::Error> {
        if let Some(bp) = self.breakpoints.get(&addr) {
            println!("Breakpoint {} at {:#x} already exists.", bp.idx, addr);
        } else {
            let bp_idx = self.next_bp_num;
            self.next_bp_num += 1;

            let old_byte = self.write_byte(addr, 0xcc)?;

            self.breakpoints.insert(
                addr,
                Breakpoint {
                    address: addr,
                    original_byte: old_byte,
                    idx: bp_idx,
                    set_on_continue: false,
                },
            );
            let breakpoint = self.breakpoints.get(&addr).unwrap();
            breakpoint.pprint(&self.debug_info, self.base_address);
            println!(" created.");
        }
        Ok(())
    }

    pub fn delete_breakpoint(&mut self, addr: usize) -> Result<(), nix::Error> {
        if self.restore_breakpoint(addr)? {
            let bp = self
                .breakpoints
                .remove(&addr)
                .expect("Breakpoint should exist in map?");
            bp.pprint(&self.debug_info, self.base_address);
            println!(" deleted.");
        }
        Ok(())
    }

    fn restore_breakpoint(&mut self, addr: usize) -> Result<bool, nix::Error> {
        if let Some(bp) = self.breakpoints.get(&addr) {
            self.write_byte(addr, bp.original_byte).ok();
            Ok(true)
        } else {
            println!("No restore info at address {:#x} found.", addr);
            Ok(false)
        }
    }

    pub fn list_breakpoints(&self) {
        for (_, breakpoint) in &self.breakpoints {
            println!("Breakpoint {} at {:#x}", breakpoint.idx, breakpoint.address);
        }
    }

    fn align_addr_to_word(&self, addr: usize) -> usize {
        addr & (-(size_of::<usize>() as isize) as usize)
    }

    fn write_byte(&self, addr: usize, byte: u8) -> Result<u8, nix::Error> {
        let aligned_addr = self.align_addr_to_word(addr);
        let byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid, aligned_addr as ptrace::AddressType)? as u64;
        let orig_byte = (word >> 8 * byte_offset) & 0xff;
        let masked_word = word & !(0xff << 8 * byte_offset);
        let updated_word = masked_word | ((byte as u64) << 8 * byte_offset);
        // println!(
        //     "Replace at {:#018x}: {:#18x} -> {:#18x}",
        //     aligned_addr, word, updated_word
        // );
        unsafe {
            ptrace::write(
                self.pid,
                aligned_addr as ptrace::AddressType,
                updated_word as *mut std::ffi::c_void,
            )?;
        }
        Ok(orig_byte as u8)
    }

    pub fn kill(&self) -> Result<(), nix::errno::Errno> {
        ptrace::kill(self.pid)
    }

    fn fork_child(target: &str) -> Result<Pid, nix::Error> {
        match unsafe { fork() }? {
            ForkResult::Child => {
                bootstrap_target_process(target);
                Ok(Pid::from_raw(0)) // not used by anyone
            }
            ForkResult::Parent { child } => Ok(child),
        }
    }
}

/// Do ptrace(TRACEME) then execve
fn bootstrap_target_process(target: &str) {
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
        let ret = libc::execve(c_target, argv.as_ptr(), env.as_ptr());
        println!("Programm returned {}", ret);
    }
}
