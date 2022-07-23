use std::{fs::File, rc::Rc};

use addr2line::{self, fallible_iterator::FallibleIterator};
use gimli::{EndianReader, RunTimeEndian, Reader};
use memmap2;

pub struct DebugInfo {
    context: addr2line::Context<EndianReader<RunTimeEndian, Rc<[u8]>>>,
}

impl DebugInfo {
    pub fn create(target: &str) -> DebugInfo {
        let file = File::open(target).unwrap();
        let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
        let object = addr2line::object::File::parse(&*map).unwrap();
        let context = addr2line::Context::new(&object).unwrap();
        println!("Successfully loaded debug information for file {}.", &target);
        DebugInfo {
            context: context
        }
    }

    pub fn get_location(&self, addr: usize) {

        let frames = self.context.find_frames(addr as u64).expect("Could not get frames.");
        let frames = frames.iterator();

        for frame in frames {
            match frame {
                Ok(f) => {
                    let function_name = f.function.unwrap().name.escape_ascii().to_string();
                    let location = f.location.unwrap();
                    println!("{} at {}:{}", &function_name, location.file.unwrap_or(""), location.line.unwrap_or(0));
                }

                Err(e) => {
                    println!("Error during get location iterator {}", e);
                }
            }
        }
    }
}
