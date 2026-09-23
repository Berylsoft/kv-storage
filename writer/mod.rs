use std::path::Path;
use rusqlite::{Connection, params};
use crate::{
    Metadata,
    error::{Error, Result, ErrorContext},
    init::*,
};

pub struct Writer {
    conn: Connection,
}

impl Writer {
    pub fn open(path: impl AsRef<Path>, metadata: Metadata) -> Result<Self> {
        register_cksumvfs_once().context("register cksumvfs")?;
        let conn = Connection::open(path).context("open file")?;
        set_reserve_bytes(&conn).context("set reserve bytes")?;
        run_vacuum(&conn)?;
        set_synchronous(&conn)?;
        enable_foreign_keys(&conn)?;
        ensure_checksum_enabled(&conn)?;
        check_or_write_version(&conn)?;
        init_schema(&conn)?;
        check_or_write_metadata(&conn, metadata)?;
        Ok(Self { conn })
    }

    pub fn write_domain(&mut self, domain_id: u32, domain: &[u8]) -> Result<()> {
        let tr = self.conn.transaction().context("write domain: begin transaction")?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        ).context("write domain: check if domain exists")?;
        if domain_exists {
            return Err(Error::DuplicateDomain);
        }

        let updated_rows = tr.execute(
            "INSERT INTO domains (domain_id, domain) VALUES (?, ?)",
            params![domain_id, domain],
        ).context("write domain: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write domain updated_rows not 1"));
        }

        tr.commit().context("write domain: commit")?;
        Ok(())
    }

    pub fn write_kv(&mut self, domain_id: u32, key: &[u8], value: &[u8]) -> Result<()> {
        let tr = self.conn.transaction().context("write kv: begin transaction")?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        ).context("write kv: check if domain exists")?;
        if !domain_exists {
            return Err(Error::UnknownDomain);
        }

        let key_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM storage WHERE (domain_id, key) = (?, ?))",
            params![domain_id, key], |r| r.get(0),
        ).context("write kv: check if key exists")?;
        if key_exists {
            return Err(Error::DuplicateKey);
        }

        let updated_rows = tr.execute(
            "INSERT INTO storage (domain_id, key, value) VALUES (?, ?, ?)",
            params![domain_id, key, value],
        ).context("write kv: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write kv updated_rows not 1"));
        }

        tr.commit().context("write kv: commit")?;
        Ok(())
    }

    pub fn close(self) -> Result<()> {
        match self.conn.close() {
            Ok(()) => Ok(()),
            Err((_conn, err)) => Err(Error::Rusqlite(err, "close file")),
        }
    }
}

#[cfg(feature = "actor")]
pub mod actor;

#[cfg(feature = "actor-domain-handle")]
pub mod actor_domain_handle;
