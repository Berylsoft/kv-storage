use std::path::Path;
use rusqlite::{Connection, params};
use crate::{Metadata, error::{Error, Result, ErrorContext}, init};

pub struct Writer {
    pub(crate) conn: Connection,
}

impl Writer {
    pub fn open(path: impl AsRef<Path>, metadata: Metadata) -> Result<Self> {
        init::register_cksumvfs_once().context("register cksumvfs")?;
        let conn = Connection::open(path).context("open file")?;
        init::set_reserve_bytes(&conn).context("set reserve bytes")?;
        init::run_vacuum(&conn)?;
        init::set_synchronous(&conn)?;
        init::enable_foreign_keys(&conn)?;
        init::ensure_checksum_enabled(&conn)?;
        let new = init::check_or_write_version_return_new(&conn)?;
        init::init_schema(&conn)?;
        init::check_or_write_metadata(&conn, metadata, new)?;
        Ok(Self { conn })
    }

    pub fn get_domain_name_by_id(&mut self, domain_id: u32) -> Result<Option<Box<[u8]>>> {
        let tr = self.conn.transaction()
            .context("get_domain_name_by_id in write: begin transaction")?;

        let mut stmt = tr.prepare_cached(
            "SELECT domain FROM domains WHERE domain_id = ?",
        ).context("get_domain_name_by_id in write: prepare")?;

        let mut rows = stmt.query(
            params![domain_id],
        ).context("get_domain_name_by_id in write: query")?;

        let domain_name = match rows.next().context("get_domain_name_by_id in write: first row")? {
            None => None,
            Some(row) => Some(row.get(0).context("get_domain_name_by_id in write: get domain name")?)
        };

        if rows.next().context("get_domain_name_by_id in write: second row")?.is_some() {
            return Err(Error::Invariant("more than 1 rows in get_domain_name_by_id result"));
        }

        drop(rows);

        drop(stmt);

        tr.commit().context("get_domain_name_by_id in write: commit")?;
        Ok(domain_name)
    }

    pub fn get_domain_id_by_name(&mut self, domain: &[u8]) -> Result<Option<u32>> {
        let tr = self.conn.transaction()
            .context("get_domain_id_by_name in write: begin transaction")?;

        let mut stmt = tr.prepare_cached(
            "SELECT domain_id FROM domains WHERE domain = ?",
        ).context("get_domain_id_by_name in write: prepare")?;

        let mut rows = stmt.query(
            params![domain],
        ).context("get_domain_id_by_name in write: query")?;

        let domain_id = match rows.next().context("get_domain_id_by_name in write: first row")? {
            None => None,
            Some(row) => Some(row.get(0).context("get_domain_id_by_name in write: get domain name")?)
        };

        if rows.next().context("get_domain_id_by_name in write: second row")?.is_some() {
            return Err(Error::Invariant("more than 1 rows in get_domain_id_by_name result"));
        }

        drop(rows);

        drop(stmt);

        tr.commit().context("get_domain_id_by_name in write: commit")?;
        Ok(domain_id)
    }

    pub fn get_max_domain_id(&mut self) -> Result<u32> {
        let tr = self.conn.transaction()
            .context("get_max_domain_id: begin transaction")?;

        let mut stmt = tr.prepare_cached(
            "SELECT MAX(domain_id) FROM domains",
        ).context("get_max_domain_id: prepare")?;

        let domain_id = stmt.query_one(
            [],
            |r| r.get(0),
        ).context("get_max_domain_id: get")?;

        drop(stmt);

        tr.commit().context("get_max_domain_id: commit")?;
        Ok(domain_id)
    }

    pub fn write_domain(&mut self, domain_id: u32, domain: &[u8]) -> Result<()> {
        let tr = self.conn.transaction()
            .context("write domain: begin transaction")?;

        let mut stmt = tr.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
        ).context("write domain: check if domain id exists: prepare")?;
        let domain_id_exists: bool = stmt.query_one(
            params![domain_id], |r| r.get(0),
        ).context("write domain: check if domain id exists")?;
        if domain_id_exists {
            return Err(Error::DuplicateDomainId);
        }
        drop(stmt);

        let mut stmt = tr.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain = ?)",
        ).context("write domain: check if domain name exists: prepare")?;
        let domain_name_exists: bool = stmt.query_one(
            params![domain], |r| r.get(0),
        ).context("write domain: check if domain name exists")?;
        if domain_name_exists {
            return Err(Error::DuplicateDomainName);
        }
        drop(stmt);

        let mut stmt = tr.prepare_cached(
            "INSERT INTO domains (domain_id, domain) VALUES (?, ?)",
        ).context("write domain: write: prepare")?;
        let updated_rows = stmt.execute(
            params![domain_id, domain],
        ).context("write domain: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write domain updated_rows not 1"));
        }
        drop(stmt);

        tr.commit().context("write domain: commit")?;
        Ok(())
    }

    pub fn write_kv(&mut self, domain_id: u32, key: &[u8], value: &[u8]) -> Result<()> {
        let tr = self.conn.transaction()
            .context("write kv: begin transaction")?;

        let mut stmt = tr.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
        ).context("write kv: check if domain exists: prepare")?;
        let domain_exists: bool = stmt.query_one(
            params![domain_id], |r| r.get(0),
        ).context("write kv: check if domain exists")?;
        if !domain_exists {
            return Err(Error::UnknownDomainId);
        }
        drop(stmt);

        let mut stmt = tr.prepare_cached(
            "SELECT EXISTS (SELECT 1 FROM storage WHERE (domain_id, key) = (?, ?))",
        ).context("write kv: check if key exists: prepare")?;
        let key_exists: bool = stmt.query_one(
            params![domain_id, key], |r| r.get(0),
        ).context("write kv: check if key exists")?;
        if key_exists {
            return Err(Error::DuplicateKey);
        }
        drop(stmt);

        let mut stmt = tr.prepare_cached(
            "INSERT INTO storage (domain_id, key, value) VALUES (?, ?, ?)",
        ).context("write kv: write: prepare")?;
        let updated_rows = stmt.execute(
            params![domain_id, key, value],
        ).context("write kv: write")?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write kv updated_rows not 1"));
        }
        drop(stmt);

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
