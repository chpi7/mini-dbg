use std::{
    borrow,
    collections::HashMap,
    fs,
};

use gimli::{DebuggingInformationEntry, Dwarf, EndianSlice, RunTimeEndian};
use memmap2;
use object::{Object, ObjectSection};


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
    Const {
        byte_size: u64,
        to: usize,
        ref_addr: usize,
    },
}

#[derive(Debug)]
pub struct FormalParameter {
    pub name: String,
    pub t: usize,
    pub fbreg_offset: i64,
}

#[derive(Debug)]
pub struct Variable {
    pub name: String,
    pub t: usize,
    pub fbreg_offset: i64,
}

#[derive(Debug)]
pub struct Function {
    name: String,
    pub t: usize,
    pub formal_parameters: Vec<FormalParameter>,
    pub local_variables: Vec<Variable>,
    pub address_range: Vec<(usize, usize)>,
}

impl Type {
    pub fn void() -> Type {
        Type::Pointer {
            byte_size: 0,
            ref_addr: 0,
            to: 0,
        }
    }
}

pub struct GimliWrapper {
    target: String,
    types: HashMap<usize, Type>,
    functions: Vec<Function>,
}

impl GimliWrapper {
    pub fn create(target: &str) -> GimliWrapper {
        let mut di = GimliWrapper {
            target: String::from(target),
            types: HashMap::new(),
            functions: Vec::new(),
        };
        di.collect_info().expect("Error while collecting debug info.");
        return di;
    }

    #[allow(dead_code)]
    pub fn print_type(&self, t: &Type) {
        match t {
            Type::Base { name, is_float:_, is_signed:_, byte_size:_, ref_addr:_ } => {
                print!("{}", name.as_str());
            },
            Type::Pointer { byte_size:_, to, ref_addr:_ } => {
                self.print_type(self.types.get(to).unwrap());
                print!("*");
            },
            Type::Const { byte_size:_, to, ref_addr:_ } => {
                print!("const ");
                self.print_type(self.types.get(to).unwrap());
            },
        }
    }

    #[allow(dead_code)]
    pub fn print_function(&self, function: &Function){
        if let Some(t) = self.types.get(&function.t) {
            self.print_type(t);
            print!(" ");
        } else {
            print!("void ");
        }
        print!("{}", function.name.as_str());
        print!("(");
        let mut first = true;
        for formal_parameter in &function.formal_parameters {
            let t = self.types.get(&formal_parameter.t).unwrap();
            if !first {
                print!(", ");
            }
            self.print_type(t);
            print!(" {}", formal_parameter.name.as_str());
            first = false;
        }
        print!(")");
    }

    pub fn get_function_by_name(&self, fname: &str) -> Option<&Function> {
        self.functions.iter().find(|f| f.name == fname)
    }

    pub fn get_type(&self, t: usize) -> Option<&Type> {
        self.types.get(&t)
    }

    pub fn get_type_byte_size(&self, t: usize) -> Option<u64> {
        match self.types.get(&t)? {
            Type::Base { name:_, is_float:_, is_signed:_, byte_size, ref_addr:_ } => Some(*byte_size),
            Type::Pointer { byte_size, to:_, ref_addr:_ } => Some(*byte_size),
            Type::Const { byte_size, to:_, ref_addr:_ } => Some(*byte_size),
        }
    }

