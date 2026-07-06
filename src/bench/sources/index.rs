use anyhow::{Context, Result};
use rand::Rng;
use rand::SeedableRng;
use rand::rngs::StdRng;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

const INDEX_MAGIC: u32 = 0x47435449;
const INDEX_VERSION: u32 = 5; // v5: single-file + typed keys + CRC32 checksum

fn json_value_to_string(v: Option<&serde_json::Value>) -> String {
    match v {
        None => String::new(),
        Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        Some(serde_json::Value::Bool(b)) => b.to_string(),
        Some(other) => other.to_string(),
    }
}

const FLAG_HAS_UNICODE: u64 = 1 << 62;
const FLAG_HAS_METADATA: u64 = 1 << 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    String,
    U64,
    I64,
    U32,
    I32,
    UnixTimestampSec,
    UnixTimestampMillis,
    DatePacked,
    TimePacked,
    UUID,
    ULID,
}

impl KeyType {
    pub fn parse(&self, value: &str) -> Option<KeyValue> {
        match self {
            KeyType::String => Some(KeyValue::String(value.to_string())),
            KeyType::U64 => value.parse::<u64>().ok().map(KeyValue::U64),
            KeyType::I64 => value.parse::<i64>().ok().map(KeyValue::I64),
            KeyType::U32 => value.parse::<u32>().ok().map(KeyValue::U32),
            KeyType::I32 => value.parse::<i32>().ok().map(KeyValue::I32),
            KeyType::UnixTimestampSec => value.parse::<i64>().ok().map(KeyValue::I64),
            KeyType::UnixTimestampMillis => value.parse::<i64>().ok().map(KeyValue::I64),
            KeyType::DatePacked => parse_date(value).map(KeyValue::U32),
            KeyType::TimePacked => parse_time(value).map(KeyValue::U32),
            KeyType::UUID => parse_uuid(value).map(KeyValue::UUID),
            KeyType::ULID => parse_ulid(value).map(KeyValue::ULID),
        }
    }

    pub fn supports_numeric_vec(&self) -> bool {
        matches!(
            self,
            KeyType::U64
                | KeyType::I64
                | KeyType::U32
                | KeyType::I32
                | KeyType::DatePacked
                | KeyType::TimePacked
                | KeyType::UUID
                | KeyType::ULID
        )
    }

    pub fn id(&self) -> u8 {
        match self {
            KeyType::String => 0,
            KeyType::U64 => 1,
            KeyType::I64 => 2,
            KeyType::U32 => 3,
            KeyType::I32 => 4,
            KeyType::UnixTimestampSec => 5,
            KeyType::UnixTimestampMillis => 6,
            KeyType::DatePacked => 7,
            KeyType::TimePacked => 8,
            KeyType::UUID => 9,
            KeyType::ULID => 10,
        }
    }

    pub fn from_id(id: u8) -> Option<Self> {
        match id {
            0 => Some(KeyType::String),
            1 => Some(KeyType::U64),
            2 => Some(KeyType::I64),
            3 => Some(KeyType::U32),
            4 => Some(KeyType::I32),
            5 => Some(KeyType::UnixTimestampSec),
            6 => Some(KeyType::UnixTimestampMillis),
            7 => Some(KeyType::DatePacked),
            8 => Some(KeyType::TimePacked),
            9 => Some(KeyType::UUID),
            10 => Some(KeyType::ULID),
            _ => None,
        }
    }
}

impl Default for KeyType {
    fn default() -> Self {
        KeyType::String
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyValue {
    String(String),
    U64(u64),
    I64(i64),
    U32(u32),
    I32(i32),
    UUID(UuidParts),
    ULID(UlidParts),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UuidParts(pub u64, pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct UlidParts(pub u64, pub u64);

fn parse_uuid(s: &str) -> Option<UuidParts> {
    let s = s.trim();
    let bytes = parse_hex_bytes(s)?;
    if bytes.len() != 16 {
        return None;
    }
    let p0 = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let p1 = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    Some(UuidParts(p0, p1))
}

fn parse_ulid(s: &str) -> Option<UlidParts> {
    const BASE32_CHARS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let s = s.trim();
    if s.len() != 26 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, c) in s.bytes().enumerate() {
        let c = c.to_ascii_uppercase();
        let idx = BASE32_CHARS.iter().position(|&x| x == c)?;
        bytes[i / 2] = bytes[i / 2] * 32 + idx as u8;
    }
    let p0 = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let p1 = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    Some(UlidParts(p0, p1))
}

fn parse_hex_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.replace(['-', ':'], "");
    if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..16)
        .map(|i| {
            let idx = i * 2;
            u8::from_str_radix(&s[idx..idx + 2], 16).ok()
        })
        .collect()
}

fn parse_date(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(&['-', '/'][..]).collect();
    if parts.len() != 3 {
        return None;
    }
    let year: u32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;
    if year < 1900 || year > 2100 || month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }
    Some(year * 10000 + month * 100 + day)
}

fn parse_time(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let hour: u32 = parts[0].parse().ok()?;
    let min: u32 = parts[1].parse().ok()?;
    let sec = if parts.len() > 2 {
        parts[2].parse().ok()?
    } else {
        0
    };
    if hour > 23 || min > 59 || sec > 59 {
        return None;
    }
    Some(hour * 10000 + min * 100 + sec)
}

pub fn infer_key_type(samples: &[&str]) -> KeyType {
    if samples.is_empty() {
        return KeyType::String;
    }
    if samples.iter().all(|&s| is_uuid(s)) {
        return KeyType::UUID;
    }
    if samples.iter().all(|&s| is_ulid(s)) {
        return KeyType::ULID;
    }
    if samples.iter().all(|&s| is_date(s)) {
        return KeyType::DatePacked;
    }
    if samples.iter().all(|&s| is_time(s)) {
        return KeyType::TimePacked;
    }
    if samples.iter().all(|&s| s.parse::<u64>().is_ok()) {
        return KeyType::U64;
    }
    if samples.iter().all(|&s| s.parse::<i64>().is_ok()) {
        return KeyType::I64;
    }
    KeyType::String
}

pub struct InferenceStats {
    pub samples_taken: usize,
    pub bytes_scanned: u64,
    pub all_matched: bool,
    pub confidence: f32,
}

