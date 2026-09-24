use std::path::Path;
use rusqlite::{Connection, params};
use crate::{
    Metadata,
    error::{Result, ErrorContext},
    init::*,
};

pub struct Reader {
    conn: Connection,
}

impl Reader {
    pub fn open(path: impl AsRef<Path>) -> Result<(Self, Metadata)> {
        register_cksumvfs_once().context("register cksumvfs")?;
        let conn = Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        ).context("open file")?;
        set_reserve_bytes(&conn).context("set reserve bytes")?;

        // generally(TM) not necessary
        set_synchronous(&conn)?;
        enable_foreign_keys(&conn)?;

        ensure_checksum_enabled(&conn)?;
        check_version(&conn)?;
        init_schema(&conn)?;
        let metadata = read_metadata(&conn)?;
        Ok((Reader { conn }, metadata))
    }

    pub fn get_domain_name_by_id(&mut self, domain_id: u32) -> Result<Box<[u8]>> {
        let mut stmt = self.conn.prepare(
            "SELECT domain FROM domains WHERE domain_id = ?",
        ).context("get_domain_name_by_id: prepare")?;

        let domain_name = stmt.query_one(
            params![domain_id],
            |r| r.get(0),
        ).context("get_domain_name_by_id: get")?;
    
        Ok(domain_name)
    }

    pub fn get_domain_id_by_name(&mut self, domain: &[u8]) -> Result<u32> {
        let mut stmt = self.conn.prepare(
            "SELECT domain_id FROM domains WHERE domain = ?",
        ).context("get_domain_id_by_name: prepare")?;

        let domain_id = stmt.query_one(
            params![domain],
            |r| r.get(0),
        ).context("get_domain_id_by_name: get")?;
    
        Ok(domain_id)
    }
}
