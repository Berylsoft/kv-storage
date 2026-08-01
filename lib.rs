use std::path::Path;
pub use rusqlite;
use rusqlite::{Connection, ffi, params, types::FromSql};
use crc32fast::hash as crc32;

pub const MAGIC: i32 = 0x42654b56; // BeKV
const SET_MAGIC_STMT: &str = "PRAGMA application_id=0x42654b56;";
pub const VERSION: u32 = 1;
const SET_VERSION_STMT: &str = "PRAGMA user_version=1;";

const METADATA_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS metadata (
    id INTEGER NOT NULL PRIMARY KEY,
    ident BLOB NOT NULL,
    check (id = 0)
) WITHOUT ROWID;";

const DOMAINS_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS domains (
    domain_id INTEGER NOT NULL PRIMARY KEY,
    domain BLOB NOT NULL
) WITHOUT ROWID;";

const STORAGE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS storage (
    domain_id INTEGER NOT NULL,
    key BLOB NOT NULL,
    value_crc32 INTEGER NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY (domain_id, key)
    FOREIGN KEY (domain_id) REFERENCES domains(domain_id)
        ON DELETE RESTRICT
        ON UPDATE RESTRICT
) WITHOUT ROWID;";

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error),
    Invariant(&'static str),
    DuplicateKey,
    DuplicateDomain,
    UnknownDomain,
    NotABeKVDatabase,
    Version { exp: u32, cur: u32 },
    Ident { exp: Box<[u8]>, cur: Box<[u8]> },
    #[cfg(feature = "actor")]
    ActorClosed,
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::Rusqlite(err)
    }
}

pub type Result<T> = core::result::Result<T, Error>;

fn make_rusqlite_result(result_code: core::ffi::c_int, msg: &str) -> rusqlite::Result<()> {
    match result_code {
        ffi::SQLITE_OK => Ok(()),
        result_code => Err(rusqlite::Error::SqliteFailure(
            ffi::Error::new(result_code),
            Some(msg.to_owned()),
        )),
    }
}

fn register_cksumvfs() -> rusqlite::Result<()> {
    let result_code = unsafe {
        ffi::sqlite3_register_cksumvfs(std::ptr::null())
    };
    make_rusqlite_result(
        result_code,
        "error calling sqlite3_register_cksumvfs",
    )
}

fn set_reserve_bytes(conn: &Connection) -> rusqlite::Result<()> {
    let mut reserve_bytes = 8;
    let result_code = unsafe {
        ffi::sqlite3_file_control(
            conn.handle(),
            std::ptr::null(),
            ffi::SQLITE_FCNTL_RESERVE_BYTES,
            (&mut reserve_bytes) as *mut i32 as *mut core::ffi::c_void,
        )
    };
    make_rusqlite_result(
        result_code,
        "error calling sqlite3_file_control to set reserve bytes as 8",
    )
}

fn run_and_check_update_rows(conn: &Connection, stmt: &str, msg: &'static str) -> Result<()> {
    let updated_rows = conn.execute(stmt, [])?;
    if updated_rows == 0 {
        Ok(())
    } else {
        Err(Error::Invariant(msg))
    }
}

fn query_one_row<T: FromSql>(conn: &Connection, stmt: &str) -> rusqlite::Result<T> {
    conn.query_one(stmt, [], |r| r.get::<_, T>(0))
}

fn run_vacuum(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "vacuum;",
        "run vacuum updated_rows not 0",
    )
}

fn ensure_checksum_enabled(conn: &Connection) -> Result<()> {
    let enabled: String = query_one_row(conn, "PRAGMA checksum_verification;")?;
    if enabled != "1" {
        return Err(Error::Invariant("checksum_verification not enabled"));
    }
    Ok(())
}

fn set_synchronous(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "PRAGMA synchronous = EXTRA;",
        "set synchronous updated_rows not 0",
    )?;
    run_and_check_update_rows(
        conn,
        "PRAGMA fullfsync = true;",
        "set fullfsync updated_rows not 0",
    )
}

fn enable_foreign_keys(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        "PRAGMA foreign_keys = ON;",
        "enable foreign_keys updated_rows not 0"
    )
}

fn check_if_database_is_new(conn: &Connection) -> Result<bool> {
    let count: u32 = query_one_row(
        conn,
        "SELECT count(*) FROM sqlite_master WHERE type='table';"
    )?;
    Ok(count == 0)
}