pub fn infer_key_type_from_stream<R: Read + Seek + BufRead>(
    reader: &mut R,
    key_column_idx: usize,
    max_samples: usize,
    max_bytes_scan: u64,
) -> Result<(KeyType, InferenceStats)> {
    use std::io::SeekFrom;

    let file_size = reader.seek(SeekFrom::End(0))?;
    if file_size == 0 {
        return Ok((
            KeyType::String,
            InferenceStats {
                samples_taken: 0,
                bytes_scanned: 0,
                all_matched: false,
                confidence: 0.0,
            },
        ));
    }

    let mut samples = Vec::with_capacity(max_samples.min(1000));
    let mut bytes_scanned = 0u64;
    let mut rng = StdRng::from_entropy();

    let num_samples = max_samples.min(1000);
    let sample_positions: Vec<u64> = (0..num_samples)
        .map(|_| rng.gen_range(0..file_size))
        .collect();

    for pos in sample_positions {
        if bytes_scanned >= max_bytes_scan {
            break;
        }

        reader.seek(SeekFrom::Start(pos))?;
        if pos > 0 {
            let mut dummy = String::new();
            reader.read_line(&mut dummy).ok();
        }

        let mut this_line = String::new();
        match reader.read_line(&mut this_line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        bytes_scanned += this_line.len() as u64;

        let trimmed = this_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = if trimmed.contains('\t') {
            trimmed.split('\t').collect()
        } else {
            trimmed.split(',').collect()
        };

        if key_column_idx >= parts.len() {
            continue;
        }

        samples.push(parts[key_column_idx].to_string());
    }

    if samples.is_empty() {
        return Ok((
            KeyType::String,
            InferenceStats {
                samples_taken: 0,
                bytes_scanned,
                all_matched: false,
                confidence: 0.0,
            },
        ));
    }

    let sample_refs: Vec<&str> = samples.iter().map(|s| s.as_str()).collect();
    let inferred = infer_key_type(&sample_refs);
    let all_matched = match inferred {
        KeyType::String => true,
        _ => samples.iter().all(|s| inferred.parse(s).is_some()),
    };

    let confidence = if samples.len() >= max_samples.min(1000) {
        1.0
    } else {
        (samples.len() as f32 / max_samples.min(1000) as f32).min(1.0)
    };

    Ok((
        inferred,
        InferenceStats {
            samples_taken: samples.len(),
            bytes_scanned,
            all_matched,
            confidence,
        },
    ))
}

pub fn infer_key_type_from_ndjson_stream<R: Read + Seek + BufRead>(
    reader: &mut R,
    key_column: &str,
    max_samples: usize,
    max_bytes_scan: u64,
) -> Result<(KeyType, InferenceStats)> {
    use std::io::SeekFrom;

    let file_size = reader.seek(SeekFrom::End(0))?;
    if file_size == 0 {
        return Ok((
            KeyType::String,
            InferenceStats {
                samples_taken: 0,
                bytes_scanned: 0,
                all_matched: false,
                confidence: 0.0,
            },
        ));
    }

    let mut samples = Vec::with_capacity(max_samples.min(1000));
    let mut bytes_scanned = 0u64;
    let mut rng = StdRng::from_entropy();

    let num_samples = max_samples.min(1000);
    let sample_positions: Vec<u64> = (0..num_samples)
        .map(|_| rng.gen_range(0..file_size))
        .collect();

    for pos in sample_positions {
        if bytes_scanned >= max_bytes_scan {
            break;
        }

        reader.seek(SeekFrom::Start(pos))?;
        if pos > 0 {
            let mut dummy = String::new();
            reader.read_line(&mut dummy).ok();
        }

        let mut this_line = String::new();
        match reader.read_line(&mut this_line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }

        bytes_scanned += this_line.len() as u64;

        let trimmed = this_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let obj: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(trimmed) {
            Ok(o) => o,
            Err(_) => continue,
        };

        let value = match obj.get(key_column) {
            Some(v) => json_value_to_string(Some(v)),
            None => continue,
        };

        samples.push(value);
    }

    if samples.is_empty() {
        return Ok((
            KeyType::String,
            InferenceStats {
                samples_taken: 0,
                bytes_scanned,
                all_matched: false,
                confidence: 0.0,
            },
        ));
    }

    let sample_refs: Vec<&str> = samples.iter().map(|s| s.as_str()).collect();
    let inferred = infer_key_type(&sample_refs);
    let all_matched = match inferred {
        KeyType::String => true,
        _ => samples.iter().all(|s| inferred.parse(s).is_some()),
    };

    let confidence = if samples.len() >= max_samples.min(1000) {
        1.0
    } else {
        (samples.len() as f32 / max_samples.min(1000) as f32).min(1.0)
    };

    Ok((
        inferred,
        InferenceStats {
            samples_taken: samples.len(),
            bytes_scanned,
            all_matched,
            confidence,
        },
    ))
}

fn is_uuid(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 36 {
        return false;
    }
    let expected = "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
    let mut si = s.bytes();
    for c in expected.bytes() {
        if c == b'x' {
            if !si.next().map_or(false, |b| b.is_ascii_hexdigit()) {
                return false;
            }
        } else if si.next() != Some(c) {
            return false;
        }
    }
    true
}

fn is_ulid(s: &str) -> bool {
    const VALID_ULID_CHARS: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let s = s.trim();
    if s.len() != 26 {
        return false;
    }
    s.bytes()
        .all(|b| VALID_ULID_CHARS.contains(&b.to_ascii_uppercase()))
}

fn is_date(s: &str) -> bool {
    parse_date(s).is_some()
}

fn is_time(s: &str) -> bool {
    parse_time(s).is_some()
}

#[derive(Debug, Clone)]
pub struct BloomFilter {
    bits: Vec<u64>,
    hash_count: u32,
    bit_count: usize,
}

impl BloomFilter {
    pub fn new(expected_elements: usize, false_positive_rate: f64) -> Self {
        let m = Self::optimal_bit_count(expected_elements, false_positive_rate);
        let k = Self::optimal_hash_count(m, expected_elements);
        let bits = vec![0u64; (m + 63) / 64];
        Self {
            bits,
            hash_count: k as u32,
            bit_count: m,
        }
    }

    pub fn with_capacity(bit_count: usize, hash_count: u32) -> Self {
        let bits = vec![0u64; (bit_count + 63) / 64];
        Self {
            bits,
            hash_count,
            bit_count,
        }
    }

    fn optimal_bit_count(n: usize, p: f64) -> usize {
        let n = n.max(1) as f64;
        let p = p.clamp(0.0001, 0.9999);
        let m = -n * p.ln() / (std::f64::consts::LN_2 * std::f64::consts::LN_2);
        m.ceil() as usize
    }

    fn optimal_hash_count(m: usize, n: usize) -> usize {
        let m = m.max(1) as f64;
        let n = n.max(1) as f64;
        let k = (m / n * std::f64::consts::LN_2).ceil();
        k.max(1.0) as usize
    }

    pub fn insert(&mut self, key: &str) {
        for i in 0..self.hash_count {
            let idx = self.hash(key, i);
            self.bits[idx / 64] |= 1 << (idx % 64);
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        for i in 0..self.hash_count {
            let idx = self.hash(key, i);
            if self.bits[idx / 64] & (1 << (idx % 64)) == 0 {
                return false;
            }
        }
        true
    }

    fn hash(&self, key: &str, salt: u32) -> usize {
        let h1 = Self::fnv1a(key, 0);
        let h2 = Self::fnv1a(key, 0xdeadbeef);
        let combined = (h1 as u64).wrapping_add((h2 as u64).wrapping_mul(salt as u64));
        (combined as usize) % self.bit_count
    }

    fn fnv1a(data: &str, salt: u32) -> u32 {
        let mut hash: u32 = 2166136261u32.wrapping_add(salt);
        for byte in data.bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(16777619);
        }
        hash
    }

    pub fn bit_count(&self) -> usize {
        self.bit_count
    }

    pub fn hash_count(&self) -> u32 {
        self.hash_count
    }

    pub fn memory_bits(&self) -> usize {
        self.bits.len() * 64
    }

    pub fn write_to(&self, writer: &mut impl Write) -> Result<()> {
        writer.write_all(&self.bit_count.to_le_bytes())?;
        writer.write_all(&self.hash_count.to_le_bytes())?;
        for chunk in self.bits.chunks(8192) {
            let bytes = chunk
                .iter()
                .fold(Vec::with_capacity(chunk.len() * 8), |mut acc, &w| {
                    acc.extend_from_slice(&w.to_le_bytes());
                    acc
                });
            writer.write_all(&bytes)?;
        }
        Ok(())
    }

    pub fn read_from(reader: &mut impl Read) -> Result<Self> {
        let mut bit_buf = [0u8; 8];
        reader.read_exact(&mut bit_buf)?;
        let bit_count = usize::from_le_bytes(bit_buf);

        let mut hash_buf = [0u8; 4];
        reader.read_exact(&mut hash_buf)?;
        let hash_count = u32::from_le_bytes(hash_buf);

        let bits = vec![0u64; (bit_count + 63) / 64];
        let mut result = Self {
            bits,
            hash_count,
            bit_count,
        };

        let byte_count = (bit_count + 7) / 8;
        let mut buf = vec![0u8; byte_count];
        reader.read_exact(&mut buf)?;
        for (i, chunk) in buf.chunks(8).enumerate() {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            result.bits[i] = u64::from_le_bytes(word);
        }
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct XorFilter {
    fingerprint_bits: u32,
    array_size: usize,
    seed: u64,
    fingerprints: Vec<u8>,
}

impl XorFilter {
    pub fn new(expected_elements: usize, false_positive_rate: f64) -> Self {
        let fpr = false_positive_rate.clamp(0.0001, 0.9999);
        let fingerprint_bits = Self::optimal_fingerprint_bits(fpr);
        let array_size = Self::optimal_array_size(expected_elements);
        let fingerprints = vec![0u8; array_size];
        Self {
            fingerprint_bits,
            array_size,
            seed: 0x9e3779b97f4a7c15,
            fingerprints,
        }
    }

    fn optimal_fingerprint_bits(p: f64) -> u32 {
        let p = p.clamp(0.0001, 0.9999);
        ((-p.log2()).ceil() as u32).max(4).min(16)
    }

    fn optimal_array_size(n: usize) -> usize {
        let n = n.max(1);
        let c = 1.23;
        ((n as f64) * c).ceil() as usize
    }

    fn murmurhash64(data: &[u8], seed: u64) -> u64 {
        let c1: u64 = 0x9e3779b97f4a7c15;
        let c2: u64 = 0x9e3779b97f4a7c15;
        let mut h: u64 = seed;
        let len = data.len();

        let mut i = 0;
        while i + 8 <= len {
            let mut k = u64::from_le_bytes([
                data[i],
                data[i + 1],
                data[i + 2],
                data[i + 3],
                data[i + 4],
                data[i + 5],
                data[i + 6],
                data[i + 7],
            ]);
            k = k.wrapping_mul(c1);
            k = k.rotate_left(31);
            k = k.wrapping_mul(c2);
            h ^= k;
            h = h.rotate_left(27);
            h = h.wrapping_add(0x9e3779b97f4a7c15);
            h = h.wrapping_mul(c1);
            i += 8;
        }

        let mut k: u64 = 0;
        match len % 8 {
            7 => k ^= (data[i + 6] as u64) << 48,
            6 => k ^= (data[i + 5] as u64) << 40,
            5 => k ^= (data[i + 4] as u64) << 32,
            4 => k ^= (data[i + 3] as u64) << 24,
            3 => k ^= (data[i + 2] as u64) << 16,
            2 => k ^= (data[i + 1] as u64) << 8,
            1 => k ^= data[i] as u64,
            _ => {}
        }
        if len % 8 != 0 {
            k ^= (len as u64).wrapping_mul(c2);
            k = k.wrapping_mul(c1);
            h ^= k;
            h = h.rotate_left(31);
            h = h.wrapping_mul(c2);
        } else {
            h ^= (len as u64).wrapping_mul(c1);
            h ^= h.rotate_left(31);
            h ^= h.rotate_right(33);
        }

        h = h.wrapping_add(h << 15);
        h ^= h.rotate_right(41);
        h = h.wrapping_add(h << 13);
        h ^= h.rotate_right(35);
        h = h.wrapping_add(h << 9);
        h ^= h.rotate_right(49);
        h = h.wrapping_add(h << 15);
        h ^= h.rotate_right(33);
        h = h.wrapping_add(h << 17);
        h ^= h.rotate_right(41);
        h
    }

    fn compute_positions(&self, key: &str) -> [usize; 3] {
        let h = Self::murmurhash64(key.as_bytes(), self.seed);
        let h2 = Self::murmurhash64(key.as_bytes(), self.seed ^ 0x9e3779b97f4a7c15);

        let mask = self.array_size - 1;
        let h0 = (h as usize) & mask;
        let h1 = ((h >> 32) as usize) & mask;
        let h2 = (h2 as usize) & mask;

        [h0, h1, h2]
    }

    fn compute_fingerprint(&self, key: &str) -> u8 {
        let h = Self::murmurhash64(key.as_bytes(), self.seed ^ 0xdeadbeef);
        let fb = self.fingerprint_bits as u32;
        ((h >> 32) ^ h) as u8 & ((1u8 << fb) - 1)
    }

    pub fn insert(&mut self, key: &str) -> bool {
        let [h0, h1, h2] = self.compute_positions(key);
        let f = self.compute_fingerprint(key);

        self.fingerprints[h0] ^= f;
        self.fingerprints[h1] ^= f;
        self.fingerprints[h2] ^= f;

        true
    }

    pub fn contains(&self, key: &str) -> bool {
        let [h0, h1, h2] = self.compute_positions(key);
        let f = self.compute_fingerprint(key);

        let expected = self.fingerprints[h0] ^ self.fingerprints[h1] ^ self.fingerprints[h2];
        expected == f
    }

    pub fn array_size(&self) -> usize {
        self.array_size
    }

    pub fn fingerprint_bits(&self) -> u32 {
        self.fingerprint_bits
    }

    pub fn memory_bits(&self) -> usize {
        self.fingerprints.len() * 8
    }

    pub fn build_from_keys(keys: &[String]) -> Option<Self> {
        let n = keys.len();
        if n == 0 {
            return Some(Self::new(1, 0.01));
        }

        let mut filter = Self::new(n, 0.01);
        let mut retry_count = 0;
        const MAX_RETRIES: usize = 100;

        for key in keys {
            if !filter.insert(key) {
                retry_count += 1;
                if retry_count >= MAX_RETRIES {
                    return None;
                }
                filter = Self::new(n, 0.01);
                for k in keys {
                    if !filter.insert(k) {
                        retry_count += 1;
                        if retry_count >= MAX_RETRIES {
                            return None;
                        }
                    }
                }
            }
        }

        Some(filter)
    }
}

#[derive(Debug, Clone)]
pub struct IndexEntry {
    pub offset: u64,
    pub row_length: u32,
}

#[derive(Debug, Clone)]
pub struct IndexEntryV4 {
    pub offset: u64,
    pub row_length: u32,
    pub has_unicode_suffix: bool,
    pub has_extended_metadata: bool,
}

impl IndexEntryV4 {
    pub fn new(offset: u64, row_length: u32) -> Self {
        Self {
            offset,
            row_length,
            has_unicode_suffix: false,
            has_extended_metadata: false,
        }
    }

    pub fn with_unicode(mut self) -> Self {
        self.has_unicode_suffix = true;
        self
    }

    pub fn with_metadata(mut self) -> Self {
        self.has_extended_metadata = true;
        self
    }

    pub fn encode(&self) -> u64 {
        let mut bits = self.offset & 0x3FFFFFFFFFFFFFFF;
        if self.has_unicode_suffix {
            bits |= FLAG_HAS_UNICODE;
        }
        if self.has_extended_metadata {
            bits |= FLAG_HAS_METADATA;
        }
        bits
    }

    pub fn decode(bits: u64) -> Self {
        let offset = bits & 0x3FFFFFFFFFFFFFFF;
        let has_unicode_suffix = (bits & FLAG_HAS_UNICODE) != 0;
        let has_extended_metadata = (bits & FLAG_HAS_METADATA) != 0;
        Self {
            offset,
            row_length: 0,
            has_unicode_suffix,
            has_extended_metadata,
        }
    }

    pub fn with_row_length(mut self, row_length: u32) -> Self {
        self.row_length = row_length;
        self
    }
}

#[derive(Debug)]
pub struct IndexHeader {
    pub version: u32,
    pub key_column: String,
    pub key_type: KeyType,
    pub data_offset: u64,
    pub entry_count: u32,
}

enum KeyStorage {
    String(BTreeMap<String, Vec<IndexEntry>>),
    Numeric(Vec<(KeyValue, String, Vec<IndexEntry>)>),
}

pub struct SourceIndex {
    storage: KeyStorage,
    header: IndexHeader,
    filter: Option<XorFilter>,
}

impl SourceIndex {
    pub fn new(key_column: &str) -> Self {
        Self {
            storage: KeyStorage::String(BTreeMap::new()),
            header: IndexHeader {
                version: INDEX_VERSION,
                key_column: key_column.to_string(),
                key_type: KeyType::String,
                data_offset: 0,
                entry_count: 0,
            },
            filter: None,
        }
    }

    pub fn with_key_type(key_column: &str, key_type: KeyType) -> Self {
        let storage = if key_type.supports_numeric_vec() {
            KeyStorage::Numeric(Vec::new())
        } else {
            KeyStorage::String(BTreeMap::new())
        };
        Self {
            storage,
            header: IndexHeader {
                version: INDEX_VERSION,
                key_column: key_column.to_string(),
                key_type,
                data_offset: 0,
                entry_count: 0,
            },
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: XorFilter) -> Self {
        self.filter = Some(filter);
        self
    }

    pub fn key_type(&self) -> KeyType {
        self.header.key_type
    }

    pub fn insert(&mut self, key: String, offset: u64, row_length: u32) -> Result<()> {
        let entry = IndexEntry { offset, row_length };
        match &mut self.storage {
            KeyStorage::String(map) => {
                map.entry(key).or_default().push(entry);
            }
            KeyStorage::Numeric(vec) => {
                let kv = match self.header.key_type.parse(&key) {
                    Some(kv) => kv,
                    None => anyhow::bail!(
                        "type mismatch at offset {}: key '{}' cannot be parsed as {:?}. \
                        Consider running `grpctestify index --force` to rebuild with correct type inference.",
                        offset,
                        key,
                        self.header.key_type
                    ),
                };
                match vec.binary_search_by(|e| e.0.cmp(&kv)) {
                    Ok(pos) => vec[pos].2.push(entry),
                    Err(pos) => vec.insert(pos, (kv, key.clone(), vec![entry])),
                }
            }
        }
        Ok(())
    }

    fn binary_search(&self, key: &str) -> Option<&IndexEntry> {
        match &self.storage {
            KeyStorage::String(map) => map.get(key).and_then(|v| v.first()),
            KeyStorage::Numeric(vec) => {
                let key_type = self.header.key_type;
                let kv = key_type.parse(key)?;
                vec.binary_search_by(|e| e.0.cmp(&kv))
                    .ok()
                    .map(|pos| &vec[pos].2[0])
            }
        }
    }

    pub fn lookup(&self, key: &str) -> Option<&IndexEntry> {
        if let Some(ref filter) = self.filter {
            if !filter.contains(key) {
                return None;
            }
        }
        self.binary_search(key)
    }

    pub fn lookup_all(&self, key: &str) -> Option<&[IndexEntry]> {
        if let Some(ref filter) = self.filter {
            if !filter.contains(key) {
                return None;
            }
        }
        match &self.storage {
            KeyStorage::String(map) => map.get(key).map(|v| v.as_slice()),
            KeyStorage::Numeric(vec) => {
                let key_type = self.header.key_type;
                let kv = key_type.parse(key)?;
                vec.binary_search_by(|e| e.0.cmp(&kv))
                    .ok()
                    .map(|pos| vec[pos].2.as_slice())
            }
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        if let Some(ref filter) = self.filter {
            if !filter.contains(key) {
                return false;
            }
        }
        self.lookup_all(key).is_some()
    }

    pub fn fast_negative_lookup(&self, key: &str) -> bool {
        if let Some(ref filter) = self.filter {
            return filter.contains(key);
        }
        true
    }

    pub fn lookup_range(&self, start: &str, end: &str) -> Vec<&IndexEntry> {
        let mut results = Vec::new();
        match &self.storage {
            KeyStorage::String(map) => {
                let start_s = start.to_string();
                let end_s = end.to_string();
                for (_key, entries) in map.range(start_s..=end_s) {
                    for entry in entries {
                        results.push(entry);
                    }
                }
            }
            KeyStorage::Numeric(vec) => {
                let key_type = self.header.key_type;
                let Some(start_kv) = key_type.parse(start) else {
                    return results;
                };
                let Some(end_kv) = key_type.parse(end) else {
                    return results;
                };
                let start_idx = match vec.binary_search_by(|e| e.0.cmp(&start_kv)) {
                    Ok(idx) => idx,
                    Err(idx) => idx,
                };
                for i in start_idx..vec.len() {
                    let (kv, _, entries) = &vec[i];
                    if kv > &end_kv {
                        break;
                    }
                    for entry in entries {
                        results.push(entry);
                    }
                }
            }
        }
        results
    }

    pub fn len(&self) -> usize {
        match &self.storage {
            KeyStorage::String(map) => map.values().map(Vec::len).sum(),
            KeyStorage::Numeric(vec) => vec.iter().map(|(_, _, entries)| entries.len()).sum(),
        }
    }

    pub fn unique_keys_len(&self) -> usize {
        match &self.storage {
            KeyStorage::String(map) => map.len(),
            KeyStorage::Numeric(vec) => vec.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.storage {
            KeyStorage::String(map) => map.is_empty(),
            KeyStorage::Numeric(vec) => vec.is_empty(),
        }
    }

    pub fn entry_count(&self) -> u32 {
        self.header.entry_count
    }

    pub fn index_version(&self) -> u32 {
        self.header.version
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &IndexEntry)> {
        match &self.storage {
            KeyStorage::String(map) => Box::new(
                map.iter()
                    .flat_map(|(k, vs)| vs.iter().map(move |v| (k.as_str(), v))),
            )
                as Box<dyn Iterator<Item = (&str, &IndexEntry)>>,
            KeyStorage::Numeric(vec) => Box::new(vec.iter().flat_map(|(_, key_str, vs)| {
                let k = key_str.as_str();
                vs.iter().map(move |v| (k, v))
            }))
                as Box<dyn Iterator<Item = (&str, &IndexEntry)>>,
        }
    }

    pub fn write_to_file(&mut self, path: &Path) -> Result<()> {
        let count = self.len() as u32;
        self.header.entry_count = count;

        let key_col_bytes = self.header.key_column.as_bytes();
        let key_col_len = key_col_bytes.len() as u32;
        let key_type_id = self.header.key_type.id();

        let header_size = 4 + 4 + 1 + 4 + key_col_len + 8 + 4;
        let data_offset = header_size as u64;
        self.header.data_offset = data_offset;

        let mut buf = Vec::with_capacity(1024 * 1024);
        buf.write_all(&INDEX_MAGIC.to_le_bytes())?;
        buf.write_all(&INDEX_VERSION.to_le_bytes())?;
        buf.write_all(&key_type_id.to_le_bytes())?;
        buf.write_all(&key_col_len.to_le_bytes())?;
        buf.write_all(key_col_bytes)?;
        buf.write_all(&data_offset.to_le_bytes())?;
        buf.write_all(&count.to_le_bytes())?;

        let mut prev_key = String::new();
        match &self.storage {
            KeyStorage::String(map) => {
                for (key, entries) in map {
                    let (prefix_len, suffix) = shared_prefix_suffix(&prev_key, key);
                    write_var_u64(&mut buf, prefix_len as u64)?;
                    write_var_u64(&mut buf, suffix.len() as u64)?;
                    buf.write_all(suffix.as_bytes())?;
                    write_var_u64(&mut buf, entries.len() as u64)?;

                    let mut prev_offset = 0u64;
                    for (i, entry) in entries.iter().enumerate() {
                        if i == 0 {
                            write_var_u64(&mut buf, entry.offset)?;
                        } else {
                            write_var_u64(&mut buf, entry.offset.saturating_sub(prev_offset))?;
                        }
                        write_var_u64(&mut buf, entry.row_length as u64)?;
                        prev_offset = entry.offset;
                    }
                    prev_key = key.clone();
                }
            }
            KeyStorage::Numeric(vec) => {
                for (_, key, entries) in vec {
                    let (prefix_len, suffix) = shared_prefix_suffix(&prev_key, key);
                    write_var_u64(&mut buf, prefix_len as u64)?;
                    write_var_u64(&mut buf, suffix.len() as u64)?;
                    buf.write_all(suffix.as_bytes())?;
                    write_var_u64(&mut buf, entries.len() as u64)?;

                    let mut prev_offset = 0u64;
                    for (i, entry) in entries.iter().enumerate() {
                        if i == 0 {
                            write_var_u64(&mut buf, entry.offset)?;
                        } else {
                            write_var_u64(&mut buf, entry.offset.saturating_sub(prev_offset))?;
                        }
                        write_var_u64(&mut buf, entry.row_length as u64)?;
                        prev_offset = entry.offset;
                    }
                    prev_key = key.clone();
                }
            }
        }

        let checksum = crc32fast::hash(&buf);
        buf.write_all(&checksum.to_le_bytes())?;

        std::fs::write(path, &buf)?;
        Ok(())
    }

    pub fn read_from_file(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("failed to open index file: {}", path.display()))?;
        let mut reader = BufReader::new(file);

        let magic = read_u32(&mut reader)?;
        if magic != INDEX_MAGIC {
            anyhow::bail!(
                "invalid index file magic: expected 0x{:08X}, got 0x{:08X}",
                INDEX_MAGIC,
                magic
            );
        }

        let version = read_u32(&mut reader)?;
        if version != INDEX_VERSION {
            anyhow::bail!("unsupported index version: {version}");
        }

        let key_type_id = read_u8(&mut reader)?;
        let key_type = KeyType::from_id(key_type_id).context("invalid key type in index file")?;

        let key_col_len = read_u32(&mut reader)? as usize;
        let mut key_col_buf = vec![0u8; key_col_len];
        reader.read_exact(&mut key_col_buf)?;
        let key_column =
            String::from_utf8(key_col_buf.clone()).context("invalid UTF-8 in index key_column")?;

        let data_offset = read_u64(&mut reader)?;
        let entry_count = read_u32(&mut reader)?;

        let storage = if key_type.supports_numeric_vec() {
            KeyStorage::Numeric(Vec::new())
        } else {
            KeyStorage::String(BTreeMap::new())
        };

        let mut storage = storage;
        let mut read_entries = 0u32;
        let mut prev_key = String::new();
        while read_entries < entry_count {
            let prefix_len = read_var_u64(&mut reader)? as usize;
            let suffix_len = read_var_u64(&mut reader)? as usize;
            let mut suffix_buf = vec![0u8; suffix_len];
            reader.read_exact(&mut suffix_buf)?;
            let suffix =
                String::from_utf8(suffix_buf).context("invalid UTF-8 in index key suffix")?;
            let key = rebuild_key(&prev_key, prefix_len, &suffix)?;
            let posting_count = read_var_u64(&mut reader)? as u32;
            let mut postings = Vec::with_capacity(posting_count as usize);
            let mut prev_offset = 0u64;
            for _ in 0..posting_count {
                let raw = read_var_u64(&mut reader)?;
                let offset = if postings.is_empty() {
                    raw
                } else {
                    prev_offset.saturating_add(raw)
                };
                let row_length = read_var_u64(&mut reader)? as u32;
                postings.push(IndexEntry { offset, row_length });
                read_entries += 1;
                prev_offset = offset;
            }
            prev_key = key.clone();

            match &mut storage {
                KeyStorage::String(map) => {
                    map.insert(key, postings);
                }
                KeyStorage::Numeric(vec) => {
                    if let Some(kv) = key_type.parse(&key) {
                        vec.push((kv, key, postings));
                    }
                }
            }
        }

        let file_len = reader.seek(std::io::SeekFrom::End(0))?;
        let header_end = 4 + 4 + 1 + 4 + key_col_len + 8 + 4;
        if file_len < (header_end + 4) as u64 {
            anyhow::bail!(
                "index file too short: expected at least {} bytes, got {}",
                header_end + 4,
                file_len
            );
        }

        let checksum_pos = file_len - 4;
        reader.seek(std::io::SeekFrom::Start(checksum_pos))?;
        let stored_checksum = read_u32(&mut reader)?;

        reader.seek(std::io::SeekFrom::Start(0))?;
        let data_to_hash_len = checksum_pos as usize;
        let mut data_to_hash = vec![0u8; data_to_hash_len];
        reader.read_exact(&mut data_to_hash)?;
        let computed_checksum = crc32fast::hash(&data_to_hash);

        if computed_checksum != stored_checksum {
            anyhow::bail!(
                "index file corrupted: checksum mismatch (expected {:08x}, got {:08x}). Run `grpctestify index --force` to rebuild.",
                stored_checksum,
                computed_checksum
            );
        }

        Ok(Self {
            storage,
            header: IndexHeader {
                version,
                key_column,
                key_type,
                data_offset,
                entry_count,
            },
            filter: None,
        })
    }

    pub fn lookup_row<R: Read + Seek>(&self, reader: &mut R, key: &str) -> Result<Option<String>> {
        let entries = self.lookup_all(key);
        let entry = match entries.and_then(|e| e.first()) {
            Some(e) => e,
            None => return Ok(None),
        };

        reader.seek(SeekFrom::Start(entry.offset))?;
        let mut buf = vec![0u8; entry.row_length as usize];
        reader.read_exact(&mut buf)?;
        let line = String::from_utf8(buf).context("invalid UTF-8 in source row")?;
        Ok(Some(line))
    }

    pub fn lookup_row_from_mmap(&self, mmap_data: &[u8], key: &str) -> Result<Option<String>> {
        let entries = self.lookup_all(key);
        let entry = match entries.and_then(|e| e.first()) {
            Some(e) => e,
            None => return Ok(None),
        };

        let start = entry.offset as usize;
        let end = start + entry.row_length as usize;

        if end > mmap_data.len() {
            anyhow::bail!(
                "index entry out of bounds: offset={} len={} mmap_len={}",
                entry.offset,
                entry.row_length,
                mmap_data.len()
            );
        }

        let line = String::from_utf8(mmap_data[start..end].to_vec())
            .context("invalid UTF-8 in source row")?;
        Ok(Some(line))
    }

    pub fn key_column(&self) -> &str {
        &self.header.key_column
    }
}

pub fn read_index_key_type<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Result<KeyType> {
    use std::io::SeekFrom;
    reader.seek(SeekFrom::Start(0))?;
    let magic = read_u32(reader)?;
    if magic != INDEX_MAGIC {
        anyhow::bail!("not a valid index file");
    }
    let version = read_u32(reader)?;
    if version != INDEX_VERSION {
        anyhow::bail!("unsupported index version: {}", version);
    }
    let key_type_id = read_u8(reader)?;
    KeyType::from_id(key_type_id).context("invalid key type in index file")
}

pub fn is_index_valid(path: &Path) -> bool {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut reader = BufReader::new(file);
    if read_u32(&mut reader).ok() != Some(INDEX_MAGIC) {
        return false;
    }
    if read_u32(&mut reader).ok() != Some(INDEX_VERSION) {
        return false;
    }
    if read_u8(&mut reader).is_err() {
        return false;
    }
    let key_col_len = match read_u32(&mut reader) {
        Ok(l) => l as usize,
        Err(_) => return false,
    };
    let mut key_col_buf = vec![0u8; key_col_len];
    if reader.read_exact(&mut key_col_buf).is_err() {
        return false;
    }
    if read_u64(&mut reader).is_err() {
        return false;
    }
    if read_u32(&mut reader).is_err() {
        return false;
    }

    let file_len = match reader.seek(std::io::SeekFrom::End(0)) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let header_end = 4 + 4 + 1 + 4 + key_col_len + 8 + 4;
    if file_len < (header_end + 4) as u64 {
        return false;
    }

    let checksum_pos = file_len - 4;
    if reader.seek(std::io::SeekFrom::Start(checksum_pos)).is_err() {
        return false;
    }
    let stored_checksum = match read_u32(&mut reader) {
        Ok(c) => c,
        Err(_) => return false,
    };

    if reader.seek(std::io::SeekFrom::Start(0)).is_err() {
        return false;
    }
    let data_to_hash_len = checksum_pos as usize;
    let mut data_to_hash = vec![0u8; data_to_hash_len];
    if reader.read_exact(&mut data_to_hash).is_err() {
        return false;
    }
    let computed_checksum = crc32fast::hash(&data_to_hash);
    computed_checksum == stored_checksum
}

fn read_u32(reader: &mut impl Read) -> Result<u32> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_u64(reader: &mut impl Read) -> Result<u64> {
    let mut buf = [0u8; 8];
    reader.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

fn read_u8(reader: &mut impl Read) -> Result<u8> {
    let mut buf = [0u8; 1];
    reader.read_exact(&mut buf)?;
    Ok(buf[0])
}

fn shared_prefix_suffix<'a>(prev: &str, current: &'a str) -> (usize, &'a str) {
    let max = prev.len().min(current.len());
    let mut i = 0usize;
    let prev_b = prev.as_bytes();
    let cur_b = current.as_bytes();
    while i < max && prev_b[i] == cur_b[i] {
        i += 1;
    }
    while i > 0 && !current.is_char_boundary(i) {
        i -= 1;
    }
    (i, &current[i..])
}

fn rebuild_key(prev: &str, prefix_len: usize, suffix: &str) -> Result<String> {
    if prefix_len > prev.len() || !prev.is_char_boundary(prefix_len) {
        anyhow::bail!("invalid key prefix length in index stream");
    }
    let mut out = String::with_capacity(prefix_len + suffix.len());
    out.push_str(&prev[..prefix_len]);
    out.push_str(suffix);
    Ok(out)
}

fn write_var_u64(writer: &mut impl Write, mut value: u64) -> Result<()> {
    while value >= 0x80 {
        writer.write_all(&[((value as u8) & 0x7F) | 0x80])?;
        value >>= 7;
    }
    writer.write_all(&[value as u8])?;
    Ok(())
}

fn read_var_u64(reader: &mut impl Read) -> Result<u64> {
    let mut shift = 0u32;
    let mut out = 0u64;
    loop {
        if shift > 63 {
            anyhow::bail!("varint too long in index stream");
        }
        let mut b = [0u8; 1];
        reader.read_exact(&mut b)?;
        let byte = b[0];
        out |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn write_and_read_roundtrip() {
        let dir = std::env::temp_dir().join("gctf_index_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.gcti");

        let mut idx = SourceIndex::new("pvz_id");
        idx.insert("pvz_001".into(), 0, 50);
        idx.insert("pvz_002".into(), 51, 60);
        idx.insert("pvz_003".into(), 112, 45);
        idx.write_to_file(&path).unwrap();

        let loaded = SourceIndex::read_from_file(&path).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.key_column(), "pvz_id");

        let e1 = loaded.lookup("pvz_001").unwrap();
        assert_eq!(e1.offset, 0);
        assert_eq!(e1.row_length, 50);

        let e2 = loaded.lookup("pvz_002").unwrap();
        assert_eq!(e2.offset, 51);

        assert!(loaded.lookup("missing").is_none());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn invalid_magic_fails() {
        let dir = std::env::temp_dir().join("gctf_index_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_magic.gcti");

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&0xDEADBEEFu32.to_le_bytes()).unwrap();

        let result = SourceIndex::read_from_file(&path);
        assert!(result.is_err());

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn entries_sorted_by_key() {
        let mut idx = SourceIndex::new("id");
        idx.insert("c".into(), 200, 10);
        idx.insert("a".into(), 0, 10);
        idx.insert("b".into(), 100, 10);

        let keys: Vec<&str> = idx.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, vec!["a", "b", "c"]);
    }

    #[test]
    fn lookup_row_from_source() {
        let source_data = "id,name,age\n1,Alice,30\n2,Bob,25\n";

        let mut idx = SourceIndex::new("id");
        let header_line = "id,name,age\n";
        let row1_offset = header_line.len() as u64;
        let row1 = "1,Alice,30";
        idx.insert("1".into(), row1_offset, row1.len() as u32);

        let row2_offset = (header_line.len() + row1.len() + 1) as u64;
        let row2 = "2,Bob,25";
        idx.insert("2".into(), row2_offset, row2.len() as u32);

        let mut cursor = Cursor::new(source_data);
        let line1 = idx.lookup_row(&mut cursor, "1").unwrap().unwrap();
        assert_eq!(line1, "1,Alice,30");

        let line2 = idx.lookup_row(&mut cursor, "2").unwrap().unwrap();
        assert_eq!(line2, "2,Bob,25");

        assert!(idx.lookup_row(&mut cursor, "99").unwrap().is_none());
    }

    #[test]
    fn empty_index_roundtrip() {
        let dir = std::env::temp_dir().join("gctf_index_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.gcti");

        let mut idx = SourceIndex::new("id");
        idx.write_to_file(&path).unwrap();

        let loaded = SourceIndex::read_from_file(&path).unwrap();
        assert!(loaded.is_empty());
        assert_eq!(loaded.len(), 0);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn unicode_keys() {
        let dir = std::env::temp_dir().join("gctf_index_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("unicode.gcti");

        let mut idx = SourceIndex::new("город");
        idx.insert("Москва".into(), 0, 10);
        idx.insert("Санкт-Петербург".into(), 10, 20);
        idx.write_to_file(&path).unwrap();

        let loaded = SourceIndex::read_from_file(&path).unwrap();
        assert_eq!(loaded.key_column(), "город");
        assert!(loaded.contains("Москва"));
        assert!(loaded.contains("Санкт-Петербург"));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn duplicate_keys_are_preserved() {
        let dir = std::env::temp_dir().join("gctf_index_dup_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dup.gcti");

        let mut idx = SourceIndex::new("zone_id");
        idx.insert("z1".into(), 10, 20);
        idx.insert("z1".into(), 31, 22);
        idx.insert("z2".into(), 54, 18);
        idx.write_to_file(&path).unwrap();

        let loaded = SourceIndex::read_from_file(&path).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.unique_keys_len(), 2);

        let all = loaded.lookup_all("z1").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].offset, 10);
        assert_eq!(all[1].offset, 31);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn index_entry_v4_encode_decode() {
        let entry = IndexEntryV4::new(0x123456789ABC, 100);
        let encoded = entry.encode();
        let decoded = IndexEntryV4::decode(encoded);
        assert_eq!(decoded.offset, 0x123456789ABC);
        assert!(!decoded.has_unicode_suffix);
        assert!(!decoded.has_extended_metadata);
    }

    #[test]
    fn index_entry_v4_with_unicode_flag() {
        let entry = IndexEntryV4::new(0x1000, 50).with_unicode();
        let encoded = entry.encode();
        assert!(encoded & FLAG_HAS_UNICODE != 0);

        let decoded = IndexEntryV4::decode(encoded);
        assert!(decoded.has_unicode_suffix);
    }

    #[test]
    fn index_entry_v4_with_metadata_flag() {
        let entry = IndexEntryV4::new(0x1000, 50).with_metadata();
        let encoded = entry.encode();
        assert!(encoded & FLAG_HAS_METADATA != 0);

        let decoded = IndexEntryV4::decode(encoded);
        assert!(decoded.has_extended_metadata);
    }

    #[test]
    fn index_entry_v4_both_flags() {
        let entry = IndexEntryV4::new(0x1000, 50).with_unicode().with_metadata();
        let encoded = entry.encode();
        assert!(encoded & FLAG_HAS_UNICODE != 0);
        assert!(encoded & FLAG_HAS_METADATA != 0);

        let decoded = IndexEntryV4::decode(encoded);
        assert!(decoded.has_unicode_suffix);
        assert!(decoded.has_extended_metadata);
    }

    #[test]
    fn index_entry_v4_max_offset() {
        let max_offset: u64 = 0x3FFFFFFFFFFFFFFF;
        let entry = IndexEntryV4::new(max_offset, 1000);
        let encoded = entry.encode();
        let decoded = IndexEntryV4::decode(encoded);
        assert_eq!(decoded.offset, max_offset);
    }

    #[test]
    fn lookup_range_string_keys() {
        let mut idx = SourceIndex::new("zone_id");
        idx.insert("zone_a".into(), 0, 10);
        idx.insert("zone_b".into(), 20, 15);
        idx.insert("zone_c".into(), 50, 20);
        idx.insert("zone_d".into(), 100, 25);

        let results = idx.lookup_range("zone_a", "zone_c");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0].offset, 0);
        assert_eq!(results[1].offset, 20);
        assert_eq!(results[2].offset, 50);
    }

    #[test]
    fn lookup_range_numeric_keys() {
        let mut idx = SourceIndex::with_key_type("date_id", KeyType::DatePacked);
        idx.insert("2024-01-01".into(), 0, 10);
        idx.insert("2024-01-15".into(), 20, 15);
        idx.insert("2024-01-31".into(), 50, 20);
        idx.insert("2024-02-01".into(), 100, 25);

        let results = idx.lookup_range("2024-01-01", "2024-01-31");
        assert_eq!(results.len(), 3);

        let results2 = idx.lookup_range("2024-01-10", "2024-01-20");
        assert_eq!(results2.len(), 1);
        assert_eq!(results2[0].offset, 20);
    }

    #[test]
    fn lookup_range_single_key() {
        let mut idx = SourceIndex::new("id");
        idx.insert("a".into(), 0, 10);
        idx.insert("b".into(), 20, 15);
        idx.insert("c".into(), 50, 20);

        let results = idx.lookup_range("b", "b");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].offset, 20);
    }

    #[test]
    fn lookup_range_no_match() {
        let mut idx = SourceIndex::new("id");
        idx.insert("a".into(), 0, 10);
        idx.insert("c".into(), 50, 20);

        let results = idx.lookup_range("b", "b");
        assert!(results.is_empty());
    }
}
