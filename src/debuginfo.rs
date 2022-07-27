use std::{
    fmt::Display,
    fs,
    io::{self, BufRead},
    rc::Rc,
};

use addr2line::{self, fallible_iterator::FallibleIterator};
use gimli::{EndianReader, RunTimeEndian};
use memmap2;

use crate::gimliwrapper::GimliWrapper;

pub struct Location {
    address: u64,
    file: String,
    line: u32,
    pub function_name: String,
}

pub struct DebugInfo {
    context: addr2line::Context<EndianReader<RunTimeEndian, Rc<[u8]>>>,
    target: String,
    pub dwarf_info: GimliWrapper,
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file_name = self.file.split("/").last().unwrap();
        write!(
            f,
            "{:#x} {}() in {}, line {}",
            self.address, self.function_name, file_name, self.line
        )
    }
}

impl DebugInfo {
    pub fn create(target: &str) -> DebugInfo {
        let file = fs::File::open(target).unwrap();
        let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
        let object = object::File::parse(&*map).unwrap();
        let context = addr2line::Context::new(&object).unwrap();
        let dwarf_info = GimliWrapper::create(target);
        println!(
            "Successfully loaded debug information for file {}.",
            &target
        );
        let di = DebugInfo {
            context: context,
            target: String::from(target),
            dwarf_info,
        };

        return di;
    }

    pub fn get_location_at_addr(&self, addr: usize) -> Option<Location> {
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
                        address: addr as u64,
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

    pub fn print_code_at_addr(&self, addr: usize, range: usize) {
        let location = self
            .get_location_at_addr(addr)
            .expect("Could not get location for address.");
        let source_file = fs::File::open(location.file).expect("Could not open source code.");
        // let mut lines: Vec<String> = Vec::new();

        for (idx, line) in io::BufReader::new(source_file).lines().enumerate() {
            if let Ok(line) = line {
                let diff = (idx + 1).abs_diff(location.line as usize);
                if diff <= range {
                    println!("{}\t{}", if diff == 0 { "->" } else { "  " }, line.as_str());
                }
            }
        }
    }
}
