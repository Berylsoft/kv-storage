use std::path::Path;
use rusqlite::{Connection, params};
use crate::{Metadata, error::{Result, ErrorContext}, init};

pub struct Reader {
    conn: Connection,
}

impl Reader {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, Metadata)> {
        init::register_cksumvfs_once().context("register cksumvfs")?;
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).context("open file")?;
        init::set_reserve_bytes(&conn).context("set reserve bytes")?;

        // generally(TM) not necessary
        init::set_synchronous(&conn)?;
        init::enable_foreign_keys(&conn)?;

        init::ensure_checksum_enabled(&conn)?;
        init::check_version(&conn)?;
        init::init_schema(&conn)?;
        let metadata = init::read_metadata(&conn)?;
        Ok((Reader { conn }, metadata))
    }

    pub fn get_domain_name_by_id(&mut self, domain_id: u32) -> Result<Box<[u8]>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT domain FROM domains WHERE domain_id = ?",
        ).context("get_domain_name_by_id: prepare")?;

        let domain_name = stmt.query_one(
            params![domain_id],
            |r| r.get(0),
        ).context("get_domain_name_by_id: get")?;
    
        Ok(domain_name)
    }

    pub fn get_domain_id_by_name(&mut self, domain: &[u8]) -> Result<u32> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT domain_id FROM domains WHERE domain = ?",
        ).context("get_domain_id_by_name: prepare")?;

        let domain_id = stmt.query_one(
            params![domain],
            |r| r.get(0),
        ).context("get_domain_id_by_name: get")?;
    
        Ok(domain_id)
    }

    pub fn get_value_by_key(&mut self, domain_id: u32, key: &[u8]) -> Result<Box<[u8]>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT value FROM storage WHERE domain_id = ? AND key = ?",
        ).context("get_value_by_key: prepare")?;

        let value = stmt.query_one(
            params![domain_id, key],
            |r| r.get(0),
        ).context("get_value_by_key: get")?;
    
        Ok(value)
    }

    pub fn get_key_by_value(&mut self, domain_id: u32, value: &[u8]) -> Result<Box<[u8]>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key FROM storage WHERE domain_id = ? AND value = ?",
        ).context("get_key_by_value: prepare")?;

        let key = stmt.query_one(
            params![domain_id, value],
            |r| r.get(0),
        ).context("get_key_by_value: get")?;
    
        Ok(key)
    }

    pub fn iter_range<F>(
        &mut self,
        domain_id: u32,
        key_min: &[u8],
        key_max: &[u8],
        mut f: F,
    ) -> Result<()>
    where
        F: FnMut(Box<[u8]>, Box<[u8]>),
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, value
            FROM storage
            WHERE domain_id = ?1
            AND key >= ?2
            AND key <= ?3
            ORDER BY key ASC",
        ).context("iter_range: prepare")?;

        let mut rows = stmt.query(params![domain_id, key_min, key_max])
            .context("iter_range: query")?;

        while let Some(row) = rows.next().context("iter_range: next row")? {
            let key = row.get(0).context("iter_range: next key")?;
            let value = row.get(1).context("iter_range: next value")?;
            // return result?
            f(key, value);
        }

        Ok(())
    }

    pub fn iter<F>(&mut self, domain_id: u32, mut f: F) -> Result<()>
    where
        F: FnMut(Box<[u8]>, Box<[u8]>),
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT key, value
            FROM storage
            WHERE domain_id = ?1
            ORDER BY key ASC",
        ).context("iter: prepare")?;

        let mut rows = stmt.query(params![domain_id])
            .context("iter: query")?;

        while let Some(row) = rows.next().context("iter: next row")? {
            let key = row.get(0).context("iter: next key")?;
            let value = row.get(1).context("iter: next value")?;
            // return result?
            f(key, value);
        }

        Ok(())
    }
}
