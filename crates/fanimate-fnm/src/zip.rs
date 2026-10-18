//! ZIP archives: the container of a single-file `.fnm` project, and of
//! `.moho` and `.fla` files (the importers read them through this module).
//!
//! The reader is lenient. Animate writes archives whose end record
//! misstates the size of the central directory, which strict readers
//! refuse, so it walks the central directory entry by entry instead, checks
//! each against its local header, and only needs stored and deflated
//! entries, which is all either app writes. Damaged archives give an error,
//! never a panic.
//!
//! The writer is strict and small: stored or deflated entries, UTF-8 names,
//! a fixed timestamp (so saving the same project twice gives the same
//! bytes), CRC-32 for every entry, and no ZIP64. A project that would need
//! ZIP64 (4 GiB) is an error, not a damaged file.

use std::collections::BTreeMap;

const LOCAL: &[u8; 4] = b"PK\x03\x04";
const CENTRAL: &[u8; 4] = b"PK\x01\x02";
const END: &[u8; 4] = b"PK\x05\x06";

/// The largest entry this reader inflates. Project files are far smaller;
/// the limit stops a hostile archive from exhausting memory.
const MAX_ENTRY: usize = 512 * 1024 * 1024;

/// Deflate effort for the writer: fast enough to save a project without a
/// pause, small enough to be worth it.
const LEVEL: u8 = 6;

/// 1980-01-01, the earliest date ZIP can say.
const DOS_DATE: u16 = 0x0021;
/// Bit 11 of the general purpose flags: names are UTF-8.
const UTF8_NAMES: u16 = 0x0800;

/// An archive's entries by name. Entries are inflated when read.
pub struct Archive<'a> {
    bytes: &'a [u8],
    entries: BTreeMap<String, Entry>,
}

#[derive(Clone, Copy)]
struct Entry {
    method: u16,
    crc: u32,
    compressed: usize,
    size: usize,
    data: usize,
}

fn u16_at(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        b.get(at..at.checked_add(2)?)?.try_into().ok()?,
    )))
}

fn u32_at(b: &[u8], at: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?) as usize)
}

/// Whether `bytes` start like a ZIP archive.
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(LOCAL)
}

impl<'a> Archive<'a> {
    pub fn read(bytes: &'a [u8]) -> Result<Self, String> {
        const NO_TABLE: &str = "the archive has no table of contents";
        const CUT: &str = "the archive's table of contents is cut short";
        let end = bytes.windows(4).rposition(|w| w == END).ok_or(NO_TABLE)?;
        let mut at = u32_at(bytes, end + 16).ok_or(CUT)?;
        if bytes.get(at..at + 4) != Some(CENTRAL) {
            // the stated offset is wrong: use the first directory entry
            at = bytes.windows(4).position(|w| w == CENTRAL).ok_or(NO_TABLE)?;
        }
        let mut entries = BTreeMap::new();
        while bytes.get(at..at + 4) == Some(CENTRAL) {
            let fields = (
                u16_at(bytes, at + 10),
                u32_at(bytes, at + 16),
                u32_at(bytes, at + 20),
                u32_at(bytes, at + 24),
                u16_at(bytes, at + 28),
                u16_at(bytes, at + 30),
                u16_at(bytes, at + 32),
                u32_at(bytes, at + 42),
            );
            let (
                Some(method),
                Some(crc),
                Some(compressed),
                Some(size),
                Some(name_len),
                Some(extra),
                Some(comment),
                Some(local),
            ) = fields
            else {
                return Err(CUT.into());
            };
            let name = bytes.get(at + 46..at + 46 + name_len).ok_or(CUT)?;
            let name = String::from_utf8_lossy(name).replace('\\', "/");
            at += 46 + name_len + extra + comment;
            if name.ends_with('/') || bytes.get(local..local.saturating_add(4)) != Some(LOCAL) {
                continue;
            }
            let (Some(local_name), Some(local_extra)) =
                (u16_at(bytes, local + 26), u16_at(bytes, local + 28))
            else {
                continue;
            };
            let entry = Entry {
                method: method as u16,
                crc: crc as u32,
                compressed,
                size,
                data: local + 30 + local_name + local_extra,
            };
            entries.insert(name, entry);
        }
        if entries.is_empty() {
            return Err("the archive is empty or damaged".into());
        }
        Ok(Self { bytes, entries })
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    fn entry(&self, name: &str) -> Option<Entry> {
        self.entries
            .get(name)
            .or_else(|| {
                self.entries
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, e)| e)
            })
            .copied()
    }

    /// The entry called `name` (compared without regard to case, as both
    /// apps run on case-insensitive file systems), inflated. The checksum
    /// is not checked: Animate's archives are not always right about it.
    pub fn get(&self, name: &str) -> Option<Result<Vec<u8>, String>> {
        Some(self.inflate(self.entry(name)?))
    }

    /// Like [`get`](Self::get), but an entry whose checksum does not match
    /// is an error. For archives Fanimate wrote itself.
    pub fn get_checked(&self, name: &str) -> Option<Result<Vec<u8>, String>> {
        let entry = self.entry(name)?;
        Some(self.inflate(entry).and_then(|bytes| {
            if crc32(&bytes) == entry.crc {
                Ok(bytes)
            } else {
                Err(format!("\"{name}\" is damaged (its checksum does not match)"))
            }
        }))
    }

    fn inflate(&self, e: Entry) -> Result<Vec<u8>, String> {
        let raw = self
            .bytes
            .get(e.data..e.data.saturating_add(e.compressed))
            .ok_or("an archive entry is cut short")?;
        match e.method {
            0 => Ok(raw.to_vec()),
            8 => {
                let limit = if e.size > 0 {
                    e.size.min(MAX_ENTRY)
                } else {
                    MAX_ENTRY
                };
                miniz_oxide::inflate::decompress_to_vec_with_limit(raw, limit)
                    .map_err(|_| "an archive entry is damaged".to_owned())
            }
            m => Err(format!(
                "an archive entry uses compression method {m}, which is not supported"
            )),
        }
    }
}

