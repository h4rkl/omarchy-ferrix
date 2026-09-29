//! Synthetic factory-archive generator used only by offline shell tests.

use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

type DynError = Box<dyn std::error::Error + Send + Sync>;

fn invalid(message: impl Into<String>) -> DynError {
    io::Error::new(io::ErrorKind::InvalidInput, message.into()).into()
}

fn run() -> Result<(), DynError> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(invalid(
            "usage: pixel-9a-fixture DIRECTORY BOARD stored|deflated",
        ));
    }
    let directory = PathBuf::from(&args[0]);
    let board = args[1]
        .to_str()
        .ok_or_else(|| invalid("fixture board must be UTF-8"))?;
    let nested_method = match args[2].to_str() {
        Some("stored") => CompressionMethod::Stored,
        Some("deflated") => CompressionMethod::Deflated,
        _ => return Err(invalid("fixture compression must be stored or deflated")),
    };
    fs::create_dir_all(&directory)?;

    let nested_path = directory.join("image-tegu-testbuild.zip");
    let mut nested = ZipWriter::new(File::create(&nested_path)?);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    nested.start_file("android-info.txt", deflated)?;
    writeln!(nested, "require version-baseband=radio-test")?;
    writeln!(nested, "require board={board}")?;
    writeln!(nested, "require version-bootloader=boot-test")?;
    nested.start_file("fastboot-info.txt", deflated)?;
    writeln!(nested, "# fixture")?;
    writeln!(nested, "version 1")?;
    writeln!(nested, "flash boot")?;
    writeln!(nested, "reboot fastboot")?;
    writeln!(nested, "update-super")?;
    nested.start_file("boot.img", deflated)?;
    nested.write_all(b"boot fixture")?;
    nested.start_file("vendor_boot.img", deflated)?;
    nested.write_all(b"vendor boot fixture")?;
    nested.finish()?;

    let factory_path = directory.join("tegu-testbuild-factory-fixture.zip");
    let mut factory = ZipWriter::new(File::create(&factory_path)?);
    factory.add_directory("tegu-testbuild/", SimpleFileOptions::default())?;
    factory.start_file(
        "tegu-testbuild/image-tegu-testbuild.zip",
        SimpleFileOptions::default().compression_method(nested_method),
    )?;
    io::copy(&mut File::open(&nested_path)?, &mut factory)?;
    factory.start_file("tegu-testbuild/bootloader-tegu-test.img", deflated)?;
    factory.write_all(b"bootloader fixture")?;
    factory.start_file("tegu-testbuild/radio-tegu-test.img", deflated)?;
    factory.write_all(b"radio fixture")?;
    factory.finish()?;

    let data = fs::read(&factory_path)?;
    let hash = format!("{:x}", Sha256::digest(&data));
    fs::write(
        directory.join("recovery.pins"),
        format!(
            "factory\tTESTBUILD\t17.0.0\ttegu-testbuild-factory-fixture.zip\thttps://dl.google.com/dl/android/aosp/fixture.zip\t{hash}\t{}\thttps://developers.google.com/android/images\n",
            data.len()
        ),
    )?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fixture-failed: {error}");
        std::process::exit(1);
    }
}