fn check_or_write_version(conn: &Connection) -> Result<()> {
    let magic: i32 = query_one_row(conn, "PRAGMA application_id;")?;
    let version: u32 = query_one_row(conn, "PRAGMA user_version;")?;
    let database_is_new = check_if_database_is_new(conn)?;
    match (magic, version, database_is_new) {
        (0, 0, true) => {
            run_and_check_update_rows(
                conn,
                SET_MAGIC_STMT,
                "set magic updated_rows not 0",
            )?;
            run_and_check_update_rows(
                conn,
                SET_VERSION_STMT,
                "set version updated_rows not 0",
            )
        }
        (MAGIC, VERSION, false) => {
            Ok(())
        }
        (MAGIC, cur_version, false) => {
            Err(Error::Version { exp: VERSION, cur: cur_version })
        }
        _ => {
            Err(Error::NotABeKVDatabase)
        }
    }
}

fn init_schema(conn: &Connection) -> Result<()> {
    run_and_check_update_rows(
        conn,
        METADATA_SCHEMA,
        "init metadata schema updated_rows not 0",
    )?;
    run_and_check_update_rows(
        conn,
        DOMAINS_SCHEMA,
        "init domains schema updated_rows not 0",
    )?;
    run_and_check_update_rows(
        conn,
        STORAGE_SCHEMA,
        "init storage schema updated_rows not 0",
    )
}

struct Metadata {
    ident: Box<[u8]>,
}

fn check_or_write_metadata(conn: &Connection, ident: &[u8]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT * FROM metadata")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            ident: row.get(1)?,
        })
    })?;
    match metadata_iter.next() {
        None => {
            let updated_rows = conn.execute(
                "INSERT INTO metadata (id, ident) VALUES (?, ?)",
                params![0, ident],
            )?;
            if updated_rows != 1 {
                return Err(Error::Invariant("write_metadata updated_rows not 1"));
            }
        }
        Some(cur) => {
            let cur = cur?;
            if ident != cur.ident.as_ref() {
                return Err(Error::Ident {
                    exp: ident.into(),
                    cur: cur.ident,
                });
            }
        }
    }
    if metadata_iter.next().is_some() {
        return Err(Error::Invariant("more than 1 rows in metadata table"));
    }
    Ok(())
}

pub struct Writer {
    conn: Connection,
}

impl Writer {
    pub fn open(path: impl AsRef<Path>, ident: &[u8]) -> Result<Self> {
        register_cksumvfs()?;
        let conn = Connection::open(path)?;
        set_reserve_bytes(&conn)?;
        run_vacuum(&conn)?;
        set_synchronous(&conn)?;
        enable_foreign_keys(&conn)?;
        ensure_checksum_enabled(&conn)?;
        check_or_write_version(&conn)?;
        init_schema(&conn)?;
        check_or_write_metadata(&conn, ident)?;
        Ok(Self { conn })
    }

    pub fn write_domain(&mut self, domain_id: u32, domain: &[u8]) -> Result<()> {
        let tr = self.conn.transaction()?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        )?;
        if domain_exists {
            return Err(Error::DuplicateDomain);
        }

        let updated_rows = tr.execute(
            "INSERT INTO domains (domain_id, domain) VALUES (?, ?)",
            params![domain_id, domain],
        )?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write_domain updated_rows not 1"));
        }
        
        tr.commit()?;
        Ok(())
    }

    pub fn write_kv(&mut self, domain_id: u32, key: &[u8], value: &[u8]) -> Result<()> {
        let value_crc32 = crc32(value);
        let tr = self.conn.transaction()?;

        let domain_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM domains WHERE domain_id = ?)",
            params![domain_id], |r| r.get(0),
        )?;
        if !domain_exists {
            return Err(Error::UnknownDomain);
        }

        let key_exists: bool = tr.query_one(
            "SELECT EXISTS (SELECT 1 FROM storage WHERE (domain_id, key) = (?, ?))",
            params![domain_id, key], |r| r.get(0),
        )?;
        if key_exists {
            return Err(Error::DuplicateKey);
        }

        let updated_rows = tr.execute(
            "INSERT INTO storage (domain_id, key, value_crc32, value) VALUES (?, ?, ?, ?)",
            params![domain_id, key, value_crc32, value],
        )?;
        if updated_rows != 1 {
            return Err(Error::Invariant("write_kv updated_rows not 1"));
        }
        
        tr.commit()?;
        Ok(())
    }

    pub fn close(self) -> Result<()> {
        match self.conn.close() {
            Ok(()) => Ok(()),
            Err((_conn, err)) => Err(err.into()),
        }
    }
}

#[cfg(feature = "actor")]
pub mod actor;
