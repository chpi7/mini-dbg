use std::{
    borrow,
    fmt::Display,
    fs,
    io::{self, BufRead},
    rc::Rc,
};

use addr2line::{self, fallible_iterator::FallibleIterator};
use gimli::{DebuggingInformationEntry, Dwarf, EndianReader, EndianSlice, RunTimeEndian};
use memmap2;
use object::{Object, ObjectSection};

pub struct Location {
    address: u64,
    file: String,
    line: u32,
    function_name: String,
}

#[derive(Debug, Clone)]
pub enum Type {
    Base {
        name: String,
        is_float: bool,
        is_signed: bool,
        byte_size: u64,
        ref_addr: usize,
    },
    Pointer {
        byte_size: u64,
        to: usize,
        ref_addr: usize,
    },
}

pub struct FormalParameter {
    name: String,
    t: Type,
}

pub struct Variable {
    name: String,
    t: Type,
}

pub struct Function {
    name: String,
    t: Type,
    formal_parameters: Vec<FormalParameter>,
    local_variables: Vec<Variable>,
    address_range: Vec<(usize, usize)>,
}

pub struct DebugInfo {
    context: addr2line::Context<EndianReader<RunTimeEndian, Rc<[u8]>>>,
    target: String,
}

impl Type {
    pub fn empty() -> Type {
        Type::Pointer {
            byte_size: 0,
            ref_addr: 0,
            to: 0,
        }
    }
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:#x}    {}() in {}:{}",
            self.address, self.function_name, self.file, self.line
        )
    }
}

impl DebugInfo {
    pub fn create(target: &str) -> DebugInfo {
        let file = fs::File::open(target).unwrap();
        let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
        let object = object::File::parse(&*map).unwrap();
        let context = addr2line::Context::new(&object).unwrap();
        println!(
            "Successfully loaded debug information for file {}.",
            &target
        );
        DebugInfo {
            context: context,
            target: String::from(target),
        }
    }

    /// Use for testing.
    #[allow(dead_code)]
    pub fn dump_info(&self) -> Result<(), gimli::Error> {
        let file = fs::File::open(&self.target).unwrap();
        let map = unsafe { memmap2::Mmap::map(&file).unwrap() };
        let object = object::File::parse(&*map).unwrap();

        let endian = if object.is_little_endian() {
            gimli::RunTimeEndian::Little
        } else {
            gimli::RunTimeEndian::Little
        };

        // Load a section and return as `Cow<[u8]>`.
        let load_section = |id: gimli::SectionId| -> Result<borrow::Cow<[u8]>, gimli::Error> {
            match object.section_by_name(id.name()) {
                Some(ref section) => Ok(section
                    .uncompressed_data()
                    .unwrap_or(borrow::Cow::Borrowed(&[][..]))),
                None => Ok(borrow::Cow::Borrowed(&[][..])),
            }
        };

        // Load all of the sections.
        let dwarf_cow = gimli::Dwarf::load(&load_section)?;

        // Borrow a `Cow<[u8]>` to create an `EndianSlice`.
        let borrow_section: &dyn for<'a> Fn(
            &'a borrow::Cow<[u8]>,
        )
            -> gimli::EndianSlice<'a, gimli::RunTimeEndian> =
            &|section| gimli::EndianSlice::new(&*section, endian);

        // Create `EndianSlice`s for all of the sections.
        let dwarf = dwarf_cow.borrow(&borrow_section);

        // Iterate over the compilation units.
        let mut iter = dwarf.units();

