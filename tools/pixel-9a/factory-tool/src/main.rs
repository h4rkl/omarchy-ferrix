//! Offline inspection of the exact Pixel 9a factory package.
//!
//! The outer Google archive stores its multi-gigabyte image ZIP without
//! compression. A bounded `Read + Seek` view lets `zip` inspect the nested
//! central directory and small metadata members without extracting that ZIP
//! or any firmware image.

use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::CompressionMethod;
use zip::read::ZipArchive;

const TARGET_ID: &str = "google-pixel-9a-tegu-gxq96-mp1-0";
const EXPECTED_CODENAME: &str = "tegu";
const MAX_METADATA_BYTES: u64 = 1024 * 1024;

type DynError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug)]
struct FactoryPin {
    build: String,
    android_version: String,
    filename: String,
    sha256: String,
    bytes: u64,
}

#[derive(Serialize)]
struct Inventory {
    schema_version: u32,
    evidence: String,
    event: &'static str,
    target_id: &'static str,
    redaction: Redaction,
    factory: FactoryRecord,
    compatibility_checks: BTreeMap<&'static str, bool>,
    android_info: AndroidInfo,
    fastboot_info: FastbootInfo,
    outer_members: Vec<MemberRecord>,
    nested_image_archive: NestedArchive,
}

#[derive(Serialize)]
struct Redaction {
    device_identifiers_collected: bool,
    host_paths_recorded: bool,
    payloads_extracted: bool,
}

#[derive(Serialize)]
struct FactoryRecord {
    build: String,
    android_version: String,
    filename: String,
    sha256: String,
    bytes: u64,
    verified: bool,
}

#[derive(Serialize)]
struct AndroidInfo {
    requirements: BTreeMap<String, String>,
    lines: Vec<String>,
}

#[derive(Serialize)]
struct FastbootInfo {
    version: String,
    commands: Vec<String>,
}

#[derive(Clone, Serialize)]
struct MemberRecord {
    name: String,
    uncompressed_bytes: u64,
    compressed_bytes: u64,
    compression: String,
    crc32: String,
}

#[derive(Serialize)]
struct NestedArchive {
    name: String,
    bytes: u64,
    members: Vec<MemberRecord>,
}

struct StoredMember {
    source: File,
    start: u64,
    size: u64,
    position: u64,
}

impl StoredMember {
    fn new(source: File, start: u64, size: u64) -> Self {
        Self {
            source,
            start,
            size,
            position: 0,
        }
    }
}

impl Read for StoredMember {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.size.saturating_sub(self.position);
        let count = usize::try_from(remaining.min(buffer.len() as u64)).unwrap_or(buffer.len());
        self.source
            .seek(SeekFrom::Start(self.start + self.position))?;
        let read = self.source.read(&mut buffer[..count])?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for StoredMember {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let next = match position {
            SeekFrom::Start(offset) => i128::from(offset),
            SeekFrom::Current(offset) => i128::from(self.position) + i128::from(offset),
            SeekFrom::End(offset) => i128::from(self.size) + i128::from(offset),
        };
        if next < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "negative seek in nested image archive",
            ));
        }
        self.position = u64::try_from(next).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "nested image seek overflow")
        })?;
        Ok(self.position)
    }
}

