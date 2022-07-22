mod debugger;

use crate::debugger::Debugger;

fn main() {
    let mut debugger = Debugger::create(String::from("a.out"));
    debugger.run().unwrap();
}
