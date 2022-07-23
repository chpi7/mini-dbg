use nix::sys::signal::Signal;
use nix::sys::wait::WaitStatus;

use crate::replcommand::ReplCommand;
use crate::target::Target;

pub struct Debugger {
    target_process: Option<Target>,
    target_path: String,
}

impl Debugger {
    pub fn create(target_path: String) -> Debugger {
        Debugger {
            target_process: None,
            target_path,
        }
    }

    pub fn run(&mut self) -> Result<(), ()> {
        self.run_repl();

        Ok(())
    }

    fn run_repl(&mut self) {
        loop {
            let cmd = crate::replcommand::get_command();

            if let ReplCommand::Exit = cmd {
                if let Some(t) = &mut self.target_process {
                    println!("Killing child.");
                    t.kill().expect("Could not kill child.");
                }
                break;
            } else {
                self.handle_command(&cmd);

                let should_wait = match cmd {
                    ReplCommand::Continue => true,
                    ReplCommand::SingleStep => true,
                    _ => false
                };

                if should_wait {
                    if let Some(target) = &mut self.target_process {
                        let wait_status = target.wait().expect("Error during wait.");
                        match wait_status {
                            WaitStatus::Exited(_, exit_code) => {
                                println!("Program exited with code {}", exit_code);
                                self.target_process = None;
                            }
                            // WaitStatus::Stopped(_, Signal::SIGTRAP) => {
                            //     println!("Hit trap");
                            // }
                            WaitStatus::Stopped(_, Signal::SIGSEGV) => {
                                println!("Segfault");
                            }
                            other => {
                                println!("WaitStatus = {:?}", other);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Returns true if we should run the child, and false if not.
    fn handle_command(&mut self, cmd: &ReplCommand) {
        match cmd {
            ReplCommand::Start => {
                let target_process = Target::create(&self.target_path)
                    .expect("Could not instantiate target process.");
                self.target_process = Some(target_process);
            }
            ReplCommand::Continue => {
                if let Some(target) = &mut self.target_process {
                    target.cont().expect("Error during continue call.");
                }
            }
            ReplCommand::SetBp(addr) => {
                if let Some(target) = &mut self.target_process {
                    target
                        .set_breakpoint(*addr)
                        .expect("Error while setting breakpoint.");
                }
            }
            ReplCommand::DeleteBp(addr) => {
                if let Some(target) = &mut self.target_process {
                    target
                        .delete_breakpoint(*addr)
                        .expect("Error while deleting breakpoint.");
                }
            }
            ReplCommand::ListBps => {
                if let Some(target) = &self.target_process {
                    target.list_breakpoints();
                }
            }
            ReplCommand::GetRegs => {
                if let Some(target) = &self.target_process {
                    target
                        .print_registers()
                        .expect("Error during continue call.");
                }
            }
            ReplCommand::SingleStep => {
                if let Some(target) = &self.target_process {
                    target.step().expect("Error during step call.");
                }
            }
            _ => {
                println!("Unhandled command: {:?}", cmd);
            }
        }
    }
}