fn invalid(message: impl Into<String>) -> DynError {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn factory_pin(path: &Path) -> Result<FactoryPin, DynError> {
    for raw in fs::read_to_string(path)?.lines() {
        if raw.starts_with('#') || raw.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = raw.split('\t').collect();
        if fields.first() == Some(&"factory") {
            if fields.len() != 8 {
                return Err(invalid(format!(
                    "malformed factory pin in {}",
                    path.display()
                )));
            }
            return Ok(FactoryPin {
                build: fields[1].to_owned(),
                android_version: fields[2].to_owned(),
                filename: fields[3].to_owned(),
                sha256: fields[5].to_owned(),
                bytes: fields[6]
                    .parse()
                    .map_err(|_| invalid("factory pin has a non-numeric byte count"))?,
            });
        }
    }
    Err(invalid(format!(
        "factory pin missing from {}",
        path.display()
    )))
}

fn sha256_and_size(path: &Path) -> Result<(String, u64), DynError> {
    let mut source = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut size = 0_u64;
    loop {
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        size += count as u64;
    }
    Ok((format!("{:x}", digest.finalize()), size))
}

fn compression_name(method: CompressionMethod) -> String {
    match method {
        CompressionMethod::Stored => "stored".to_owned(),
        CompressionMethod::Deflated => "deflated".to_owned(),
        other => format!("method-{other:?}"),
    }
}

fn member_record<R: Read>(member: &zip::read::ZipFile<'_, R>) -> MemberRecord {
    MemberRecord {
        name: member.name().to_owned(),
        uncompressed_bytes: member.size(),
        compressed_bytes: member.compressed_size(),
        compression: compression_name(member.compression()),
        crc32: format!("{:08x}", member.crc32()),
    }
}

fn metadata_text<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<String, DynError> {
    let mut member = archive
        .by_name(name)
        .map_err(|_| invalid(format!("nested image archive is missing {name}")))?;
    if member.size() > MAX_METADATA_BYTES {
        return Err(invalid(format!(
            "refusing oversized {name} ({} bytes)",
            member.size()
        )));
    }
    let mut bytes = Vec::with_capacity(member.size() as usize);
    member.read_to_end(&mut bytes)?;
    String::from_utf8(bytes).map_err(Into::into)
}

fn meaningful_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn inspect(pins_path: &Path, archive_path: &Path) -> Result<Inventory, DynError> {
    let pin = factory_pin(pins_path)?;
    if archive_path.file_name().and_then(|name| name.to_str()) != Some(&pin.filename) {
        return Err(invalid(format!(
            "factory filename mismatch: expected {}, got {}",
            pin.filename,
            archive_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        )));
    }
    let (actual_hash, actual_size) = sha256_and_size(archive_path)?;
    if actual_size != pin.bytes {
        return Err(invalid(format!(
            "factory size mismatch: expected {}, got {actual_size}",
            pin.bytes
        )));
    }
    if actual_hash != pin.sha256 {
        return Err(invalid(format!(
            "factory SHA-256 mismatch: expected {}, got {actual_hash}",
            pin.sha256
        )));
    }

    let mut outer = ZipArchive::new(File::open(archive_path)?)?;
    let mut outer_members = Vec::with_capacity(outer.len());
    let mut image_member = None;
    for index in 0..outer.len() {
        let member = outer.by_index(index)?;
        if !member.is_dir()
            && Path::new(member.name())
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("image-") && name.ends_with(".zip"))
        {
            if image_member.is_some() {
                return Err(invalid(
                    "expected exactly one nested image archive, found more than one",
                ));
            }
            image_member = Some((
                member.name().to_owned(),
                member.size(),
                member.compressed_size(),
                member.compression(),
                member.data_start(),
            ));
        }
        outer_members.push(member_record(&member));
    }
    let Some((image_name, image_size, image_compressed_size, image_compression, image_start)) =
        image_member
    else {
        return Err(invalid(
            "expected exactly one nested image archive, found none",
        ));
    };
    let expected_image_name = format!("image-{EXPECTED_CODENAME}-{}.zip", pin.build.to_lowercase());
    if Path::new(&image_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(&expected_image_name)
    {
        return Err(invalid(format!(
            "nested image archive mismatch: expected {expected_image_name}, got {image_name}"
        )));
    }
    if image_compression != CompressionMethod::Stored || image_compressed_size != image_size {
        return Err(invalid(
            "nested image archive is compressed; refusing multi-gigabyte extraction for inspection",
        ));
    }
    drop(outer);

    let view = StoredMember::new(File::open(archive_path)?, image_start, image_size);
    let mut nested = ZipArchive::new(view)?;
    let mut nested_members = Vec::with_capacity(nested.len());
    for index in 0..nested.len() {
        nested_members.push(member_record(&nested.by_index(index)?));
    }
    let android_lines = meaningful_lines(&metadata_text(&mut nested, "android-info.txt")?);
    let fastboot_lines = meaningful_lines(&metadata_text(&mut nested, "fastboot-info.txt")?);

    let mut requirements = BTreeMap::new();
    for line in &android_lines {
        if let Some(requirement) = line.strip_prefix("require ")
            && let Some((key, value)) = requirement.split_once('=')
        {
            requirements.insert(key.to_owned(), value.to_owned());
        }
    }
    let fastboot_version = fastboot_lines
        .iter()
        .find_map(|line| line.strip_prefix("version "))
        .ok_or_else(|| invalid("fastboot-info.txt has no version"))?
        .to_owned();
    let commands = fastboot_lines
        .iter()
        .filter(|line| !line.starts_with("version "))
        .cloned()
        .collect();
    let checks = BTreeMap::from([
        ("archive_hash_and_size", true),
        ("nested_image_name", true),
        (
            "android_board_is_tegu",
            requirements
                .get("board")
                .is_some_and(|board| board == EXPECTED_CODENAME),
        ),
        (
            "bootloader_requirement_present",
            requirements
                .get("version-bootloader")
                .is_some_and(|value| !value.is_empty()),
        ),
        (
            "baseband_requirement_present",
            requirements
                .get("version-baseband")
                .is_some_and(|value| !value.is_empty()),
        ),
        ("fastboot_info_version_1", fastboot_version == "1"),
    ]);
    let failed: Vec<_> = checks
        .iter()
        .filter_map(|(name, passed)| (!passed).then_some(*name))
        .collect();
    if !failed.is_empty() {
        return Err(invalid(format!(
            "factory compatibility checks failed: {}",
            failed.join(", ")
        )));
    }

    Ok(Inventory {
        schema_version: 1,
        evidence: format!("physical:{TARGET_ID}"),
        event: "factory-package-inventory",
        target_id: TARGET_ID,
        redaction: Redaction {
            device_identifiers_collected: false,
            host_paths_recorded: false,
            payloads_extracted: false,
        },
        factory: FactoryRecord {
            build: pin.build,
            android_version: pin.android_version,
            filename: pin.filename,
            sha256: actual_hash,
            bytes: actual_size,
            verified: true,
        },
        compatibility_checks: checks,
        android_info: AndroidInfo {
            requirements,
            lines: android_lines,
        },
        fastboot_info: FastbootInfo {
            version: fastboot_version,
            commands,
        },
        outer_members,
        nested_image_archive: NestedArchive {
            name: image_name,
            bytes: image_size,
            members: nested_members,
        },
    })
}

fn write_inventory(output: &Path, inventory: &Inventory) -> Result<(), DynError> {
    let parent = output
        .parent()
        .ok_or_else(|| invalid("inventory output has no parent directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = output.with_extension("json.tmp");
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, inventory)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, output)?;
    Ok(())
}

fn run() -> Result<PathBuf, DynError> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(invalid(
            "usage: pixel-9a-factory-inspect PINS FACTORY_ARCHIVE OUTPUT_JSON",
        ));
    }
    let pins = PathBuf::from(&args[0]);
    let archive = PathBuf::from(&args[1]);
    let output = PathBuf::from(&args[2]);
    let inventory = inspect(&pins, &archive)?;
    write_inventory(&output, &inventory)?;
    Ok(output)
}

fn main() {
    match run() {
        Ok(output) => println!(
            "{}",
            serde_json::json!({
                "evidence": format!("physical:{TARGET_ID}"),
                "event": "factory-inspection-complete",
                "inventory": output,
            })
        ),
        Err(error) => {
            eprintln!("factory-inspection-refused: {error}");
            std::process::exit(1);
        }
    }
}
