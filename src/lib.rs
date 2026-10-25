use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Walror {
    #[error("io failure: {0}")]
    Io(#[from] std::io::Error),
    #[error("corrupted record: crc mismatch")]
    CrcMismatch,
    #[error("mismatched record type")]
    InvalidRecord,
    #[error("mismatched record size")]
    RecordTooLarge,
}
const MAX_KEY_SIZE: usize = 64 * 1024;
const MAX_VAL_SIZE: usize = 512 * 1024 * 1024;
pub struct Walrus {
    data: HashMap<Vec<u8>, Vec<u8>>,
    db: File,
    db_path: PathBuf,
}

impl Walrus {
    pub fn setup<P: AsRef<Path>>(p: P) -> Result<Self, Walror> {
        let f = OpenOptions::new()
            .write(true)
            .read(true)
            .create(true)
            .truncate(false)
            .open(&p)?;

        let mut r = BufReader::new(f);
        let mut map = HashMap::new();

        loop {
            if r.fill_buf()?.is_empty() {
                break;
            }

            let mut crc_buf = [0u8; 4];
            r.read_exact(&mut crc_buf)?;

            let expected_crc = u32::from_be_bytes(crc_buf);

            let mut op = [0u8; 1];
            r.read_exact(&mut op)?;

            match op[0] {
                0x00 => {
                    let mut key_len_buf = [0u8; 4];
                    r.read_exact(&mut key_len_buf)?;
                    let key_len = u32::from_be_bytes(key_len_buf) as usize;
                    if key_len > MAX_KEY_SIZE {
                        return Err(Walror::RecordTooLarge);
                    }
                    let mut key_buf = vec![0u8; key_len];
                    r.read_exact(&mut key_buf)?;

                    let mut hasher = crc32fast::Hasher::new();
                    hasher.update(&[0x00]);
                    hasher.update(&key_len_buf);
                    hasher.update(&key_buf);

                    if hasher.finalize() != expected_crc {
                        return Err(Walror::CrcMismatch);
                    }
                    map.remove(&key_buf);
                }
                0x01 => {
                    let mut key_len_buf = [0u8; 4];
                    r.read_exact(&mut key_len_buf)?;
                    let key_len = u32::from_be_bytes(key_len_buf) as usize;
                    if key_len > MAX_KEY_SIZE {
                        return Err(Walror::RecordTooLarge);
                    }
                    let mut key_buf = vec![0u8; key_len];
                    r.read_exact(&mut key_buf)?;

                    let mut val_len_buf = [0u8; 8];
                    r.read_exact(&mut val_len_buf)?;
                    let val_len_raw = u64::from_be_bytes(val_len_buf);
                    let val_len: usize =
                        val_len_raw.try_into().map_err(|_| Walror::RecordTooLarge)?;
                    if val_len > MAX_VAL_SIZE {
                        return Err(Walror::RecordTooLarge);
                    }
                    let mut val_buf = vec![0u8; val_len];
                    r.read_exact(&mut val_buf)?;

                    let mut hasher = crc32fast::Hasher::new();
                    hasher.update(&[0x01]);
                    hasher.update(&key_len_buf);
                    hasher.update(&key_buf);
                    hasher.update(&val_len_buf);
                    hasher.update(&val_buf);

                    if hasher.finalize() != expected_crc {
                        return Err(Walror::CrcMismatch);
                    }

                    map.insert(key_buf, val_buf);
                }
                _ => return Err(Walror::InvalidRecord),
            }
        }

        let mut f = r.into_inner();
        f.seek(SeekFrom::End(0))?;

        Ok(Self {
            data: map,
            db: f,
            db_path: p.as_ref().to_path_buf(),
        })
    }

    pub fn get<K: AsRef<[u8]>>(&self, key: K) -> Option<&[u8]> {
        self.data.get(key.as_ref()).map(|v| v.as_slice())
    }

    pub fn put<K: AsRef<[u8]>, V: AsRef<[u8]>>(&mut self, k: K, v: V) -> Result<(), Walror> {
        let key = k.as_ref();
        let val = v.as_ref();
        let buf = encode_put(key, val)?;

        self.db.write_all(&buf)?;
        self.data.insert(key.to_vec(), val.to_vec());
        Ok(())
    }

    pub fn del<K: AsRef<[u8]>>(&mut self, k: K) -> Result<(), Walror> {
        let key = k.as_ref();
        if !self.data.contains_key(key) {
            return Ok(());
        }

        let buf = encode_del(key)?;
        self.db.write_all(&buf)?;
        self.data.remove(key);
        Ok(())
    }
    pub fn compact(&mut self) -> Result<(), Walror> {
        let tmp_path = self.db_path.with_extension("tmp");

        let tmp = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp_path)?;

        let mut writer = BufWriter::new(tmp);

        for (key, val) in &self.data {
            let buf = encode_put(key, val)?;
            writer.write_all(&buf)?;
        }
        writer.flush()?;
        let tmp_file = writer
            .into_inner()
            .map_err(|e| Walror::Io(e.into_error()))?;
        tmp_file.sync_all()?;
        drop(tmp_file);

        fs::rename(&tmp_path, &self.db_path)?;
        let mut new_file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(&self.db_path)?;
        new_file.seek(SeekFrom::End(0))?;
        self.db = new_file;

        Ok(())
    }
}
fn encode_put(key: &[u8], val: &[u8]) -> Result<Vec<u8>, Walror> {
    let key_len = u32::to_be_bytes(key.len().try_into().map_err(|_| Walror::RecordTooLarge)?);
    let val_len = u64::to_be_bytes(val.len().try_into().map_err(|_| Walror::RecordTooLarge)?);

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[0x01]);
    hasher.update(&key_len);
    hasher.update(key);
    hasher.update(&val_len);
    hasher.update(val);
    let checksum = hasher.finalize();

    let mut buf = Vec::with_capacity(4 + 1 + 4 + key.len() + 8 + val.len());
    buf.extend_from_slice(&checksum.to_be_bytes());
    buf.extend_from_slice(&[0x01]);
    buf.extend_from_slice(&key_len);
    buf.extend_from_slice(key);
    buf.extend_from_slice(&val_len);
    buf.extend_from_slice(val);
    Ok(buf)
}
fn encode_del(key: &[u8]) -> Result<Vec<u8>, Walror> {
    let key_len = u32::to_be_bytes(key.len().try_into().map_err(|_| Walror::RecordTooLarge)?);

    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&[0x00]);
    hasher.update(&key_len);
    hasher.update(key);
    let checksum = hasher.finalize();

    let mut buf = Vec::with_capacity(4 + 1 + 4 + key.len());
    buf.extend_from_slice(&checksum.to_be_bytes());
    buf.extend_from_slice(&[0x00]);
    buf.extend_from_slice(&key_len);
    buf.extend_from_slice(key);
    Ok(buf)
}
#[cfg(test)]
#[test]
fn test_all() -> Result<(), Walror> {
    let test_path = "test.walrus";
    let _ = fs::remove_file(test_path);

    let mut db = Walrus::setup(test_path)?;
    db.put("user", "shawn")?;
    assert_eq!(db.get("user"), Some(b"shawn".as_slice()));

    drop(db);
    let mut db = Walrus::setup(test_path)?;
    assert_eq!(db.get("user"), Some(b"shawn".as_slice()));

    for i in 0..50 {
        db.put("counter", i.to_string())?;
    }
    db.compact()?;
    drop(db);

    let db = Walrus::setup(test_path)?;
    assert_eq!(db.get("counter"), Some(b"49".as_slice()));

    let _ = fs::remove_file(test_path);
    Ok(())
}
