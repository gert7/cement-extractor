use std::{
    array,
    ffi::OsStr,
    fs::{create_dir_all, File},
    io::{BufReader, LineWriter, Read, Result, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    str::from_utf8,
    string,
    sync::Arc,
};

use byteorder::{LittleEndian, ReadBytesExt};

const ATG_HEADER: &[u8] = "ATG CORE CEMENT LIBRARY\0\0\0\0\0\0\0\0\0".as_bytes();
const BUFFER_SIZE: usize = 1024 * 1024 * 4;

type LE = LittleEndian;

fn next_multiple(n: i32, target: i32) -> i32 {
    if n % target == 0 {
        return n;
    }

    let prev = n / target;
    target * (prev + 1)
}

fn nm_to_skip(current: i32, target: i32) -> i32 {
    next_multiple(current, target) - current
}

fn skip_to_multiple(infile: &mut File, target: i32) -> Result<usize> {
    let current = infile.stream_position().unwrap();
    let to_skip = nm_to_skip(current as i32, target);
    infile.seek(SeekFrom::Current(to_skip as i64))?;
    Ok(to_skip as usize)
}

struct ArchiveHeader {
    pub header: [u8; 32],
    _unknown: u32,
    pub directory_offset: u32,
    pub directory_length: u32,
    pub offset_to_filename_directory: u32,
    pub filename_directory_length: u32,
    _null: u32,
    pub number_of_files: u32,
}

struct RCFile {
    pub offset: u32,
    pub length: u32,
}

impl ArchiveHeader {
    fn new() -> ArchiveHeader {
        ArchiveHeader {
            header: ['\0' as u8; 32],
            _unknown: 0,
            directory_offset: 0,
            directory_length: 0,
            offset_to_filename_directory: 0,
            filename_directory_length: 0,
            _null: 0,
            number_of_files: 0,
        }
    }
}

fn read_archive_header(infile: &mut File) -> Result<ArchiveHeader> {
    let mut a_head = ArchiveHeader::new();
    infile.read_exact(&mut a_head.header)?;
    if a_head.header == ATG_HEADER {
        println!("ATG header detected!");
    }
    infile.seek(SeekFrom::Current(4))?;
    a_head.directory_offset = infile.read_u32::<LE>()?;
    a_head.directory_length = infile.read_u32::<LE>()?;
    a_head.offset_to_filename_directory = infile.read_u32::<LE>()?;
    a_head.filename_directory_length = infile.read_u32::<LE>()?;
    infile.seek(SeekFrom::Current(4))?;
    a_head.number_of_files = infile.read_u32::<LE>()?;

    println!("Number of files: {}", a_head.number_of_files);

    Ok(a_head)
}

fn read_number_of_files(infile: &mut File, number: usize) -> Result<Vec<RCFile>> {
    let mut files = Vec::new();

    for _ in 0..number {
        infile.seek(SeekFrom::Current(4))?;
        let file = RCFile {
            offset: infile.read_u32::<LE>()?,
            length: infile.read_u32::<LE>()?,
        };
        println!("{}, length: {}", file.offset, file.length);
        files.push(file);
    }

    files.sort_by(|a, b| a.offset.cmp(&b.offset));

    Ok(files)
}

fn read_filenames(infile: &mut File, number: usize) -> Result<Vec<String>> {
    let mut filenames = Vec::new();

    println!("num {}", number);

    for _ in 0..number {
        infile.seek(SeekFrom::Current(4 * 3))?;
        let fn_length = infile.read_u32::<LE>()?;
        // println!("Filename length: {}", fn_length);
        let mut filename = vec![0u8; fn_length as usize - 1];
        // exclude null terminator
        infile.read_exact(&mut filename)?;
        // let fpos = infile.stream_position().unwrap();
        // let to_skip = nm_to_skip(fpos as i32, 4) - 1;
        let to_skip = 4;
        // println!("Current: {}. To skip: {}.", fpos, to_skip);
        infile.seek(SeekFrom::Current(to_skip as i64))?;

        match String::from_utf8(filename) {
            Ok(v) => {
                println!("{}", &v);
                filenames.push(v);
            }
            Err(e) => panic!("Malformed filename in filenames directory! {}", e),
        }
    }

    Ok(filenames)
}

fn file_from_path(winpath: &String) -> Result<File> {
    let wpath = winpath.replace("\\", "/");
    let wpath = Path::new(wpath.as_str());
    let mut wpath: Vec<Component> = wpath.components().collect();
    let result = wpath
        .pop()
        .expect(format!("Filename empty! Filename provided: {}", winpath).as_str())
        .as_os_str();
    let mut newpath = PathBuf::new();
    for dirs in wpath {
        match dirs {
            Component::Prefix(_) => (),
            Component::RootDir => (),
            Component::CurDir => (),
            Component::ParentDir => (),
            Component::Normal(v) => newpath.push(v),
        }
    }
    let newpath = newpath.as_path();
    create_dir_all(&newpath)?;
    let newpath = newpath.join(result);
    let newfile = File::create(newpath.as_path())?;
    Ok(newfile)
}

fn main() -> Result<()> {
    let input_file = std::env::args().nth(1);
    let input_file = input_file.expect("Missing input file!");

    let mut handle =
        File::open(&input_file).expect(format!("Unable to open file: {}", &input_file).as_str());

    let a_head = read_archive_header(&mut handle).unwrap();
    let num_files = a_head.number_of_files as usize;

    let files = read_number_of_files(&mut handle, num_files)?;
    skip_to_multiple(&mut handle, 2048)?;
    handle.seek(SeekFrom::Current(4 + 4))?;

    let filenames = read_filenames(&mut handle, num_files).unwrap();
    skip_to_multiple(&mut handle, 2048)?;

    let mut in_buffer = [0u8; BUFFER_SIZE];

    for (file, filename) in files.into_iter().zip(filenames.into_iter()) {
        // let filename = filename.replace("\\", "_");
        let filesize: usize = file.length as usize;
        let mut to_read = filesize;
        // let mut fhandle = File::create(&filename).unwrap();
        let mut fhandle = file_from_path(&filename).unwrap();
        loop {
            if to_read <= BUFFER_SIZE {
                println!("File {} remaining bytes to read {}", filename, to_read);
                let mut remainder = vec![0u8; to_read];
                let read_bytes = handle.read(&mut remainder).unwrap();
                if read_bytes != to_read {
                    panic!("Incorrect amount of bytes read at file {}", filename);
                }
                fhandle.write_all(remainder.as_slice()).unwrap();
                break;
            } else {
                println!("File {} remaining bytes to read {}", filename, to_read);
                let read = handle.read(&mut in_buffer).unwrap();
                fhandle.write_all(&in_buffer).unwrap();
                to_read = to_read - read;
            }
        }
        skip_to_multiple(&mut handle, 2048)?;
    }

    Ok(())
}
