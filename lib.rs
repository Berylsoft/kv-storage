use std::path::Path;
pub use rusqlite;
use rusqlite::{Connection, ffi, params};
use crc32fast::hash as crc32;

pub const MAGIC: i32 = 0x42654b56; // BeKV
const SET_MAGIC_STMT: &str = "PRAGMA application_id=0x42654b56;";
pub const VERSION: u32 = 1;
const SET_VERSION_STMT: &str = "PRAGMA user_version=1;";

const METADATA_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS metadata (
    id INTEGER PRIMARY KEY,
    ident BLOB NOT NULL,
    check (id = 1)
);";

const STORAGE_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS storage (
    domain BLOB NOT NULL,
    key BLOB NOT NULL,
    value_crc32 INTEGER NOT NULL,
    value BLOB NOT NULL,
    PRIMARY KEY (domain, key)
) WITHOUT ROWID;";

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error),
    Invariant(&'static str),
    DuplicateKey(rusqlite::Error),
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

fn check_if_updated_rows_zero(result: rusqlite::Result<usize>, msg: &'static str) -> Result<()> {
    let updated_rows = result?;
    if updated_rows == 0 {
        Ok(())
    } else {
        Err(Error::Invariant(msg))
    }
}

fn run_vacuum(conn: &Connection) -> Result<()> {
    check_if_updated_rows_zero(
        conn.execute("vacuum;", []),
        "run vacuum updated_rows not 0",
    )
}

fn ensure_checksum_enabled(conn: &Connection) -> Result<()> {
    let enabled: String = conn.query_row(
        "PRAGMA checksum_verification;", [],
        |r| r.get(0),
    )?;
    if enabled != "1" {
        return Err(Error::Invariant("checksum_verification not enabled"));
    }
    Ok(())
}

fn set_synchronous(conn: &Connection) -> Result<()> {
    check_if_updated_rows_zero(
        conn.execute("PRAGMA synchronous = EXTRA;", []),
        "set synchronous updated_rows not 0",
    )?;
    check_if_updated_rows_zero(
        conn.execute("PRAGMA fullfsync = true;", []),
        "set fullfsync updated_rows not 0",
    )
}

fn check_if_database_is_new(conn: &Connection) -> Result<bool> {
    let count: u32 = conn.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type='table';", [],
        |r| r.get(0),
    )?;
    Ok(count == 0)
}

fn check_or_write_version(conn: &Connection) -> Result<()> {
    let magic: i32 = conn.query_row(
        "PRAGMA application_id;", [],
        |r| r.get(0),
    )?;
    let version: u32 = conn.query_row(
        "PRAGMA user_version;", [],
        |r| r.get(0),
    )?;
    let database_is_new = check_if_database_is_new(conn)?;
    match (magic, version, database_is_new) {
        (0, 0, true) => {
            check_if_updated_rows_zero(
                conn.execute(SET_MAGIC_STMT, []),
                "set magic updated_rows not 0",
            )?;
            check_if_updated_rows_zero(
                conn.execute(SET_VERSION_STMT, []),
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
    check_if_updated_rows_zero(
        conn.execute(METADATA_SCHEMA, []),
        "init metadata schema updated_rows not 0",
    )?;
    check_if_updated_rows_zero(
        conn.execute(STORAGE_SCHEMA, []),
        "init storage schema updated_rows not 0",
    )
}

struct Metadata {
    ident: Box<[u8]>,
}

fn check_or_write_metadata(conn: &Connection, ident: &[u8]) -> Result<()> {
    let mut stmt = conn.prepare("SELECT * FROM metadata WHERE id = 1")?;
    let mut metadata_iter = stmt.query_map([], |row| {
        Ok(Metadata {
            ident: row.get(1)?,
        })
    })?;
    match metadata_iter.next() {
        None => {
            let updated_rows = conn.execute(
                "INSERT INTO metadata (ident) VALUES (?)",
                params![ident],
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

fn filter_error_duplicate_key(err: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(code, _msg) = &err
        && code.extended_code == 1555
        // && let Some(msg) = msg
        // && msg == "UNIQUE constraint failed: storage.domain, storage.key"
    {
        Error::DuplicateKey(err)
    } else {
        Error::Rusqlite(err)
    }
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
        ensure_checksum_enabled(&conn)?;
        check_or_write_version(&conn)?;
        init_schema(&conn)?;
        check_or_write_metadata(&conn, ident)?;
        Ok(Self { conn })
    }

    pub fn write_kv(&self, domain: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        let value_crc32 = crc32(value);
        let result = self.conn.execute(
            "INSERT INTO storage (domain, key, value_crc32, value) VALUES (?, ?, ?, ?)",
            params![domain, key, value_crc32, value],
        );
        match result {
            Ok(updated_rows) => {
                if updated_rows != 1 {
                    return Err(Error::Invariant("write_kv updated_rows not 1"));
                }
                Ok(())
            }
            Err(err) => Err(filter_error_duplicate_key(err)),
        }
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
