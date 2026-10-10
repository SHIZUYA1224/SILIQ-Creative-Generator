use super::*;
use design_model::{SUPPORTED_SCHEMA_VERSIONS, load_document, validate::is_sha256_hex};
use std::collections::HashSet;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

pub(super) struct CheckedPackage {
    pub manifest: Manifest,
    pub document: DesignDocument,
    pub entries: Entries,
}

pub(super) fn write(file: File, manifest: &Manifest, entries: &Entries) -> Result<File, Issue> {
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    writer.start_file("manifest.json", options).map_err(fs::io_error)?;
    writer
        .write_all(&serde_json::to_vec_pretty(manifest).map_err(fs::io_error)?)
        .map_err(fs::io_error)?;
    for (path, bytes) in entries {
        writer.start_file(path, options).map_err(fs::io_error)?;
        writer.write_all(bytes).map_err(fs::io_error)?;
    }
    writer.finish().map_err(fs::io_error)
}

pub(super) fn entry_limit(path: &str) -> Result<u64, Issue> {
    if matches!(path, "manifest.json" | "design.json" | "fonts/manifest.json") {
        return Ok(MAX_JSON_BYTES);
    }
    let (sub, file) = path.split_once('/').ok_or_else(|| invalid("unexpected package entry"))?;
    let (sha, ext) = file.split_once('.').ok_or_else(|| invalid("invalid resource entry name"))?;
    if !is_sha256_hex(sha) {
        return Err(invalid("invalid resource content hash"));
    }
    match (sub, ext) {
        ("assets", "png" | "jpg" | "webp") => Ok(MAX_IMAGE_BYTES),
        ("assets", "svg") => Ok(10 * 1024 * 1024),
        ("fonts", "ttf" | "otf" | "ttc") => Ok(MAX_FONT_BYTES),
        _ => Err(invalid("unexpected resource entry type")),
    }
}

pub(super) fn read(mut file: File) -> Result<CheckedPackage, Issue> {
    // Check the raw directory first: ZIP libraries may collapse duplicate
    // names before exposing len(), and may allocate from untrusted counts.
    let names = inspect_container(&mut file)?;
    file.seek(SeekFrom::Start(0)).map_err(fs::io_error)?;
    let mut archive = ZipArchive::new(file).map_err(|_| invalid("malformed ZIP container"))?;
    if archive.len() != names.len() {
        return Err(invalid("ZIP entry count is inconsistent"));
    }
    let mut entries = Entries::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| invalid("ZIP entry cannot be read"))?;
        let name = entry.name().to_owned();
        if !names.contains(&name)
            || !entry.is_file()
            || entry.is_symlink()
            || entry.encrypted()
            || entry.compression() != CompressionMethod::Stored
        {
            return Err(invalid("unsupported or unsafe ZIP entry"));
        }
        let maximum = entry_limit(&name)?;
        let size = entry.size();
        if size > maximum {
            return Err(limit());
        }
        let mut bytes = Vec::with_capacity(size as usize);
        entry
            .take(maximum + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("ZIP entry is corrupt or its CRC is inconsistent"))?;
        if bytes.len() as u64 != size || bytes.len() as u64 > maximum {
            return Err(invalid("ZIP entry size is inconsistent"));
        }
        total = total.checked_add(bytes.len() as u64).ok_or_else(limit)?;
        if total > MAX_PACKAGE_BYTES {
            return Err(limit());
        }
        if entries.insert(name, Arc::new(bytes)).is_some() {
            return Err(invalid("duplicate ZIP entry"));
        }
    }
    let manifest_bytes = entries.remove("manifest.json").ok_or_else(|| invalid("manifest.json is missing"))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(|_| invalid("project manifest is malformed"))?;
    if manifest.format != PACKAGE_FORMAT
        || manifest.format_version != FORMAT_VERSION
        || !SUPPORTED_SCHEMA_VERSIONS.contains(&manifest.schema_version.as_str())
    {
        return Err(invalid("project format or schema version is unsupported"));
    }
    if version(&manifest.minimum_engine_version).is_none_or(|v| v > version(ENGINE_VERSION).unwrap()) {
        return Err(invalid("project requires a newer or unknown engine version"));
    }
    if manifest.entries.len() != entries.len() {
        return Err(invalid("manifest entry count is inconsistent"));
    }
    let mut declared = HashSet::new();
    for digest in &manifest.entries {
        entry_limit(&digest.path)?;
        if !declared.insert(&digest.path) {
            return Err(invalid("duplicate manifest entry"));
        }
        let bytes = entries
            .get(&digest.path)
            .ok_or_else(|| invalid("manifest references a missing entry"))?;
        if !is_sha256_hex(&digest.sha256) || digest.size != bytes.len() as u64 || digest.sha256 != sha256_hex(bytes) {
            return Err(Issue::error(
                Code::AssetChecksum,
                "project entry checksum or size does not match",
                "Use an intact copy of the project; the source file was not changed.",
            ));
        }
        if digest.path.starts_with("assets/") || (digest.path.starts_with("fonts/") && digest.path != "fonts/manifest.json") {
            let file = digest.path.split('/').nth(1).unwrap();
            if !file.starts_with(&format!("{}.", digest.sha256)) {
                return Err(invalid("resource filename does not match its content hash"));
            }
        }
    }
    let document = load_document(entries.get("design.json").ok_or_else(|| invalid("design.json is missing"))?)
        .map_err(|issues| issues.into_iter().next().unwrap_or_else(|| invalid("design document is invalid")))?;
    if document.document_id != manifest.document_id
        || document.revision != manifest.revision
        || document.schema_version != manifest.schema_version
    {
        return Err(invalid("document identity, revision or version differs from the manifest"));
    }
    let font_manifest: FontManifest = serde_json::from_slice(
        entries
            .get("fonts/manifest.json")
            .ok_or_else(|| invalid("font manifest is missing"))?,
    )
    .map_err(|_| invalid("font manifest is malformed"))?;
    validate_references(&document, &font_manifest, &entries)?;
    Ok(CheckedPackage {
        manifest,
        document,
        entries,
    })
}

