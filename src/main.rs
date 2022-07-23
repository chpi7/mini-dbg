mod debugger;
mod debuginfo;
mod replcommand;
mod target;
mod util;

use crate::debugger::Debugger;

fn main() {
    println!("mini-dbg v0.1");

    let mut debugger = Debugger::create(String::from("a.out"));
    debugger.run().unwrap();
}