        while let Some(header) = iter.next()? {
            let unit = dwarf.unit(header)?;
            // println!("Unit: {:?}", unit.name);

            let mut base_types: Vec<Type> = Vec::new();

            // 1) Read base types
            let mut depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_base_type => {
                        base_types.push(self.process_base_type(entry, &dwarf)?);
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            // 2) Read pointer types
            let mut depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_pointer_type => {
                        base_types.push(self.process_pointer_type(entry)?);
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            // 3) Read everything else
            let mut function: Option<Function> = None;
            let mut depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_subprogram => {
                        // function = Some(self.process_subprogram(entry, &dwarf)?);
                    }
                    gimli::DW_TAG_formal_parameter => {
                        if let Some(function) = &mut function {
                            let fp = self.process_formal_parameter(entry, &dwarf)?;
                            function.formal_parameters.push(fp);
                        }
                    }
                    gimli::DW_TAG_variable => {
                        if let Some(function) = &mut function {
                            let fp = self.process_variable(entry, &dwarf)?;
                            function.local_variables.push(fp);
                        }
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            println!("Loaded base types:");
            for bt in &base_types {
                println!("{:?}", bt);
            }
        }

        Ok(())
    }

    fn process_base_type(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    ) -> Result<Type, gimli::Error> {
        let mut is_float = false;
        let mut is_signed = false;
        let mut name = String::from("");
        let mut byte_size = 0;
        let ref_addr = entry.offset().0;

        let mut attrs = entry.attrs();
        while let Some(attr) = attrs.next()? {
            match attr.name() {
                gimli::DW_AT_byte_size => {
                    byte_size = attr
                        .value()
                        .udata_value()
                        .expect("Could not get udata_value");
                }
                gimli::DW_AT_encoding => {
                    if let gimli::AttributeValue::Encoding(encoding) = attr.value() {
                        is_signed = (encoding == gimli::DW_ATE_signed_fixed)
                            || (encoding == gimli::DW_ATE_signed_char)
                            || (encoding == gimli::DW_ATE_signed);
                        is_float = encoding == gimli::DW_ATE_float;
                    }
                }
                gimli::DW_AT_name => match attr.value() {
                    gimli::AttributeValue::String(slice) => {
                        name = slice.to_string_lossy().to_string();
                    }
                    gimli::AttributeValue::DebugStrRef(offset) => {
                        let n = dwarf.debug_str.get_str(offset)?;
                        name = n.to_string_lossy().to_string();
                    }
                    _ => {
                        println!("Could not get base_type name.");
                    }
                },
                _ => {}
            }
        }
        Ok(Type::Base {
            name,
            is_float,
            is_signed,
            byte_size,
            ref_addr,
        })
    }

    fn process_pointer_type(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
    ) -> Result<Type, gimli::Error> {
        let mut byte_size = 0;
        let mut to = 0;
        let ref_addr = entry.offset().0;

        let mut attrs = entry.attrs();
        while let Some(attr) = attrs.next()? {
            match attr.name() {
                gimli::DW_AT_byte_size => {
                    byte_size = attr
                        .value()
                        .udata_value()
                        .expect("Could not get udata_value");
                }
                gimli::DW_AT_type => {
                    if let gimli::AttributeValue::UnitRef(gimli::UnitOffset(offset)) = attr.value()
                    {
                        to = offset;
                    } else {
                        println!("Could not get base_type offset for pointer type.");
                    }
                }
                _ => {}
            }
        }

        Ok(Type::Pointer {
            byte_size,
            to,
            ref_addr,
        })
    }

    // fn process_subprogram(
    //     &self,
    //     entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
    //     dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    // ) -> Result<Function, gimli::Error> {

    // }

    fn process_formal_parameter(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    ) -> Result<FormalParameter, gimli::Error> {
        Ok(FormalParameter {
            name: String::from("Hey"),
            // TODO: only set the ref_addr for the type and resolve it later when we know all types
            t: Type::empty(),
        })
    }

    fn process_variable(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    ) -> Result<Variable, gimli::Error> {
        Ok(Variable {
            name: String::from("Hey"),
            // TODO: only set the ref_addr for the type and resolve it later when we know all types
            t: Type::empty(),
        })
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
                    println!("{}\t{}", if diff == 0 { "🔴" } else { "  " }, line.as_str());
                }
            }
        }
    }
}