fn version(s: &str) -> Option<(u32, u32, u32)> {
    let mut values = s.split('.');
    let value = (
        values.next()?.parse().ok()?,
        values.next()?.parse().ok()?,
        values.next()?.parse().ok()?,
    );
    if values.next().is_some() { None } else { Some(value) }
}

fn validate_references(doc: &DesignDocument, fonts: &FontManifest, entries: &Entries) -> Result<(), Issue> {
    let mut allowed: HashSet<String> = ["design.json".into(), "fonts/manifest.json".into()].into_iter().collect();
    for asset in &doc.assets {
        if asset.state == ResolveState::Resolved {
            let path = format!(
                "assets/{}.{}",
                asset.sha256.as_ref().ok_or_else(|| invalid("asset hash is missing"))?,
                asset.mime_hint.extension()
            );
            if !entries.contains_key(&path) {
                return Err(invalid("resolved asset bytes are missing"));
            }
            allowed.insert(path);
        }
    }
    if fonts.fonts.len() != doc.fonts.len() {
        return Err(invalid("font catalog differs from the document"));
    }
    for (reference, font) in fonts.fonts.iter().zip(&doc.fonts) {
        if reference.font != *font {
            return Err(invalid("font catalog differs from the document"));
        }
        match &reference.embedded_path {
            Some(path) => {
                if font.package_policy != PackagePolicy::EmbedConfirmed || font.state != ResolveState::Resolved {
                    return Err(invalid("font bytes were embedded without confirmed permission"));
                }
                let prefix = format!("fonts/{}.", font.sha256.as_ref().ok_or_else(|| invalid("font hash is missing"))?);
                if !path.starts_with(&prefix) || !entries.contains_key(path) {
                    return Err(invalid("embedded font reference is inconsistent"));
                }
                entry_limit(path)?;
                allowed.insert(path.clone());
            }
            None if font.package_policy == PackagePolicy::EmbedConfirmed && font.state == ResolveState::Resolved => {
                return Err(invalid("confirmed embedded font bytes are missing"));
            }
            None => {}
        }
    }
    if entries.keys().any(|path| !allowed.contains(path)) {
        return Err(invalid("unreferenced or unapproved resource bytes in project"));
    }
    Ok(())
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

/// Strict Stored ZIP profile, validated before allocating archive entries.
fn inspect_container(file: &mut File) -> Result<HashSet<String>, Issue> {
    let length = file.metadata().map_err(fs::io_error)?.len();
    if !(22..=MAX_PACKAGE_BYTES + 16 * 1024 * 1024).contains(&length) {
        return Err(limit());
    }
    let tail_len = length.min(65_557) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64))).map_err(fs::io_error)?;
    let mut tail = vec![0_u8; tail_len];
    file.read_exact(&mut tail).map_err(fs::io_error)?;
    let end = (0..=tail_len - 22)
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] && i + 22 + u16_at(&tail, i + 20) as usize == tail_len)
        .ok_or_else(|| invalid("ZIP end record is missing or truncated"))?;
    if u16_at(&tail, end + 4) != 0 || u16_at(&tail, end + 6) != 0 || u16_at(&tail, end + 8) != u16_at(&tail, end + 10) {
        return Err(invalid("multi-volume ZIP is unsupported"));
    }
    let count = u16_at(&tail, end + 10) as usize;
    if count == 0 || count > MAX_ENTRIES {
        return Err(limit());
    }
    let directory_size = u32_at(&tail, end + 12) as u64;
    let directory_start = u32_at(&tail, end + 16) as u64;
    let end_position = length - tail_len as u64 + end as u64;
    if directory_size > 8 * 1024 * 1024 || directory_start.checked_add(directory_size) != Some(end_position) {
        return Err(invalid("ZIP directory size or offset is inconsistent"));
    }
    file.seek(SeekFrom::Start(directory_start)).map_err(fs::io_error)?;
    let mut directory = vec![0_u8; directory_size as usize];
    file.read_exact(&mut directory).map_err(fs::io_error)?;
    let mut position = 0_usize;
    let mut names = HashSet::new();
    let mut ranges = Vec::new();
    let mut total = 0_u64;
    for _ in 0..count {
        if position + 46 > directory.len() || directory[position..position + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err(invalid("ZIP central record is invalid"));
        }
        let record = &directory[position..];
        let flags = u16_at(record, 8);
        let method = u16_at(record, 10);
        let compressed = u32_at(record, 20) as u64;
        let expanded = u32_at(record, 24) as u64;
        let name_len = u16_at(record, 28) as usize;
        let extra_len = u16_at(record, 30) as usize;
        let comment_len = u16_at(record, 32) as usize;
        let disk = u16_at(record, 34);
        let attributes = u32_at(record, 38);
        let local_start = u32_at(record, 42) as u64;
        let record_len = 46 + name_len + extra_len + comment_len;
        if record_len > record.len() || extra_len != 0 || disk != 0 || method != 0 || flags & !0x0800 != 0 || compressed != expanded {
            return Err(invalid("compressed, encrypted, extended or malformed ZIP entry is unsupported"));
        }
        let mode = attributes >> 16;
        if (mode & 0o170000 != 0 && mode & 0o170000 != 0o100000) || attributes & 0x10 != 0 {
            return Err(invalid("ZIP links and non-file entries are refused"));
        }
        let raw_name = &record[46..46 + name_len];
        let name = std::str::from_utf8(raw_name).map_err(|_| invalid("ZIP entry name must be portable ASCII"))?;
        if !name.is_ascii() || !names.insert(name.to_owned()) {
            return Err(invalid("duplicate or nonportable ZIP entry name"));
        }
        let maximum = entry_limit(name)?; // Rejects traversal, NUL, case aliases.
        if expanded > maximum {
            return Err(limit());
        }
        total = total.checked_add(expanded).ok_or_else(limit)?;
        if total > MAX_PACKAGE_BYTES {
            return Err(limit());
        }
        // Verify local headers too: central/local filename differences can
        // otherwise cause interpretation differences between ZIP readers.
        file.seek(SeekFrom::Start(local_start)).map_err(fs::io_error)?;
        let mut local = [0_u8; 30];
        file.read_exact(&mut local).map_err(|_| invalid("ZIP local header is truncated"))?;
        if local[..4] != [0x50, 0x4b, 0x03, 0x04]
            || u16_at(&local, 6) != flags
            || u16_at(&local, 8) != method
            || u16_at(&local, 26) as usize != name_len
            || u16_at(&local, 28) != 0
        {
            return Err(invalid("ZIP local header differs from its directory"));
        }
        if flags & 8 == 0
            && (u32_at(&local, 18) as u64 != compressed
                || u32_at(&local, 22) as u64 != expanded
                || u32_at(&local, 14) != u32_at(record, 16))
        {
            return Err(invalid("ZIP local size or CRC differs from its directory"));
        }
        let mut local_name = vec![0_u8; name_len];
        file.read_exact(&mut local_name)
            .map_err(|_| invalid("ZIP local filename is truncated"))?;
        if local_name != raw_name {
            return Err(invalid("ZIP local filename differs from its directory"));
        }
        let data_end = local_start
            .checked_add(30 + name_len as u64)
            .and_then(|v| v.checked_add(compressed))
            .ok_or_else(limit)?;
        if data_end > directory_start {
            return Err(invalid("ZIP entry overlaps its directory"));
        }
        ranges.push((local_start, data_end));
        position += record_len;
    }
    if position != directory.len() {
        return Err(invalid("ZIP directory contains trailing records"));
    }
    ranges.sort_unstable();
    if ranges.first().is_none_or(|r| r.0 != 0)
        || ranges.last().is_none_or(|r| r.1 != directory_start)
        || ranges.windows(2).any(|r| r[0].1 != r[1].0)
    {
        return Err(invalid("ZIP has overlapping entries, unrecorded data or a prefixed executable"));
    }
    Ok(names)
}