    fn collect_info(&mut self) -> Result<(), gimli::Error> {
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

            let mut types: Vec<Type> = Vec::new();
            let mut functions: Vec<Function> = Vec::new();

            // 1) Read base types
            let mut _depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                _depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_base_type => {
                        types.push(self.process_base_type(entry, &dwarf)?);
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            // 2) Read pointer types
            let mut _depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                _depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_pointer_type => {
                        types.push(self.process_pointer_type(entry)?);
                    }
                    gimli::DW_TAG_const_type => {
                        types.push(self.process_const_type(entry)?);
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            // 3) Read everything else
            let mut _depth = 0;
            let mut entries = unit.entries();
            while let Some((delta_depth, entry)) = entries.next_dfs()? {
                _depth += delta_depth;

                match entry.tag() {
                    gimli::DW_TAG_subprogram => {
                        functions.push(self.process_subprogram(entry, &dwarf)?);
                    }
                    gimli::DW_TAG_formal_parameter => {
                        if let Some(function) = functions.last_mut() {
                            let fp =
                                self.process_formal_parameter(entry, &dwarf, unit.encoding())?;
                            function.formal_parameters.push(fp);
                        }
                    }
                    gimli::DW_TAG_variable => {
                        if let Some(function) = functions.last_mut() {
                            let fp = self.process_variable(entry, &dwarf, unit.encoding())?;
                            function.local_variables.push(fp);
                        }
                    }
                    _ => {} // println!("Skipping <{}><{:#x}> {}", depth, entry.offset().0, entry.tag());
                }
            }

            for typ in types {
                let ref_addr = *match &typ {
                    Type::Base {
                        name: _,
                        is_float: _,
                        is_signed: _,
                        byte_size: _,
                        ref_addr,
                    } => ref_addr,
                    Type::Pointer {
                        byte_size: _,
                        to: _,
                        ref_addr,
                    } => ref_addr,
                    Type::Const {
                        byte_size: _,
                        to: _,
                        ref_addr,
                    } => ref_addr,
                };
                self.types.insert(ref_addr, typ);
            }

            self.functions.extend(functions);
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
                gimli::DW_AT_name => {
                    name = self
                        .resolve_dw_at_name(&attr, dwarf)
                        .unwrap_or(String::new());
                }
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

    fn resolve_dw_at_name(
        &self,
        attr: &gimli::Attribute<EndianSlice<RunTimeEndian>>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    ) -> Option<String> {
        match attr.value() {
            gimli::AttributeValue::String(slice) => Some(slice.to_string_lossy().to_string()),
            gimli::AttributeValue::DebugStrRef(offset) => match dwarf.debug_str.get_str(offset) {
                Ok(n) => Some(n.to_string_lossy().to_string()),
                Err(_) => None,
            },
            _ => None,
        }
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

    fn process_const_type(
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

        Ok(Type::Const {
            byte_size,
            to,
            ref_addr,
        })
    }

    fn process_subprogram(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
    ) -> Result<Function, gimli::Error> {
        let mut name = String::new();
        let mut t = 0;
        let mut low_pc = 0;
        let mut high_pc = 0;
        let mut high_offset = None;

        let mut attrs = entry.attrs();
        while let Some(attr) = attrs.next()? {
            // println!("   {}: {:?}", attr.name(), attr.value());
            match attr.name() {
                gimli::DW_AT_name => {
                    name = self
                        .resolve_dw_at_name(&attr, dwarf)
                        .unwrap_or(String::new());
                }
                gimli::DW_AT_high_pc => {
                    match attr.value() {
                        gimli::AttributeValue::Udata(v) => high_offset = Some(v),
                        gimli::AttributeValue::Addr(v) => high_pc = v,
                        _ => println!("Unsupported high_pc format")
                    }
                }
                gimli::DW_AT_low_pc => {
                    if let gimli::AttributeValue::Addr(v) = attr.value() {
                        low_pc = v;
                    } else {
                        println!("could not read low_pc")
                    }
                }
                gimli::DW_AT_type => {
                    if let gimli::AttributeValue::UnitRef(gimli::UnitOffset(offset)) = attr.value()
                    {
                        t = offset;
                    } else {
                        println!("Could not get base_type offset for pointer type.");
                    }
                }
                _ => {}
            }
        }

        if let Some(offset) = high_offset {
            // 2.17.2 Contiguous Address Range
            // if offset -> low + offset is one past the last instruction
            // if addr -> high is the last instruction
            high_pc = low_pc + offset - 1;
        }

        Ok(Function {
            address_range: vec![(low_pc as usize, high_pc as usize)],
            formal_parameters: Vec::new(),
            local_variables: Vec::new(),
            name: name,
            t: t,
        })
    }

    fn process_formal_parameter(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
        encoding: gimli::Encoding,
    ) -> Result<FormalParameter, gimli::Error> {
        let mut name = String::new();
        let mut t = 0;
        let mut fbreg_offset = 0;

        // println!("<{:x}> {}", entry.offset().0, entry.tag());
        let mut attrs = entry.attrs();
        while let Some(attr) = attrs.next()? {
            // println!("   {}: {:?}", attr.name(), attr.value());
            match attr.name() {
                gimli::DW_AT_name => {
                    name = self
                        .resolve_dw_at_name(&attr, dwarf)
                        .unwrap_or(String::new());
                }
                gimli::DW_AT_type => {
                    if let gimli::AttributeValue::UnitRef(gimli::UnitOffset(offset)) = attr.value()
                    {
                        t = offset;
                    } else {
                        println!("Could not get base_type offset for pointer type.");
                    }
                }
                gimli::DW_AT_location => {
                    if let gimli::AttributeValue::Exprloc(gimli::Expression(es)) = &mut attr.value()
                    {
                        match gimli::Operation::parse(es, encoding) {
                            Ok(gimli::Operation::FrameOffset { offset }) => {
                                fbreg_offset = offset;
                            }
                            _ => {
                                println!("Could not parse DW_AT_location operation.");
                            }
                        }
                    } else {
                        println!("Could not interpret DW_AT_location");
                    }
                }
                _ => {}
            }
        }
        Ok(FormalParameter {
            name,
            t,
            fbreg_offset,
        })
    }

    fn process_variable(
        &self,
        entry: &DebuggingInformationEntry<EndianSlice<RunTimeEndian>, usize>,
        dwarf: &Dwarf<EndianSlice<RunTimeEndian>>,
        encoding: gimli::Encoding,
    ) -> Result<Variable, gimli::Error> {
        let mut name = String::new();
        let mut t = 0;
        let mut fbreg_offset = 0;

        // println!("<{:x}> {}", entry.offset().0, entry.tag());
        let mut attrs = entry.attrs();
        while let Some(attr) = attrs.next()? {
            // println!("   {}: {:?}", attr.name(), attr.value());
            match attr.name() {
                gimli::DW_AT_name => {
                    name = self
                        .resolve_dw_at_name(&attr, dwarf)
                        .unwrap_or(String::new());
                }
                gimli::DW_AT_type => {
                    if let gimli::AttributeValue::UnitRef(gimli::UnitOffset(offset)) = attr.value()
                    {
                        t = offset;
                    } else {
                        println!("Could not get base_type offset for pointer type.");
                    }
                }
                gimli::DW_AT_location => {
                    if let gimli::AttributeValue::Exprloc(gimli::Expression(es)) = &mut attr.value()
                    {
                        match gimli::Operation::parse(es, encoding) {
                            Ok(gimli::Operation::FrameOffset { offset }) => {
                                fbreg_offset = offset;
                            }
                            _ => {
                                println!("Could not parse DW_AT_location operation.");
                            }
                        }
                    } else {
                        println!("Could not interpret DW_AT_location");
                    }
                }
                _ => {}
            }
        }
        Ok(Variable {
            name,
            t,
            fbreg_offset,
        })
    }
}