/// CRC-32 (the ZIP and PNG one) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, slot) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 {
                    0xEDB8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
            }
            *slot = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, b| {
        table[((c ^ u32::from(*b)) & 0xff) as usize] ^ (c >> 8)
    })
}

/// Pack `files` (name and contents, in this order) into an archive. Each
/// entry is deflated when that makes it smaller, stored otherwise (pictures
/// and most sounds are already compressed).
pub fn write(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    const TOO_BIG: &str = "the project is too large for a single .fnm file (4 GiB)";
    let count = u16::try_from(files.len()).map_err(|_| "too many files for a single .fnm file")?;
    let u32_of = |n: usize| u32::try_from(n).map_err(|_| TOO_BIG.to_owned());
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let name_len = u16::try_from(name.len()).map_err(|_| format!("the name \"{name}\" is too long"))?;
        let crc = crc32(data);
        let deflated = miniz_oxide::deflate::compress_to_vec(data, LEVEL);
        let (method, body): (u16, &[u8]) = if deflated.len() < data.len() {
            (8, &deflated)
        } else {
            (0, data)
        };
        let (size, packed, offset) = (u32_of(data.len())?, u32_of(body.len())?, u32_of(out.len())?);

        out.extend_from_slice(LOCAL);
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&UTF8_NAMES.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // time
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&packed.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra field
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body);

        central.extend_from_slice(CENTRAL);
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&UTF8_NAMES.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&packed.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        // extra, comment, disk number, internal and external attributes
        central.extend_from_slice(&[0; 2 + 2 + 2 + 2 + 4]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let directory_at = u32_of(out.len())?;
    let directory_len = u32_of(central.len())?;
    u32_of(out.len() + central.len() + 22)?;
    out.extend_from_slice(&central);
    out.extend_from_slice(END);
    out.extend_from_slice(&[0; 4]); // this disk, disk with the directory
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&directory_len.to_le_bytes());
    out.extend_from_slice(&directory_at.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn written_archives_read_back_exactly() {
        let big = vec![7u8; 100_000];
        let noise: Vec<u8> = (0..4000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let zip = write(&[
            ("a.json", b"{\"x\":1}"),
            ("big", &big),
            ("noise", &noise),
            ("empty", b""),
        ])
        .unwrap();
        assert!(is_zip(&zip));
        let a = Archive::read(&zip).unwrap();
        assert_eq!(a.get_checked("a.json").unwrap().unwrap(), b"{\"x\":1}");
        assert_eq!(a.get_checked("big").unwrap().unwrap(), big);
        assert_eq!(a.get_checked("noise").unwrap().unwrap(), noise);
        assert_eq!(a.get_checked("empty").unwrap().unwrap(), b"");
        // the repetitive entry was deflated, so the archive is far smaller than its contents
        assert!(zip.len() < 10_000, "{}", zip.len());
        // the same input gives the same bytes
        assert_eq!(
            zip,
            write(&[
                ("a.json", b"{\"x\":1}"),
                ("big", &big),
                ("noise", &noise),
                ("empty", b"")
            ])
            .unwrap()
        );
    }

    #[test]
    fn reads_entries_case_insensitively_and_skips_folders() {
        let zip = write(&[("DOMDocument.xml", b"<x/>"), ("LIBRARY/", b"")]).unwrap();
        let a = Archive::read(&zip).unwrap();
        assert_eq!(a.names().collect::<Vec<_>>(), ["DOMDocument.xml"]);
        assert_eq!(a.get("domdocument.xml").unwrap().unwrap(), b"<x/>");
        assert!(a.get("missing").is_none());
    }

    #[test]
    fn a_wrong_directory_offset_is_recovered() {
        let mut zip = write(&[("a", b"1")]).unwrap();
        let n = zip.len();
        zip[n - 6..n - 2].copy_from_slice(&999u32.to_le_bytes());
        assert_eq!(Archive::read(&zip).unwrap().get("a").unwrap().unwrap(), b"1");
    }

    #[test]
    fn a_changed_byte_fails_the_checksum() {
        let mut zip = write(&[("a", b"12345")]).unwrap();
        let at = zip.windows(5).position(|w| w == b"12345").unwrap();
        zip[at] ^= 1;
        let a = Archive::read(&zip).unwrap();
        assert!(a.get_checked("a").unwrap().is_err());
        // the lenient read still gives the bytes, as the importers need
        assert!(a.get("a").unwrap().is_ok());
    }

    #[test]
    fn damaged_archives_are_errors_not_panics() {
        assert!(Archive::read(b"PK\x03\x04garbage").is_err());
        let zip = write(&[("a", b"12345"), ("b", &[9u8; 5000])]).unwrap();
        for cut in 0..zip.len() {
            if let Ok(a) = Archive::read(&zip[..cut]) {
                for name in ["a", "b"] {
                    let _ = a.get_checked(name);
                }
            }
        }
        let mut flipped = zip.clone();
        for i in 0..flipped.len() {
            flipped[i] ^= 0xff;
            if let Ok(a) = Archive::read(&flipped) {
                let _ = a.get_checked("a");
                let _ = a.get_checked("b");
            }
            flipped[i] ^= 0xff;
        }
    }
}
