use std::{fmt::Display, fs::File, rc::Rc};

use addr2line::{self, fallible_iterator::FallibleIterator};
use gimli::{EndianReader, RunTimeEndian};
use memmap2;

pub struct DebugInfo {
    context: addr2line::Context<EndianReader<RunTimeEndian, Rc<[u8]>>>,
}

pub struct Location {
    file: String,
    line: u32,
    function_name: String,
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "function {} in {}:{}",
            self.function_name, self.file, self.line
        )
    }
}

impl DebugInfo {
    pub fn create(target: &str) -> DebugInfo {
        let file = File::open(target).unwrap();
        let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
        let object = addr2line::object::File::parse(&*map).unwrap();
        let context = addr2line::Context::new(&object).unwrap();
        println!(
            "Successfully loaded debug information for file {}.",
            &target
        );
        DebugInfo { context: context }
    }

    pub fn get_function_at_addr(&self, addr: usize) -> Option<Location> {
        let frames = self
            .context
            .find_frames(addr as u64)
            .expect("Could not get frames.");
        let frames = frames.iterator();

        for frame in frames {
            match frame {
                Ok(f) => {
                    let function_name = f.function.unwrap().name.escape_ascii().to_string();
                    let location = f.location.unwrap();
                    return Some(Location {
                        file: String::from(location.file.unwrap_or("")),
                        function_name,
                        line: location.line.unwrap_or(0),
                    });
                }

                Err(e) => {
                    println!("Error during get location iterator {}", e);
                    return None;
                }
            }
        }
        None
    }
}
