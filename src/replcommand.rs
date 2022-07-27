use std::io::{stdin, stdout, Write};

#[derive(Debug)]
pub enum ReplCommand {
    Start,
    Continue,
    Exit,
    Unknown,
    SetBp(usize),
    SetBpName(String),
    DeleteBp(usize),
    ListBps,
    GetRegs,
    SingleStep,
    Backtrace,
    Frame,
    GetVar
}

/// Very sophisticated command parser.
pub fn get_command() -> ReplCommand {
    let mut input = String::new();

    print!("> ");
    stdout().flush().unwrap();
    stdin().read_line(&mut input).expect("Could not read line");

    match input.trim() {
        "cont" => ReplCommand::Continue,
        "r" => ReplCommand::Continue,
        "exit" => ReplCommand::Exit,
        "e" => ReplCommand::Exit,
        "regs" => ReplCommand::GetRegs,
        "s" => ReplCommand::SingleStep,
        "lsb" => ReplCommand::ListBps,
        "back" => ReplCommand::Backtrace,
        "frame" => ReplCommand::Frame,
        "f" => ReplCommand::Frame,
        "get" => ReplCommand::GetVar,
        _ => {
            if input.starts_with("b") {
                let parts: Vec<&str> = input.trim().split(' ').collect();
                if parts.len() == 2 {
                    if let Some(parsed_addr) = parse_address(parts[1]) {
                        ReplCommand::SetBp(parsed_addr)
                    } else {
                        ReplCommand::SetBpName(String::from(parts[1]))
                    }
                } else {
                    println!("unsupported breakpoint command format.");
                    ReplCommand::Unknown
                }
            } else if input.starts_with("rb") {
                let parts: Vec<&str> = input.trim().split(' ').collect();
                if parts.len() == 2 {
                    if let Some(parsed_addr) = parse_address(parts[1]) {
                        ReplCommand::DeleteBp(parsed_addr)
                    } else {
                        println!("unsupported breakpoint command format.");
                        ReplCommand::Unknown
                    }
                } else {
                    println!("unsupported breakpoint command format.");
                    ReplCommand::Unknown
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
