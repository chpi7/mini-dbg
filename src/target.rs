use std::collections::HashMap;
use std::ffi::{self, c_void};
use std::mem::size_of;

use nix::libc;
use nix::sys::personality::Persona;
use nix::sys::signal::Signal;
use nix::sys::wait::{wait, WaitStatus};
use nix::sys::{personality, ptrace};
use nix::unistd::{fork, ForkResult, Pid};

use crate::debuginfo::{DebugInfo, Location};
use crate::util::{add_offset, get_base_address};

pub struct Breakpoint {
    pub address: usize,
    pub idx: u32,
    original_byte: u8,
    /// Set to true if this bp was hit on SIGTRAP.
    set_on_continue: bool,
}

impl Breakpoint {
    pub fn pprint(&self, debug_info: &DebugInfo, base_address: usize) {
        let location = debug_info.get_location_at_addr(self.address - base_address);
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
    pub base_address: usize,
    next_bp_num: u32,
    pub breakpoints: HashMap<usize, Breakpoint>,
    pub debug_info: DebugInfo,
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

    pub fn get_current_location(&self) -> Option<Location> {
        self.debug_info
            .get_location_at_addr(self.get_virtual_address())
    }

    pub fn get_virtual_address(&self) -> usize {
        let regs = ptrace::getregs(self.pid).expect("Could not get registers.");
        (regs.rip as usize) - self.base_address
    }

    /// Retrieve the current canonical frame address (CFA).
    /// See DWARF standarf v4, section 6.4 Call Frame Information.
    pub fn get_cfa(&self) -> usize {
        // CFA is the sp at the call site of the current function
        // In the preamble:     mov    rbp,rsp
        // rbp is the old stack pointer after call
        // ret would pop into rip --> + 8 bytes
        // the old sp should be rbp + 8?

        let regs = ptrace::getregs(self.pid).expect("Could not get registers.");
        let cfa = regs.rbp + 16;

        cfa as usize
    }

    pub fn get_offset_from_cfa(&self, rbp: usize, offset: isize) -> usize {
        // Breakpoint 1, complex_function (a=21845, b=1431654909) at segfault.c:1
        // 1       int complex_function(int a, int b) {
        // (gdb) s
        // 2           return 2*a + b;
        // (gdb) s
        // 3       }
        // (gdb) i f
        // Stack level 0, frame at 0x7fffffffddf0:
        // rip = 0x555555555142 in complex_function (segfault.c:3); saved rip = 0x555555555198
        // called by frame at 0x7fffffffde20
        // source language c.
        // Arglist at 0x7fffffffddd8, args: a=1, b=2
        // Locals at 0x7fffffffddd8, Previous frame's sp is 0x7fffffffddf0                  <---- this is RBP+16 == CFA
        // Saved registers:
        // rbp at 0x7fffffffdde0, rip at 0x7fffffffdde8
        // (gdb) i r rbp rsp
        // rbp            0x7fffffffdde0      0x7fffffffdde0
        // rsp            0x7fffffffdde0      0x7fffffffdde0
        // (gdb) x/1dw $rbp+16-20                                                           <---- -20 fbreg offset
        // 0x7fffffffdddc: 1
        // (gdb) x/1dw $rbp+16-24                                                           <---- -24 fbreg offset
        // 0x7fffffffddd8: 2

        let cfa = rbp + 16;
        let address = add_offset(cfa, offset);
        // println!(
        //     "rbp {:#18x}\tcfa {:#18x}\toffset {} = {:#18x}",
        //     rbp, cfa, offset, address
        // );
        address
    }

    pub fn read_bytes(&self, addr: usize, _amount: usize) -> Result<Vec<u8>, nix::Error> {
        let aligned_addr = self.align_addr_to_word(addr);
        let _byte_offset = addr - aligned_addr;
        let word = ptrace::read(self.pid, aligned_addr as ptrace::AddressType)? as u64;
        println!("{:#034x}", word);
        let bytes = word.to_le_bytes();
        for byte in bytes {
            println!("{:#06x}", byte);
        }

        Ok(vec![])
    }

    pub fn print_current_source_line(&self, range: usize) {
        let addr = self.get_virtual_address();
        self.debug_info.print_code_at_addr(addr, range)
    }

    pub fn print_backtrace(&self) {
        let regs = ptrace::getregs(self.pid).expect("Could not get registers.");

        let mut rbp = regs.rbp;
        let mut rip = regs.rip;
        let mut i = 0;

        println!("Backtrace:");
        while rbp != 0x0 {
            if let Some(location) = self
                .debug_info
                .get_location_at_addr(rip as usize - self.base_address)
            {
                println!("{} {}", i, location);

                // switch to get function by address
                if let Some(function) = self
                    .debug_info
                    .dwarf_info
                    .get_function_by_name(&location.function_name)
                {
                    for formal in &function.formal_parameters {
                        self.print_local(
                            rbp as usize,
                            formal.fbreg_offset as isize,
                            formal.t,
                            &formal.name,
                        );
                    }
                    for local in &function.local_variables {
                        self.print_local(
                            rbp as usize,
                            local.fbreg_offset as isize,
                            local.t,
                            &local.name,
                        );
                    }
                }
            } else {
                break;
            }
            rip = ptrace::read(self.pid, (rbp + 8) as *mut c_void).expect("Could not read next rip")
                as u64;
            rbp =
                ptrace::read(self.pid, rbp as *mut c_void).expect("Could not read next rbp") as u64;
            i += 1;
        }
    }

    fn print_local(&self, rbp: usize, fbreg_offset: isize, t: usize, name: &str) {
        let val_addr = self.get_offset_from_cfa(rbp as usize, fbreg_offset as isize);
        let val_size = self
            .debug_info
            .dwarf_info
            .get_type_byte_size(t)
            .expect("Could not get type byte size") as u32;
        let val = ptrace::read(self.pid, val_addr as *mut c_void).unwrap_or(0) as u64;
        let val_mask = (1 as u64).checked_shl(8 * val_size).map(|v| v - 1).unwrap_or(!0);
        let val = val & val_mask;
        println!("{} = {:#18x}", name, val);
    }

    pub fn print_registers(&self) -> Result<(), nix::Error> {
        let regs = ptrace::getregs(self.pid)?;
        let rbp_pointee = ptrace::read(self.pid, regs.rbp as *mut c_void).ok();
        let rsp_pointee = ptrace::read(self.pid, regs.rsp as *mut c_void).ok();

        println!("rax\t{:#18x}", regs.rax);
        println!("rbx\t{:#18x}", regs.rbx);
        println!("rcx\t{:#18x}", regs.rcx);
        println!("rdx\t{:#18x}", regs.rdx);

        println!("rsi\t{:#18x}", regs.rsi);
        println!("rdi\t{:#18x}", regs.rdi);
        print!("rbp\t{:#18x}", regs.rbp);
        if let Some(rbp_pointee) = rbp_pointee {
            println!("\t-> {:#18x}", rbp_pointee);
        } else {
            println!("\t-> <invalid>");
        }
        print!("rsp\t{:#18x}", regs.rsp);
        if let Some(rsp_pointee) = rsp_pointee {
            println!("\t-> {:#18x}", rsp_pointee);
        } else {
            println!("\t-> <invalid>");
        }

        println!("r8\t{:#18x}", regs.r8);
        println!("r9\t{:#18x}", regs.r9);
        println!("r10\t{:#18x}", regs.r10);
        println!("r11\t{:#18x}", regs.r11);
        println!("r12\t{:#18x}", regs.r12);
        println!("r13\t{:#18x}", regs.r13);
        println!("r14\t{:#18x}", regs.r14);
        println!("r15\t{:#18x}", regs.r15);

        Ok(())

        // println!("{:?}", regs);
        // Ok(())
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
            println!("");
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
            breakpoint.pprint(&self.debug_info, self.base_address);
            println!("");
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
