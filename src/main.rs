mod debugger;

use crate::debugger::Debugger;

fn main() {
    let debugger = Debugger::create(String::from("a.out"));
    debugger.run().unwrap();
}
