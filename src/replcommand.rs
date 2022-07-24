use std::io::{stdin, stdout, Write};

#[derive(Debug)]
pub enum ReplCommand {
    Start,
    Continue,
    Exit,
    Unknown,
    SetBp(usize),
    DeleteBp(usize),
    ListBps,
    GetRegs,
    SingleStep,
    Backtrace,
    Frame,
}

/// Very sophisticated command parser.
pub fn get_command() -> ReplCommand {
    let mut input = String::new();

    print!("> ");
    stdout().flush().unwrap();
    stdin().read_line(&mut input).expect("Could not read line");

    match input.trim() {
        "cont" => ReplCommand::Continue,
        "c" => ReplCommand::Continue,
        "r" => ReplCommand::Continue,
        "start" => ReplCommand::Start,
        "exit" => ReplCommand::Exit,
        "e" => ReplCommand::Exit,
        "regs" => ReplCommand::GetRegs,
        "s" => ReplCommand::SingleStep,
        "lsb" => ReplCommand::ListBps,
        "back" => ReplCommand::Backtrace,
        "frame" => ReplCommand::Frame,
        "f" => ReplCommand::Frame,
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
                    let parsed_addr =
                        parse_address(parts[2]).expect("Address could not be parsed.");
                    bp_type(parsed_addr)
                }
            } else {
                ReplCommand::Unknown
            }
        }
    }
}

fn parse_address(addr: &str) -> Option<usize> {
    let addr_without_0x = if addr.to_lowercase().starts_with("0x") {
        &addr[2..]
    } else {
        &addr
    };
    usize::from_str_radix(addr_without_0x, 16).ok()
}
