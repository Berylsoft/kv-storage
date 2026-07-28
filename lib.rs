use rusqlite::{Connection, ffi, params};
use std::path::Path;

#[derive(Debug)]
pub enum Error {
    Rusqlite(rusqlite::Error),
    Invariant(&'static str),
    DuplicateKey(rusqlite::Error),
}

impl From<rusqlite::Error> for Error {
    fn from(value: rusqlite::Error) -> Self {
        Error::Rusqlite(value)
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
        "error calling sqlite3_register_cksumvfs"
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

fn run_vaccum(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("vacuum;")
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

fn set_synchronous(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA synchronous = EXTRA;
        PRAGMA fullfsync = true;",
    )
}

fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv_storage (
            domain BLOB NOT NULL,
            key BLOB NOT NULL,
            value BLOB NOT NULL,
            PRIMARY KEY (domain, key)
        ) WITHOUT ROWID;",
    )
}

fn filter_error_duplicate_key(err: rusqlite::Error) -> Error {
    if let rusqlite::Error::SqliteFailure(code, _msg) = &err
        && code.extended_code == 1555
        // && let Some(msg) = msg
        // && msg == "UNIQUE constraint failed: kv_storage.domain, kv_storage.key"
    {
        Error::DuplicateKey(err)
    } else {
        Error::Rusqlite(err)
    }
}

pub struct WriteContext {
    conn: Connection,
}

impl WriteContext {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        register_cksumvfs()?;
        let conn = Connection::open(path)?;
        set_reserve_bytes(&conn)?;
        run_vaccum(&conn)?;
        set_synchronous(&conn)?;
        ensure_checksum_enabled(&conn)?;
        init_schema(&conn)?;
        Ok(Self { conn })
    }

    pub fn write_kv(&self, domain: &[u8], key: &[u8], value: &[u8]) -> Result<()> {
        let result = self.conn.execute(
            "INSERT INTO kv_storage (domain, key, value) VALUES (?, ?, ?)",
            params![domain, key, value],
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
}
